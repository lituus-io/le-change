//! SHA resolution with complex fallback chains for GitHub Actions

use crate::error::{Error, Result};
use crate::types::InputConfig;
use std::path::Path;

/// SHA resolver for Git references
pub struct ShaResolver {
    repo_path: std::path::PathBuf,
}

impl ShaResolver {
    /// Create a new SHA resolver for a repository
    pub fn new<P: AsRef<Path>>(repo_path: P) -> Self {
        Self {
            repo_path: repo_path.as_ref().to_path_buf(),
        }
    }

    /// Resolve the current (head) SHA based on config and environment
    pub fn resolve_current_sha(&self, config: &InputConfig) -> Result<String> {
        let repo = git2::Repository::open(&self.repo_path)?;

        // 1. If `until` date provided: Last commit before date
        if let Some(until) = &config.until {
            return self.commit_before_date(&repo, until.as_ref());
        }

        // 2. If `sha` explicitly provided: Validate and use
        if let Some(sha) = &config.sha {
            return self.validate_sha(&repo, sha.as_ref());
        }

        // 3. If PR context: Use PR head SHA from environment
        if let Ok(pr_head) = std::env::var("GITHUB_HEAD_REF") {
            if !pr_head.is_empty() {
                return self.resolve_ref(&repo, &pr_head);
            }
        }

        // 4. Default: Use HEAD
        self.resolve_ref(&repo, "HEAD")
    }

    /// Resolve the base SHA based on config and environment
    pub fn resolve_base_sha(&self, config: &InputConfig) -> Result<String> {
        let repo = git2::Repository::open(&self.repo_path)?;

        // 1. If `base_sha` explicitly provided: Validate and use
        if let Some(base_sha) = &config.base_sha {
            return self.validate_sha(&repo, base_sha.as_ref());
        }

        // 2. If `since` date provided: Last commit before date
        if let Some(since) = &config.since {
            return self.commit_before_date(&repo, since.as_ref());
        }

        // 3. If PR context: Use PR base SHA from environment
        if let Ok(pr_base) = std::env::var("GITHUB_BASE_REF") {
            if !pr_base.is_empty() {
                return self.resolve_ref(&repo, &pr_base);
            }
        }

        // 4. Default: Use HEAD^
        self.resolve_ref(&repo, "HEAD^")
    }

    /// Resolve a reference to a full SHA
    fn resolve_ref(&self, repo: &git2::Repository, reference: &str) -> Result<String> {
        // Try as direct OID first
        if let Ok(oid) = git2::Oid::from_str(reference) {
            // Verify it exists
            if repo.find_object(oid, None).is_ok() {
                return Ok(oid.to_string());
            }
        }

        // Try as reference (branch, tag, HEAD, etc.)
        let resolved = repo.revparse_single(reference)
            .map_err(|e| Error::Git(format!("Failed to resolve reference '{}': {}", reference, e)))?;

        Ok(resolved.id().to_string())
    }

    /// Validate that a SHA exists in the repository
    fn validate_sha(&self, repo: &git2::Repository, sha: &str) -> Result<String> {
        let oid = git2::Oid::from_str(sha)
            .map_err(|e| Error::Git(format!("Invalid SHA '{}': {}", sha, e)))?;

        // Verify it exists
        repo.find_object(oid, None)
            .map_err(|e| Error::Git(format!("SHA '{}' not found in repository: {}", sha, e)))?;

        Ok(oid.to_string())
    }

    /// Find the last commit before a given date
    fn commit_before_date(&self, repo: &git2::Repository, date_str: &str) -> Result<String> {
        // Parse the date string
        let target_time = self.parse_date(date_str)?;

        // Walk commits from HEAD backwards
        let mut revwalk = repo.revwalk()?;
        revwalk.push_head()?;
        revwalk.set_sorting(git2::Sort::TIME)?;

        for oid in revwalk {
            let oid = oid?;
            let commit = repo.find_commit(oid)?;
            let commit_time = commit.time().seconds();

            if commit_time <= target_time {
                return Ok(oid.to_string());
            }
        }

        Err(Error::Git(format!("No commits found before date '{}'", date_str)))
    }

    /// Parse a date string to Unix timestamp
    fn parse_date(&self, date_str: &str) -> Result<i64> {
        // Try parsing ISO 8601 format: YYYY-MM-DD or YYYY-MM-DDTHH:MM:SS
        // For now, use git's date parsing via command line
        let output = std::process::Command::new("git")
            .args(["log", "-1", "--format=%ct", &format!("--before={}", date_str)])
            .current_dir(&self.repo_path)
            .output()
            .map_err(|e| Error::Git(format!("Failed to parse date: {}", e)))?;

        if !output.status.success() {
            return Err(Error::Git(format!("Failed to parse date '{}'", date_str)));
        }

        let timestamp_str = String::from_utf8_lossy(&output.stdout);
        timestamp_str.trim()
            .parse::<i64>()
            .map_err(|e| Error::Git(format!("Failed to parse timestamp from git: {}", e)))
    }

