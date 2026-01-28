//! Workflow failure tracking and coordination logic

use crate::error::{Error, Result};
use crate::http::WorkflowApiClient;
use crate::interner::StringInterner;
use crate::types::{
    ChangeType, ChangedFile, FileOrigin, InputConfig, InternedString, WorkflowCheckResult,
    WorkflowConclusion, WorkflowFailure, WorkflowRun, WorkflowStatus,
};
use futures::future::try_join_all;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

/// Workflow tracker for failure detection and active workflow coordination
pub struct WorkflowTracker<'a> {
    api_client: WorkflowApiClient,
    config: &'a InputConfig<'a>,
    interner: &'a StringInterner,
}

impl<'a> WorkflowTracker<'a> {
    /// Create a new workflow tracker
    pub fn new(
        api_client: WorkflowApiClient,
        config: &'a InputConfig<'a>,
        interner: &'a StringInterner,
    ) -> Self {
        Self {
            api_client,
            config,
            interner,
        }
    }

    /// Main entry point: check workflows and return results
    ///
    /// Phase 1: Check and wait for active workflows (cross-branch)
    /// Phase 2: Find recent failures (same branch only)
    pub async fn check_workflows(
        &self,
        branch: &str,
        current_files: &[ChangedFile],
    ) -> Result<WorkflowCheckResult> {
        let mut result = WorkflowCheckResult::default();

        // Extract owner/repo from environment
        let (owner, repo) = self.extract_owner_repo()?;

        // Phase 1: Check active workflows (cross-branch)
        if self.config.wait_for_active_workflows {
            let (blocking_runs, waited, wait_time_ms) = self
                .check_and_wait_for_active_workflows(&owner, &repo, current_files)
                .await?;

            result.blocking_runs = blocking_runs;
            result.waited = waited;
            result.wait_time_ms = wait_time_ms;
        }

        // Phase 2: Find recent failures (same branch)
        if self.config.include_failed_files {
            result.failures = self.find_recent_failures(&owner, &repo, branch).await?;
        }

        Ok(result)
    }

    /// Check for active workflows and wait for them to complete
    ///
    /// Returns: (blocking_runs, waited, wait_time_ms)
    async fn check_and_wait_for_active_workflows(
        &self,
        owner: &str,
        repo: &str,
        current_files: &[ChangedFile],
    ) -> Result<(Vec<WorkflowRun>, bool, u64)> {
        // Query active workflows (any branch)
        let queued = self
            .api_client
            .list_workflow_runs(owner, repo, "", Some("queued"), 100, 1, self.interner)
            .await?;

        let in_progress = self
            .api_client
            .list_workflow_runs(owner, repo, "", Some("in_progress"), 100, 1, self.interner)
            .await?;

        let mut active_workflows = queued;
        active_workflows.extend(in_progress);

        if active_workflows.is_empty() {
            return Ok((vec![], false, 0));
        }

        // Filter to workflows with overlapping files
        let overlapping = self
            .filter_overlapping_workflows(owner, repo, &active_workflows, current_files)
            .await?;

        if overlapping.is_empty() {
            return Ok((vec![], false, 0));
        }

        // Wait for workflows to complete
        let start = std::time::Instant::now();
        let completed = self.wait_for_workflows(owner, repo, &overlapping).await?;
        let wait_time_ms = start.elapsed().as_millis() as u64;

        Ok((completed, true, wait_time_ms))
    }

