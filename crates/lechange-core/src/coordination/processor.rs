//! Main file processing coordinator

use crate::types::{DiffResult, InputConfig};
use crate::interner::StringInterner;
use crate::error::Result;
use crate::git::{GitRepository, ShaResolver, SubmoduleProcessor};
use crate::patterns::matcher::PatternMatcher;
use crate::file_ops::FileOps;
use crate::http::GitHubApiClient;
use crate::traits::AsyncGitOps;
use rayon::prelude::*;
use std::path::Path;

/// File processor that orchestrates the entire detection pipeline
pub struct FileProcessor<'a> {
    git_ops: &'a GitRepository,
    interner: &'a StringInterner,
    config: &'a InputConfig<'a>,
}

impl<'a> FileProcessor<'a> {
    /// Create a new file processor
    pub fn new(
        git_ops: &'a GitRepository,
        interner: &'a StringInterner,
        config: &'a InputConfig<'a>,
    ) -> Self {
        Self {
            git_ops,
            interner,
            config,
        }
    }

    /// Main processing pipeline
    pub async fn process(&self) -> Result<DiffResult> {
        // Step 1: Check if we should use REST API
        if self.config.use_rest_api {
            return self.process_via_rest_api().await;
        }

        // Step 2: Resolve SHAs
        let repo_path = std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."));

        let sha_resolver = ShaResolver::new(&repo_path);
        let base_sha = sha_resolver.resolve_base_sha(self.config)?;
        let head_sha = sha_resolver.resolve_current_sha(self.config)?;

        // Step 3: Compute diff
        let mut diff = self.git_ops.diff(
            &base_sha,
            &head_sha,
            self.interner,
            &self.config.diff_filter,
        ).await?;

        // Step 4: Process submodules if enabled
        if self.config.include_submodules {
            let submodule_diff = self.process_submodules(&base_sha, &head_sha).await?;
            diff.files.extend(submodule_diff.files);
        }

        // Step 5: Filter by patterns (parallel with Rayon)
        if let Some(ref files_patterns) = self.config.files {
            let patterns: Vec<&str> = files_patterns.iter()
                .map(|c| c.as_ref())
                .collect();

            let ignore_patterns: Vec<&str> = self.config.files_ignore.as_ref()
                .map(|ignore| ignore.iter().map(|c| c.as_ref()).collect())
                .unwrap_or_else(Vec::new);

            let matcher = PatternMatcher::new(
                &patterns,
                &ignore_patterns,
                self.config.negation_patterns_first,
            )?;

            diff.files = matcher.filter_files_parallel(&diff.files, self.interner);
        }

        // Step 6: Detect symlinks (parallel)
        if !self.config.exclude_symlinks {
            self.detect_symlinks_parallel(&mut diff.files);
        }

        Ok(diff)
    }

    /// Process via GitHub REST API
    async fn process_via_rest_api(&self) -> Result<DiffResult> {
        let client = GitHubApiClient::from_env()?;
        let (owner, repo, pr_number) = GitHubApiClient::extract_pr_info_from_env()?;

        let mut diff = client.fetch_changed_files(&owner, &repo, pr_number, self.interner).await?;

        // Apply pattern filtering if configured
        if let Some(ref files_patterns) = self.config.files {
            let patterns: Vec<&str> = files_patterns.iter()
                .map(|c| c.as_ref())
                .collect();

            let ignore_patterns: Vec<&str> = self.config.files_ignore.as_ref()
                .map(|ignore| ignore.iter().map(|c| c.as_ref()).collect())
                .unwrap_or_else(Vec::new);

            let matcher = PatternMatcher::new(
                &patterns,
                &ignore_patterns,
                self.config.negation_patterns_first,
            )?;

            diff.files = matcher.filter_files_parallel(&diff.files, self.interner);
        }

        Ok(diff)
    }

    /// Process submodules recursively
    async fn process_submodules(
        &self,
        base_sha: &str,
        head_sha: &str,
    ) -> Result<DiffResult> {
        let repo_path = std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."));

        let max_depth = self.config.dir_names_max_depth.map(|d| d as u8);
        let processor = SubmoduleProcessor::new(&repo_path, max_depth);

        processor.process_submodule_changes(
            base_sha,
            head_sha,
            self.interner,
            0, // Start at depth 0
        )
    }

    /// Detect symlinks in parallel using Rayon
    fn detect_symlinks_parallel(&self, files: &mut [crate::types::ChangedFile]) {
        let file_ops = FileOps::new();

        files.par_iter_mut().for_each(|file| {
            if let Some(path_str) = self.interner.resolve(file.path) {
                let path = Path::new(path_str);
                if let Ok(is_link) = file_ops.is_symlink_sync(path) {
                    file.is_symlink = is_link;
                }
            }
        });
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

    #[tokio::test]
    async fn test_processor_basic_flow() {
        let (_dir, repo_path) = create_test_repo();

        // Create second commit
        fs::write(repo_path.join("file2.txt"), "content2").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "Add file2"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        // Change to repo directory
        std::env::set_current_dir(&repo_path).unwrap();

        let repo = GitRepository::discover(&repo_path).unwrap();
        let interner = StringInterner::new();
        let config = InputConfig::default();

        let processor = FileProcessor::new(&repo, &interner, &config);
        let result = processor.process().await.unwrap();

        assert_eq!(result.files.len(), 1);
        assert_eq!(interner.resolve(result.files[0].path), Some("file2.txt"));
    }
}