    /// Get merge base between two commits (for three-dot diff)
    pub fn merge_base(&self, commit1: &str, commit2: &str) -> Result<String> {
        let repo = git2::Repository::open(&self.repo_path)?;

        let oid1 = git2::Oid::from_str(commit1)
            .map_err(|e| Error::Git(format!("Invalid SHA '{}': {}", commit1, e)))?;
        let oid2 = git2::Oid::from_str(commit2)
            .map_err(|e| Error::Git(format!("Invalid SHA '{}': {}", commit2, e)))?;

        let merge_base = repo.merge_base(oid1, oid2)
            .map_err(|e| Error::Git(format!("Failed to find merge base: {}", e)))?;

        Ok(merge_base.to_string())
    }

    /// Check if this is an initial commit (no parent)
    pub fn is_initial_commit(&self, sha: &str) -> Result<bool> {
        let repo = git2::Repository::open(&self.repo_path)?;

        let oid = git2::Oid::from_str(sha)
            .map_err(|e| Error::Git(format!("Invalid SHA '{}': {}", sha, e)))?;

        let commit = repo.find_commit(oid)?;
        Ok(commit.parent_count() == 0)
    }

    /// Get the empty tree SHA (for comparing against initial commits)
    pub fn empty_tree_sha() -> &'static str {
        // Git's well-known empty tree SHA
        "4b825dc642cb6eb9a060e54bf8d69288fbee4904"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_repo() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let repo_path = dir.path().to_path_buf();

        // Initialize git repo
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        std::process::Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        std::process::Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        // Create initial commit
        fs::write(repo_path.join("file1.txt"), "content1").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "Initial commit"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        (dir, repo_path)
    }

    #[test]
    fn test_resolve_head() {
        let (_dir, repo_path) = create_test_repo();
        let resolver = ShaResolver::new(&repo_path);

        let config = InputConfig::default();
        let sha = resolver.resolve_current_sha(&config).unwrap();
        assert_eq!(sha.len(), 40); // SHA is 40 hex characters
    }

    #[test]
    fn test_validate_sha() {
        let (_dir, repo_path) = create_test_repo();
        let resolver = ShaResolver::new(&repo_path);

        // Get HEAD SHA
        let repo = git2::Repository::open(&repo_path).unwrap();
        let head = repo.head().unwrap();
        let head_sha = head.target().unwrap().to_string();

        let config = InputConfig {
            sha: Some(std::borrow::Cow::Owned(head_sha.clone())),
            ..Default::default()
        };

        let resolved = resolver.resolve_current_sha(&config).unwrap();
        assert_eq!(resolved, head_sha);
    }

    #[test]
    fn test_resolve_base_sha() {
        let (_dir, repo_path) = create_test_repo();
        let resolver = ShaResolver::new(&repo_path);

        // Create second commit
        fs::write(repo_path.join("file2.txt"), "content2").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "Second commit"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        let config = InputConfig::default();
        let base_sha = resolver.resolve_base_sha(&config).unwrap();
        assert_eq!(base_sha.len(), 40);
    }

    #[test]
    fn test_is_initial_commit() {
        let (_dir, repo_path) = create_test_repo();
        let resolver = ShaResolver::new(&repo_path);

        let config = InputConfig::default();
        let sha = resolver.resolve_current_sha(&config).unwrap();

        // This should be the initial commit
        assert!(resolver.is_initial_commit(&sha).unwrap());
    }

    #[test]
    fn test_merge_base() {
        let (_dir, repo_path) = create_test_repo();

        // Create a branch
        std::process::Command::new("git")
            .args(["checkout", "-b", "feature"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        fs::write(repo_path.join("feature.txt"), "feature").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "Feature commit"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        let resolver = ShaResolver::new(&repo_path);
        let repo = git2::Repository::open(&repo_path).unwrap();

        let main_sha = repo.revparse_single("main").unwrap().id().to_string();
        let feature_sha = repo.revparse_single("feature").unwrap().id().to_string();

        let merge_base = resolver.merge_base(&main_sha, &feature_sha).unwrap();
        assert_eq!(merge_base.len(), 40);
        assert_eq!(merge_base, main_sha); // merge base should be main commit
    }
}