    /// Filter workflows to only those with overlapping files
    async fn filter_overlapping_workflows(
        &self,
        owner: &str,
        repo: &str,
        workflows: &[WorkflowRun],
        current_files: &[ChangedFile],
    ) -> Result<Vec<WorkflowRun>> {
        // Build HashSet of current file paths for fast lookup
        let current_paths: HashSet<InternedString> = current_files.iter().map(|f| f.path).collect();

        // Fetch commit files for each workflow in parallel
        let api_client = &self.api_client;
        let interner = self.interner;

        // Use Rayon for parallel processing
        let overlapping: Vec<WorkflowRun> = workflows
            .par_iter()
            .filter_map(|workflow| {
                // Create a Tokio runtime for this thread
                let rt = tokio::runtime::Runtime::new().ok()?;

                // Fetch commit files
                let sha = interner.resolve(workflow.head_sha)?;
                let files = rt
                    .block_on(api_client.get_commit_files(owner, repo, sha, interner))
                    .ok()?;

                // Check for overlap
                let has_overlap = files.iter().any(|f| current_paths.contains(f));

                if has_overlap {
                    Some(workflow.clone())
                } else {
                    None
                }
            })
            .collect();

        Ok(overlapping)
    }

    /// Wait for multiple workflows to complete with exponential backoff
    async fn wait_for_workflows(
        &self,
        owner: &str,
        repo: &str,
        workflows: &[WorkflowRun],
    ) -> Result<Vec<WorkflowRun>> {
        // Create futures for each workflow
        let wait_futures: Vec<_> = workflows
            .iter()
            .map(|workflow| {
                self.api_client.wait_for_workflow(
                    owner,
                    repo,
                    workflow.id,
                    self.config.workflow_max_wait_seconds,
                    self.interner,
                )
            })
            .collect();

        // Wait for all workflows in parallel
        let completed = try_join_all(wait_futures).await?;

        Ok(completed)
    }

    /// Find recent workflow failures on the same branch
    async fn find_recent_failures(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
    ) -> Result<Vec<WorkflowFailure>> {
        // Query completed workflows for this branch
        let per_page = self.config.workflow_lookback_commits;
        let completed = self
            .api_client
            .list_workflow_runs(
                owner,
                repo,
                branch,
                Some("completed"),
                per_page,
                1,
                self.interner,
            )
            .await?;

        // Filter to failures only
        let failures: Vec<&WorkflowRun> = completed
            .iter()
            .filter(|run| {
                run.status == WorkflowStatus::Completed
                    && run.conclusion == Some(WorkflowConclusion::Failure)
            })
            .collect();

        if failures.is_empty() {
            return Ok(vec![]);
        }

        // Fetch commit files for each failure in parallel
        let api_client = &self.api_client;
        let interner = self.interner;

        let failure_results: Vec<WorkflowFailure> = failures
            .par_iter()
            .filter_map(|run| {
                // Create a Tokio runtime for this thread
                let rt = tokio::runtime::Runtime::new().ok()?;

                // Fetch commit files
                let sha = interner.resolve(run.head_sha)?;
                let files = rt
                    .block_on(api_client.get_commit_files(owner, repo, sha, interner))
                    .ok()?;

                Some(WorkflowFailure {
                    run: (*run).clone(),
                    files,
                })
            })
            .collect();

        Ok(failure_results)
    }

    /// Merge failed files into current changes
    ///
    /// Files appearing in both current changes and failures are marked as both.
    /// Files only in failures are added with Unknown change type.
    pub fn merge_failed_files(
        &self,
        current_files: &mut Vec<ChangedFile>,
        failures: &[WorkflowFailure],
    ) {
        if failures.is_empty() {
            return;
        }

        // Build map of current files: path -> index
        let mut current_map: HashMap<InternedString, usize> = HashMap::new();
        for (i, file) in current_files.iter().enumerate() {
            current_map.insert(file.path, i);
        }

        // Collect all failed file paths (deduplicated)
        let mut failed_paths: HashSet<InternedString> = HashSet::new();
        for failure in failures {
            failed_paths.extend(failure.files.iter().copied());
        }

        // Mark files that are in both current and failures
        for path in &failed_paths {
            if let Some(&index) = current_map.get(path) {
                current_files[index].origin.in_previous_failure = true;
            }
        }

        // Add files that are only in failures
        for path in failed_paths {
            if !current_map.contains_key(&path) {
                current_files.push(ChangedFile {
                    path,
                    change_type: ChangeType::Unknown,
                    previous_path: None,
                    is_symlink: false,
                    submodule_depth: 0,
                    origin: FileOrigin {
                        in_current_changes: false,
                        in_previous_failure: true,
                    },
                });
            }
        }
    }

