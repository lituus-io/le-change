//! SHA resolution with complex fallback chains for GitHub Actions

use crate::error::{Error, Result};
use crate::types::InputConfig;
use std::path::Path;

/// GitHub event type for event-aware SHA resolution
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitHubEvent {
    /// Pull request event
    PullRequest,
    /// Pull request target event
    PullRequestTarget,
    /// Push event
    Push,
    /// Merge group event
    MergeGroup,
    /// Release event
    Release,
    /// Tag event
    Tag,
    /// Workflow dispatch event
    WorkflowDispatch,
    /// Workflow call event
    WorkflowCall,
    /// Scheduled event
    Schedule,
    /// Unknown event type
    Unknown,
}

impl GitHubEvent {
    /// Parse from GITHUB_EVENT_NAME environment variable
    pub fn from_env() -> Self {
        match std::env::var("GITHUB_EVENT_NAME").as_deref() {
            Ok("pull_request") => Self::PullRequest,
            Ok("pull_request_target") => Self::PullRequestTarget,
            Ok("push") => Self::Push,
            Ok("merge_group") => Self::MergeGroup,
            Ok("release") => Self::Release,
            Ok("create") | Ok("tag") => Self::Tag,
            Ok("workflow_dispatch") => Self::WorkflowDispatch,
            Ok("workflow_call") => Self::WorkflowCall,
            Ok("schedule") => Self::Schedule,
            _ => Self::Unknown,
        }
    }
}

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
    pub fn resolve_current_sha(&self, config: &InputConfig<'_>) -> Result<String> {
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
    pub fn resolve_base_sha(&self, config: &InputConfig<'_>) -> Result<String> {
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

        // 4. Default: Use HEAD^ (fall back to empty tree for initial commit)
        self.resolve_ref(&repo, "HEAD^")
            .or_else(|_| Ok(Self::empty_tree_sha().to_string()))
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
        let resolved = repo.revparse_single(reference).map_err(|e| {
            Error::Git(format!(
                "Failed to resolve reference '{}': {}",
                reference, e
            ))
        })?;

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

        Err(Error::Git(format!(
            "No commits found before date '{}'",
            date_str
        )))
    }

    /// Parse a date string to Unix timestamp
    fn parse_date(&self, date_str: &str) -> Result<i64> {
        // Try parsing ISO 8601 format: YYYY-MM-DD or YYYY-MM-DDTHH:MM:SS
        // For now, use git's date parsing via command line
        let output = std::process::Command::new("git")
            .args([
                "log",
                "-1",
                "--format=%ct",
                &format!("--before={}", date_str),
            ])
            .current_dir(&self.repo_path)
            .output()
            .map_err(|e| Error::Git(format!("Failed to parse date: {}", e)))?;

        if !output.status.success() {
            return Err(Error::Git(format!("Failed to parse date '{}'", date_str)));
        }

        let timestamp_str = String::from_utf8_lossy(&output.stdout);
        timestamp_str
            .trim()
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

        let merge_base = repo
            .merge_base(oid1, oid2)
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

    /// Event-aware SHA resolution with 8+ decision paths
    ///
    /// Resolution priority:
    /// 1. Explicit config (base_sha/sha) — always wins
    /// 2. Event-specific logic (PR, Push, MergeGroup, Tag, etc.)
    /// 3. Default fallback (HEAD^..HEAD)
    pub fn resolve_event_aware(&self, config: &InputConfig<'_>) -> Result<(String, String)> {
        // If both SHAs are explicitly configured, use them directly
        if config.base_sha.is_some() && config.sha.is_some() {
            let base = self.resolve_base_sha(config)?;
            let head = self.resolve_current_sha(config)?;
            return Ok((base, head));
        }

        // If date-based, delegate to existing methods
        if config.since.is_some() || config.until.is_some() {
            let base = self.resolve_base_sha(config)?;
            let head = self.resolve_current_sha(config)?;
            return Ok((base, head));
        }

        let event = GitHubEvent::from_env();

        match event {
            GitHubEvent::PullRequest | GitHubEvent::PullRequestTarget => {
                // PR: use GITHUB_BASE_REF..GITHUB_HEAD_REF (or PR merge commit)
                let base = if config.base_sha.is_some() {
                    self.resolve_base_sha(config)?
                } else if let Ok(base_ref) = std::env::var("GITHUB_BASE_REF") {
                    if !base_ref.is_empty() {
                        let repo = git2::Repository::open(&self.repo_path)?;
                        self.resolve_ref(&repo, &format!("origin/{}", base_ref))?
                    } else {
                        self.resolve_base_sha(config)?
                    }
                } else {
                    self.resolve_base_sha(config)?
                };
                let head = self.resolve_current_sha(config)?;
                Ok((base, head))
            }

            GitHubEvent::Push => {
                // Push: use event payload's before..after if available
                let base = if config.base_sha.is_some() {
                    self.resolve_base_sha(config)?
                } else if let Ok(event_path) = std::env::var("GITHUB_EVENT_PATH") {
                    self.resolve_push_base_from_event(&event_path)?
                } else {
                    self.resolve_base_sha(config)?
                };
                let head = self.resolve_current_sha(config)?;
                Ok((base, head))
            }

            GitHubEvent::MergeGroup => {
                // Merge group: GITHUB_SHA is the merge commit
                let head = self.resolve_current_sha(config)?;
                let repo = git2::Repository::open(&self.repo_path)?;
                // For merge groups, compare against the merge base
                let oid = git2::Oid::from_str(&head)?;
                let commit = repo.find_commit(oid)?;
                if commit.parent_count() > 0 {
                    let parent = commit.parent(0)?;
                    Ok((parent.id().to_string(), head))
                } else {
                    Ok((Self::empty_tree_sha().to_string(), head))
                }
            }

            GitHubEvent::Release | GitHubEvent::Tag => {
                // Tag/release: compare against previous tag if configured
                if config.tags_pattern.is_some() {
                    if let Ok(prev_tag) = self.find_previous_tag(config) {
                        let head = self.resolve_current_sha(config)?;
                        return Ok((prev_tag, head));
                    }
                }
                // Fallback: HEAD^..HEAD
                let base = self.resolve_base_sha(config)?;
                let head = self.resolve_current_sha(config)?;
                Ok((base, head))
            }

            GitHubEvent::WorkflowDispatch | GitHubEvent::WorkflowCall | GitHubEvent::Schedule => {
                // Manual/scheduled: use explicit config or default HEAD^..HEAD
                let base = self.resolve_base_sha(config)?;
                let head = self.resolve_current_sha(config)?;
                Ok((base, head))
            }

            GitHubEvent::Unknown => {
                // Unknown event or local development: default resolution
                let base = self.resolve_base_sha(config)?;
                let head = self.resolve_current_sha(config)?;
                Ok((base, head))
            }
        }
    }

    /// Extract base SHA from push event payload JSON
    fn resolve_push_base_from_event(&self, event_path: &str) -> Result<String> {
        let content = std::fs::read_to_string(event_path)
            .map_err(|e| Error::EventParse(format!("Failed to read event file: {}", e)))?;

        let event: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| Error::EventParse(format!("Failed to parse event JSON: {}", e)))?;

        if let Some(before) = event.get("before").and_then(|v| v.as_str()) {
            // Check if "before" is the null SHA (new branch push)
            if before == "0000000000000000000000000000000000000000" {
                // New branch push: try merge-base with default branch first
                let repo = git2::Repository::open(&self.repo_path)?;
                for default_ref in &["origin/main", "origin/master"] {
                    if let Ok(target) = self.resolve_ref(&repo, default_ref) {
                        let head = self.resolve_ref(&repo, "HEAD")?;
                        if let Ok(merge_base) = self.merge_base(&head, &target) {
                            return Ok(merge_base);
                        }
                    }
                }
                // Truly initial repo — compare against empty tree
                return Ok(Self::empty_tree_sha().to_string());
            }

            // Validate the SHA exists in the repo
            let repo = git2::Repository::open(&self.repo_path)?;
            match self.validate_sha(&repo, before) {
                Ok(sha) => return Ok(sha),
                Err(_) => {
                    // SHA not in local repo (shallow clone), fall back to HEAD^
                    // or empty tree for initial commit
                    return self
                        .resolve_ref(&repo, "HEAD^")
                        .or_else(|_| Ok(Self::empty_tree_sha().to_string()));
                }
            }
        }

        // No "before" in event, fall back to HEAD^ or empty tree for initial commit
        let repo = git2::Repository::open(&self.repo_path)?;
        self.resolve_ref(&repo, "HEAD^")
            .or_else(|_| Ok(Self::empty_tree_sha().to_string()))
    }

    /// Find the previous tag matching the configured pattern
    ///
    /// Walks tags sorted by commit time, finds the most recent tag before HEAD
    /// that matches `tags_pattern` and doesn't match `tags_ignore_pattern`.
    pub fn find_previous_tag(&self, config: &InputConfig<'_>) -> Result<String> {
        let repo = git2::Repository::open(&self.repo_path)?;

        let pattern = config.tags_pattern.as_ref().ok_or_else(|| {
            Error::Config("tags_pattern is required for tag comparison".to_string())
        })?;

        // Build glob matchers
        let include_glob = globset::Glob::new(pattern)
            .map_err(|e| Error::Pattern(format!("Invalid tags_pattern '{}': {}", pattern, e)))?
            .compile_matcher();

        let exclude_matcher = config
            .tags_ignore_pattern
            .as_ref()
            .map(|p| {
                globset::Glob::new(p)
                    .map_err(|e| {
                        Error::Pattern(format!("Invalid tags_ignore_pattern '{}': {}", p, e))
                    })
                    .map(|g| g.compile_matcher())
            })
            .transpose()?;

        // Get current HEAD for comparison
        let head_oid = repo.revparse_single("HEAD")?.id();

        // Collect matching tags with their commit times
        let mut matching_tags: Vec<(String, i64)> = Vec::new();

        repo.tag_foreach(|oid, name_bytes| {
            let name = String::from_utf8_lossy(name_bytes);
            // Strip refs/tags/ prefix
            let tag_name = name.strip_prefix("refs/tags/").unwrap_or(&name).to_string();

            // Check include pattern
            if !include_glob.is_match(&tag_name) {
                return true; // Continue iteration
            }

            // Check exclude pattern
            if let Some(ref exclude) = exclude_matcher {
                if exclude.is_match(&tag_name) {
                    return true; // Continue, this tag is excluded
                }
            }

            // Peel to commit to get the time
            if let Ok(obj) = repo.find_object(oid, None) {
                let peeled = obj.peel(git2::ObjectType::Commit).ok();
                if let Some(commit_obj) = peeled {
                    if commit_obj.id() != head_oid {
                        if let Ok(commit) = commit_obj.into_commit() {
                            let time = commit.time().seconds();
                            matching_tags.push((tag_name, time));
                        }
                    }
                }
            }

            true // Continue iteration
        })?;

        if matching_tags.is_empty() {
            return Err(Error::Git(format!(
                "No previous tags found matching pattern '{}'",
                pattern
            )));
        }

        // Sort by time descending, take the most recent
        matching_tags.sort_by_key(|t| std::cmp::Reverse(t.1));

        let tag_name = &matching_tags[0].0;
        self.resolve_ref(&repo, tag_name)
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
    fn test_resolve_event_aware_default() {
        let (_dir, repo_path) = create_test_repo();

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

        let resolver = ShaResolver::new(&repo_path);
        let config = InputConfig::default();

        // With no event env vars, should fall back to HEAD^..HEAD
        let result = resolver.resolve_event_aware(&config);
        assert!(result.is_ok());

        let (base, head) = result.unwrap();
        assert_eq!(base.len(), 40);
        assert_eq!(head.len(), 40);
        assert_ne!(base, head);
    }

    #[test]
    fn test_resolve_event_aware_explicit_shas() {
        let (_dir, repo_path) = create_test_repo();

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

        let resolver = ShaResolver::new(&repo_path);

        // Get HEAD and HEAD^ SHAs
        let repo = git2::Repository::open(&repo_path).unwrap();
        let head = repo.revparse_single("HEAD").unwrap().id().to_string();
        let base = repo.revparse_single("HEAD^").unwrap().id().to_string();

        let config = InputConfig {
            base_sha: Some(std::borrow::Cow::Owned(base.clone())),
            sha: Some(std::borrow::Cow::Owned(head.clone())),
            ..Default::default()
        };

        let result = resolver.resolve_event_aware(&config).unwrap();
        assert_eq!(result.0, base);
        assert_eq!(result.1, head);
    }

    #[test]
    fn test_find_previous_tag() {
        let (_dir, repo_path) = create_test_repo();

        // Tag the initial commit
        std::process::Command::new("git")
            .args(["tag", "v0.1.0"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

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

        // Tag the second commit
        std::process::Command::new("git")
            .args(["tag", "v0.2.0"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        // Create third commit (HEAD)
        fs::write(repo_path.join("file3.txt"), "content3").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "Third commit"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        let resolver = ShaResolver::new(&repo_path);
        let config = InputConfig {
            tags_pattern: Some(std::borrow::Cow::Borrowed("v*")),
            ..Default::default()
        };

        let result = resolver.find_previous_tag(&config);
        assert!(result.is_ok());
        let tag_sha = result.unwrap();
        assert_eq!(tag_sha.len(), 40);
    }

    #[test]
    fn test_find_previous_tag_with_ignore() {
        let (_dir, repo_path) = create_test_repo();

        // Tag initial commit
        std::process::Command::new("git")
            .args(["tag", "v0.1.0-rc1"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        // Create second commit + tag
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

        // Create third commit (HEAD)
        fs::write(repo_path.join("file3.txt"), "content3").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "Third commit"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        let resolver = ShaResolver::new(&repo_path);
        let config = InputConfig {
            tags_pattern: Some(std::borrow::Cow::Borrowed("v*")),
            tags_ignore_pattern: Some(std::borrow::Cow::Borrowed("*-rc*")),
            ..Default::default()
        };

        // Only v0.1.0-rc1 exists but it's excluded by ignore pattern
        let result = resolver.find_previous_tag(&config);
        assert!(result.is_err()); // No matching tags
    }

    #[test]
    fn test_merge_base() {
        let (_dir, repo_path) = create_test_repo();

        let repo = git2::Repository::open(&repo_path).unwrap();

        // Get the current branch name instead of assuming "main"
        let head = repo.head().unwrap();
        let base_sha = head.target().unwrap().to_string();

        // Create a feature branch
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
        let feature_sha = repo.revparse_single("feature").unwrap().id().to_string();

        let merge_base = resolver.merge_base(&base_sha, &feature_sha).unwrap();
        assert_eq!(merge_base.len(), 40);
        assert_eq!(merge_base, base_sha); // merge base should be the initial commit
    }
}
