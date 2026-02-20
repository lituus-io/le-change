//! GitHub REST API client for pull request file fetching

use crate::error::{Error, Result};
use crate::interner::StringInterner;
use crate::types::{ChangeType, ChangedFile, DiffResult};
use serde::Deserialize;

/// GitHub API response for a changed file
#[derive(Debug, Deserialize)]
struct GitHubFile {
    /// File path
    filename: String,
    /// Change status (added, removed, modified, renamed, copied, changed)
    status: String,
    /// Previous filename (for renamed files)
    previous_filename: Option<String>,
}

/// GitHub API response for comparing two refs
#[derive(Debug, Deserialize)]
struct GitHubCompareResponse {
    #[allow(dead_code)]
    total_commits: u32,
    files: Vec<GitHubFile>,
    /// Whether the file list was truncated (too many files)
    #[serde(default)]
    #[allow(dead_code)]
    files_truncated: bool,
}

/// GitHub API client for fetching PR files
pub struct GitHubApiClient {
    client: reqwest::Client,
    base_url: String,
    token: Option<String>,
}

impl std::fmt::Debug for GitHubApiClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitHubApiClient")
            .field("base_url", &self.base_url)
            .field("token", &self.token.as_ref().map(|_| "<redacted>"))
            .finish_non_exhaustive()
    }
}