    /// Extract owner and repo from environment
    fn extract_owner_repo(&self) -> Result<(String, String)> {
        let repository = std::env::var("GITHUB_REPOSITORY")
            .map_err(|_| Error::Config("GITHUB_REPOSITORY not set".to_string()))?;

        let parts: Vec<&str> = repository.split('/').collect();
        if parts.len() != 2 {
            return Err(Error::Config(format!(
                "Invalid GITHUB_REPOSITORY format: {}",
                repository
            )));
        }

        Ok((parts[0].to_string(), parts[1].to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_failed_files_deduplication() {
        let interner = StringInterner::new();

        let path1 = interner.intern("src/lib.rs");
        let path2 = interner.intern("src/types.rs");
        let path3 = interner.intern("src/error.rs");

        // Current files: path1 and path2
        let mut current_files = vec![
            ChangedFile {
                path: path1,
                change_type: ChangeType::Modified,
                previous_path: None,
                is_symlink: false,
                submodule_depth: 0,
                origin: FileOrigin {
                    in_current_changes: true,
                    in_previous_failure: false,
                },
            },
            ChangedFile {
                path: path2,
                change_type: ChangeType::Added,
                previous_path: None,
                is_symlink: false,
                submodule_depth: 0,
                origin: FileOrigin {
                    in_current_changes: true,
                    in_previous_failure: false,
                },
            },
        ];

        // Failed files: path2 and path3
        let failures = vec![WorkflowFailure {
            run: WorkflowRun {
                id: 123,
                name: interner.intern("CI"),
                status: WorkflowStatus::Completed,
                conclusion: Some(WorkflowConclusion::Failure),
                branch: interner.intern("main"),
                head_sha: interner.intern("abc123"),
                created_at: 0,
            },
            files: vec![path2, path3],
        }];

        let config = InputConfig::default();
        let api_client = WorkflowApiClient::new("https://api.github.com".to_string(), None);
        let tracker = WorkflowTracker::new(api_client, &config, &interner);

        tracker.merge_failed_files(&mut current_files, &failures);

        // Verify results
        assert_eq!(current_files.len(), 3);

        // path1: current only
        assert!(current_files[0].origin.in_current_changes);
        assert!(!current_files[0].origin.in_previous_failure);

        // path2: both
        assert!(current_files[1].origin.in_current_changes);
        assert!(current_files[1].origin.in_previous_failure);

        // path3: failure only
        assert!(!current_files[2].origin.in_current_changes);
        assert!(current_files[2].origin.in_previous_failure);
        assert_eq!(current_files[2].change_type, ChangeType::Unknown);
    }

    #[test]
    fn test_extract_owner_repo() {
        let interner = StringInterner::new();
        let config = InputConfig::default();
        let api_client = WorkflowApiClient::new("https://api.github.com".to_string(), None);
        let tracker = WorkflowTracker::new(api_client, &config, &interner);

        // Save original value
        let original = std::env::var("GITHUB_REPOSITORY").ok();

        // Test valid format
        std::env::set_var("GITHUB_REPOSITORY", "owner/repo");
        let result = tracker.extract_owner_repo();
        assert!(result.is_ok());
        let (owner, repo) = result.unwrap();
        assert_eq!(owner, "owner");
        assert_eq!(repo, "repo");

        // Test invalid format
        std::env::set_var("GITHUB_REPOSITORY", "invalid");
        let result = tracker.extract_owner_repo();
        assert!(result.is_err());

        // Restore original value
        if let Some(val) = original {
            std::env::set_var("GITHUB_REPOSITORY", val);
        } else {
            std::env::remove_var("GITHUB_REPOSITORY");
        }
    }
}
