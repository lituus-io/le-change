//! GitHub REST API client for pull request file fetching

use crate::error::{Error, Result};
use crate::types::{ChangedFile, ChangeType, DiffResult};
use crate::interner::StringInterner;
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

/// GitHub API client for fetching PR files
pub struct GitHubApiClient {
    client: reqwest::Client,
    base_url: String,
    token: Option<String>,
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
            let mut request = self.client
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

            let files: Vec<GitHubFile> = response
                .json()
                .await
                .map_err(|e| Error::Runtime(format!("Failed to parse GitHub API response: {}", e)))?;

            if files.is_empty() {
                break;
            }

            all_files.extend(files);
            page += 1;

            // Safety limit to prevent infinite loops
            if page > 1000 {
                return Err(Error::Runtime("Too many pages in GitHub API response".to_string()));
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

            let previous_path = file.previous_filename
                .as_ref()
                .map(|p| interner.intern(p));

            result.files.push(ChangedFile {
                path: interner.intern(&file.filename),
                change_type,
                previous_path,
                is_symlink: false, // GitHub API doesn't provide symlink info
                submodule_depth: 0,
            });
        }

        Ok(result)
    }

    /// Extract owner, repo, and PR number from GitHub environment
    pub fn extract_pr_info_from_env() -> Result<(String, String, u32)> {
        // Try GITHUB_REPOSITORY (format: owner/repo)
        let repository = std::env::var("GITHUB_REPOSITORY")
            .map_err(|_| Error::Config("GITHUB_REPOSITORY not set".to_string()))?;

        let parts: Vec<&str> = repository.split('/').collect();
        if parts.len() != 2 {
            return Err(Error::Config(format!("Invalid GITHUB_REPOSITORY format: {}", repository)));
        }

        let owner = parts[0].to_string();
        let repo = parts[1].to_string();

        // Try to get PR number from GITHUB_REF (format: refs/pull/123/merge)
        let github_ref = std::env::var("GITHUB_REF")
            .map_err(|_| Error::Config("GITHUB_REF not set".to_string()))?;

        let pr_number = if github_ref.starts_with("refs/pull/") {
            let pr_parts: Vec<&str> = github_ref.split('/').collect();
            if pr_parts.len() >= 3 {
                pr_parts[2].parse::<u32>()
                    .map_err(|_| Error::Config(format!("Invalid PR number in GITHUB_REF: {}", github_ref)))?
            } else {
                return Err(Error::Config(format!("Invalid GITHUB_REF format: {}", github_ref)));
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
    fn test_extract_pr_info_invalid_format() {
        // Save original env vars
        let original_repo = std::env::var("GITHUB_REPOSITORY").ok();
        let original_ref = std::env::var("GITHUB_REF").ok();

        // Set invalid format
        std::env::set_var("GITHUB_REPOSITORY", "invalid");
        std::env::set_var("GITHUB_REF", "refs/pull/123/merge");

        let result = GitHubApiClient::extract_pr_info_from_env();
        assert!(result.is_err());

        // Restore original env vars
        if let Some(repo) = original_repo {
            std::env::set_var("GITHUB_REPOSITORY", repo);
        } else {
            std::env::remove_var("GITHUB_REPOSITORY");
        }
        if let Some(ref_val) = original_ref {
            std::env::set_var("GITHUB_REF", ref_val);
        } else {
            std::env::remove_var("GITHUB_REF");
        }
    }
}