impl GitHubApiClient {
    /// Create a new GitHub API client
    pub fn new(base_url: String, token: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .user_agent("lechange/0.1.0")
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            client,
            base_url,
            token,
        }
    }

    /// Create from environment variables
    pub fn from_env() -> Result<Self> {
        let base_url = std::env::var("GITHUB_API_URL")
            .unwrap_or_else(|_| "https://api.github.com".to_string());

        let token = std::env::var("GITHUB_TOKEN").ok();

        Ok(Self::new(base_url, token))
    }

    /// Fetch changed files for a pull request
    pub async fn fetch_changed_files(
        &self,
        owner: &str,
        repo: &str,
        pr_number: u32,
        interner: &StringInterner,
    ) -> Result<DiffResult> {
        let url = format!(
            "{}/repos/{}/{}/pulls/{}/files",
            self.base_url, owner, repo, pr_number
        );

        let mut all_files = Vec::new();
        let mut page = 1;

        // Paginate through all results (GitHub returns max 100 per page)
        loop {
            let mut request = self
                .client
                .get(&url)
                .query(&[("page", page.to_string()), ("per_page", "100".to_string())]);

            // Add authorization header if token is present
            if let Some(ref token) = self.token {
                request = request.header("Authorization", format!("Bearer {}", token));
            }

            let response = request
                .send()
                .await
                .map_err(|e| Error::Runtime(format!("GitHub API request failed: {}", e)))?;

            if !response.status().is_success() {
                return Err(Error::Runtime(format!(
                    "GitHub API returned error: {}",
                    response.status()
                )));
            }

            let files: Vec<GitHubFile> = response.json().await.map_err(|e| {
                Error::Runtime(format!("Failed to parse GitHub API response: {}", e))
            })?;

            if files.is_empty() {
                break;
            }

            all_files.extend(files);
            page += 1;

            // Safety limit to prevent infinite loops
            if page > 1000 {
                return Err(Error::Runtime(
                    "Too many pages in GitHub API response".to_string(),
                ));
            }
        }

        // Convert GitHub files to DiffResult
        let mut result = DiffResult::default();

        for file in all_files {
            let change_type = match file.status.as_str() {
                "added" => ChangeType::Added,
                "removed" => ChangeType::Deleted,
                "modified" => ChangeType::Modified,
                "renamed" => ChangeType::Renamed,
                "copied" => ChangeType::Copied,
                "changed" => ChangeType::TypeChanged,
                _ => ChangeType::Unknown,
            };

            let previous_path = file.previous_filename.as_ref().map(|p| interner.intern(p));

            result.files.push(ChangedFile {
                path: interner.intern(&file.filename),
                change_type,
                previous_path,
                is_symlink: false, // GitHub API doesn't provide symlink info
                submodule_depth: 0,
                origin: crate::types::FileOrigin {
                    in_current_changes: true,
                    in_previous_failure: false,
                    in_previous_success: false,
                },
            });
        }

        Ok(result)
    }

    /// Compare two refs and get changed files
    ///
    /// Endpoint: GET /repos/{owner}/{repo}/compare/{base}...{head}
    /// Useful for non-PR contexts where git diff is not available.
    pub async fn compare_refs(
        &self,
        owner: &str,
        repo: &str,
        base: &str,
        head: &str,
        interner: &StringInterner,
    ) -> Result<DiffResult> {
        let url = format!(
            "{}/repos/{}/{}/compare/{}...{}",
            self.base_url, owner, repo, base, head
        );

        let mut all_files = Vec::new();
        let mut page = 1;

        loop {
            let mut request = self
                .client
                .get(&url)
                .query(&[("page", page.to_string()), ("per_page", "100".to_string())]);

            if let Some(ref token) = self.token {
                request = request.header("Authorization", format!("Bearer {}", token));
            }

            let response = request
                .send()
                .await
                .map_err(|e| Error::Runtime(format!("GitHub API compare request failed: {}", e)))?;

            if !response.status().is_success() {
                return Err(Error::Runtime(format!(
                    "GitHub API compare returned error: {}",
                    response.status()
                )));
            }

            let compare: GitHubCompareResponse = response.json().await.map_err(|e| {
                Error::Runtime(format!("Failed to parse GitHub compare response: {}", e))
            })?;

            all_files.extend(compare.files);

            // GitHub compare API paginates via Link header for large diffs
            if all_files.len() >= compare.total_commits as usize * 100 || compare.files_truncated {
                break; // We've gotten what we can
            }

            if all_files.len() < 100 * page as usize {
                break; // Less than a full page, we're done
            }

            page += 1;

            if page > 100 {
                break; // Safety limit
            }
        }

        // Convert to DiffResult
        let mut result = DiffResult::default();

        for file in all_files {
            let change_type = match file.status.as_str() {
                "added" => ChangeType::Added,
                "removed" => ChangeType::Deleted,
                "modified" => ChangeType::Modified,
                "renamed" => ChangeType::Renamed,
                "copied" => ChangeType::Copied,
                "changed" => ChangeType::TypeChanged,
                _ => ChangeType::Unknown,
            };

            let previous_path = file.previous_filename.as_ref().map(|p| interner.intern(p));

            result.files.push(ChangedFile {
                path: interner.intern(&file.filename),
                change_type,
                previous_path,
                is_symlink: false,
                submodule_depth: 0,
                origin: crate::types::FileOrigin {
                    in_current_changes: true,
                    in_previous_failure: false,
                    in_previous_success: false,
                },
            });
        }

        Ok(result)
    }

    /// Extract owner, repo, and PR number from GitHub environment
    pub fn extract_pr_info_from_env() -> Result<(String, String, u32)> {
        let repository = std::env::var("GITHUB_REPOSITORY")
            .map_err(|_| Error::Config("GITHUB_REPOSITORY not set".to_string()))?;
        let github_ref = std::env::var("GITHUB_REF")
            .map_err(|_| Error::Config("GITHUB_REF not set".to_string()))?;
        Self::parse_pr_info(&repository, &github_ref)
    }

    /// Parse owner, repo, and PR number from repository and ref strings
    fn parse_pr_info(repository: &str, github_ref: &str) -> Result<(String, String, u32)> {
        let parts: Vec<&str> = repository.split('/').collect();
        if parts.len() != 2 {
            return Err(Error::Config(format!(
                "Invalid GITHUB_REPOSITORY format: {}",
                repository
            )));
        }

        let owner = parts[0].to_string();
        let repo = parts[1].to_string();

        let pr_number = if github_ref.starts_with("refs/pull/") {
            let pr_parts: Vec<&str> = github_ref.split('/').collect();
            if pr_parts.len() >= 3 {
                pr_parts[2].parse::<u32>().map_err(|_| {
                    Error::Config(format!("Invalid PR number in GITHUB_REF: {}", github_ref))
                })?
            } else {
                return Err(Error::Config(format!(
                    "Invalid GITHUB_REF format: {}",
                    github_ref
                )));
            }
        } else {
            return Err(Error::Config("Not a pull request event".to_string()));
        };

        Ok((owner, repo, pr_number))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_github_client_creation() {
        let client = GitHubApiClient::new("https://api.github.com".to_string(), None);
        assert_eq!(client.base_url, "https://api.github.com");
        assert!(client.token.is_none());
    }

    #[test]
    fn test_github_client_with_token() {
        let client = GitHubApiClient::new(
            "https://api.github.com".to_string(),
            Some("test_token".to_string()),
        );
        assert_eq!(client.token, Some("test_token".to_string()));
    }

    #[test]
    fn test_parse_pr_info_invalid_format() {
        let result = GitHubApiClient::parse_pr_info("invalid", "refs/pull/123/merge");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_pr_info_happy_path() {
        let result = GitHubApiClient::parse_pr_info("owner/repo", "refs/pull/42/merge");
        assert!(result.is_ok());
        let (owner, repo, pr_number) = result.unwrap();
        assert_eq!(owner, "owner");
        assert_eq!(repo, "repo");
        assert_eq!(pr_number, 42);
    }

    #[test]
    fn test_parse_pr_info_not_a_pr() {
        let result = GitHubApiClient::parse_pr_info("owner/repo", "refs/heads/main");
        assert!(result.is_err());
    }

    #[test]
    fn test_github_client_debug_redacts_token() {
        let client = GitHubApiClient::new(
            "https://api.github.com".to_string(),
            Some("ghp_SuperSecretToken12345".to_string()),
        );
        let debug_output = format!("{:?}", client);
        assert!(
            !debug_output.contains("ghp_SuperSecretToken12345"),
            "Debug output must not contain the actual token: {}",
            debug_output
        );
        assert!(
            debug_output.contains("<redacted>"),
            "Debug output should show <redacted>: {}",
            debug_output
        );
    }

    #[test]
    fn test_github_client_debug_no_token() {
        let client = GitHubApiClient::new("https://api.github.com".to_string(), None);
        let debug_output = format!("{:?}", client);
        assert!(
            !debug_output.contains("<redacted>"),
            "Should not show <redacted> when no token is set"
        );
        assert!(debug_output.contains("token: None"));
    }
}
