//! Workflow failure tracking and coordination logic

use crate::coordination::extract_owner_repo;
use crate::error::Result;
use crate::http::WorkflowApiClient;
use crate::interner::StringInterner;
use crate::patterns::loader::PatternGroup;
use crate::types::{
    ChangeType, ChangedFile, FailureTrackingLevel, FileOrigin, InputConfig, InternedString,
    WorkflowCheckResult, WorkflowConclusion, WorkflowFailure, WorkflowJob, WorkflowRun,
    WorkflowStatus, WorkflowSuccess,
};
use futures::future::{join_all, try_join_all};
use std::collections::{HashMap, HashSet};

/// Extract the matrix key from a job name.
///
/// Looks for text between `[` and `]` brackets.
/// Returns a zero-copy `&str` slice into the input.
///
/// Examples:
/// - `"Deploy [prod]"` -> `Some("prod")`
/// - `"Deploy [staging]"` -> `Some("staging")`
/// - `"Lint"` -> `None`
#[inline]
pub fn extract_job_key(job_name: &str) -> Option<&str> {
    let start = job_name.find('[')? + 1;
    let end = job_name[start..].find(']')? + start;
    Some(&job_name[start..end])
}

/// Workflow tracker for failure detection, success tracking, and active workflow coordination
pub struct WorkflowTracker<'a> {
    api_client: WorkflowApiClient,
    config: &'a InputConfig<'a>,
    interner: &'a StringInterner,
    yaml_groups: &'a [PatternGroup],
}

impl<'a> WorkflowTracker<'a> {
    /// Create a new workflow tracker
    pub fn new(
        api_client: WorkflowApiClient,
        config: &'a InputConfig<'a>,
        interner: &'a StringInterner,
        yaml_groups: &'a [PatternGroup],
    ) -> Self {
        Self {
            api_client,
            config,
            interner,
            yaml_groups,
        }
    }

    /// Main entry point: check workflows and return results
    ///
    /// Phase 1: Check and wait for active workflows (cross-branch)
    /// Phase 2: Find recent failures (same branch, optionally with job-level detail)
    /// Phase 3: Find recent successes (same branch, for skip decisions)
    pub async fn check_workflows(
        &self,
        branch: &str,
        current_files: &[ChangedFile],
    ) -> Result<WorkflowCheckResult> {
        let mut result = WorkflowCheckResult::default();

        // Extract owner/repo from environment
        let (owner, repo) = extract_owner_repo()?;

        // Phase 1: Check active workflows (cross-branch)
        if self.config.wait_for_active_workflows {
            let (blocking_runs, waited, wait_time_ms, blocked_groups) = self
                .check_and_wait_for_active_workflows(&owner, &repo, current_files)
                .await?;

            result.blocking_runs = blocking_runs;
            result.waited = waited;
            result.wait_time_ms = wait_time_ms;
            result.blocked_groups = blocked_groups;
        }

        // Phase 2: Find recent failures (same branch)
        if self.config.include_failed_files {
            result.failures = self.find_recent_failures(&owner, &repo, branch).await?;
        }

        // Phase 3: Find recent successes (same branch) — for skip decisions
        if self.config.skip_successful_files {
            result.successes = self.find_recent_successes(&owner, &repo, branch).await?;
        }

        Ok(result)
    }

    /// Check for active workflows and wait for them to complete
    ///
    /// Returns: (blocking_runs, waited, wait_time_ms, blocked_groups)
    async fn check_and_wait_for_active_workflows(
        &self,
        owner: &str,
        repo: &str,
        current_files: &[ChangedFile],
    ) -> Result<(
        Vec<WorkflowRun>,
        bool,
        u64,
        HashMap<InternedString, Vec<u64>>,
    )> {
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

        // Exclude the current workflow run and only wait for earlier runs
        // to prevent deadlock (both sides waiting for each other).
        // Priority: lower run ID = earlier arrival = runs first.
        if let Ok(current_run_id) = std::env::var("GITHUB_RUN_ID") {
            if let Ok(id) = current_run_id.parse::<u64>() {
                active_workflows.retain(|w| w.id < id);
            }
        }

        // Filter by workflow_name_filter to only wait for same-named workflows
        if self.config.workflow_name_filter.is_some() {
            active_workflows.retain(|w| self.matches_workflow_name_filter(w));
        }

        if active_workflows.is_empty() {
            return Ok((vec![], false, 0, HashMap::new()));
        }

        // Filter to workflows with overlapping files/groups
        let (overlapping, blocked_groups) = self
            .filter_overlapping_workflows(owner, repo, &active_workflows, current_files)
            .await?;

        if overlapping.is_empty() {
            return Ok((vec![], false, 0, HashMap::new()));
        }

        // Wait for workflows to complete
        let start = std::time::Instant::now();
        let completed = self.wait_for_workflows(owner, repo, &overlapping).await?;
        let wait_time_ms = start.elapsed().as_millis() as u64;

        Ok((completed, true, wait_time_ms, blocked_groups))
    }

    /// Filter workflows to only those with overlapping files or groups.
    ///
    /// Returns `(overlapping_runs, blocked_groups)` where `blocked_groups` maps
    /// group keys to the IDs of workflow runs that overlap with that group.
    async fn filter_overlapping_workflows(
        &self,
        owner: &str,
        repo: &str,
        workflows: &[WorkflowRun],
        current_files: &[ChangedFile],
    ) -> Result<(Vec<WorkflowRun>, HashMap<InternedString, Vec<u64>>)> {
        // Build HashSet of current file paths for fast lookup
        let current_paths: HashSet<InternedString> = current_files.iter().map(|f| f.path).collect();

        // Pre-compute current groups: group_key → set of matching file paths
        let current_groups: HashMap<InternedString, HashSet<InternedString>> =
            if !self.yaml_groups.is_empty() {
                let mut groups = HashMap::new();
                for group in self.yaml_groups {
                    let key = self.interner.intern(&group.name);
                    let mut matched = HashSet::new();
                    for &path in &current_paths {
                        if let Some(p) = self.interner.resolve(path) {
                            if group.matcher.matches_sync(p) {
                                matched.insert(path);
                            }
                        }
                    }
                    if !matched.is_empty() {
                        groups.insert(key, matched);
                    }
                }
                groups
            } else {
                HashMap::new()
            };

        // Fetch commit files for each workflow concurrently
        let futures: Vec<_> = workflows
            .iter()
            .map(|workflow| async {
                let sha = match self.interner.resolve(workflow.head_sha) {
                    Some(s) => s,
                    None => return None,
                };
                let files = self
                    .api_client
                    .get_commit_files(owner, repo, sha, self.interner)
                    .await
                    .ok()?;

                // Check file-level overlap
                let has_file_overlap = files.iter().any(|f| current_paths.contains(f));

                // Check group-level overlap
                let mut overlapping_groups = Vec::new();
                if !current_groups.is_empty() {
                    for group in self.yaml_groups {
                        let key = self.interner.intern(&group.name);
                        if !current_groups.contains_key(&key) {
                            continue; // Current run doesn't touch this group
                        }
                        // Check if the other workflow's files match this group
                        let other_touches_group = files.iter().any(|&f| {
                            self.interner
                                .resolve(f)
                                .map(|p| group.matcher.matches_sync(p))
                                .unwrap_or(false)
                        });
                        if other_touches_group {
                            overlapping_groups.push(key);
                        }
                    }
                }

                if has_file_overlap || !overlapping_groups.is_empty() {
                    Some((*workflow, overlapping_groups))
                } else {
                    None
                }
            })
            .collect();

        let results = join_all(futures).await;

        let mut overlapping = Vec::new();
        let mut blocked_groups: HashMap<InternedString, Vec<u64>> = HashMap::new();

        for item in results.into_iter().flatten() {
            let (run, groups) = item;
            overlapping.push(run);
            for group_key in groups {
                blocked_groups.entry(group_key).or_default().push(run.id);
            }
        }

        Ok((overlapping, blocked_groups))
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

    /// Find recent workflow failures on the same branch.
    ///
    /// When `FailureTrackingLevel::Job`, fetches jobs concurrently with commit files,
    /// then partitions: only files matching failed job patterns go to `WorkflowFailure.files`.
    async fn find_recent_failures(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
    ) -> Result<Vec<WorkflowFailure>> {
        // Query completed workflows for this branch.
        // Use a generous per_page because the API returns runs across ALL workflows
        // in the repo, but we only care about runs matching our workflow_name_filter.
        // After filtering by name+conclusion, we limit to workflow_lookback_commits.
        let lookback = self.config.workflow_lookback_commits;
        let per_page = self.api_per_page(lookback);
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

        // Filter to failures only, optionally by workflow name, limited to lookback count
        let failures: Vec<&WorkflowRun> = completed
            .iter()
            .filter(|run| {
                run.status == WorkflowStatus::Completed
                    && run.conclusion == Some(WorkflowConclusion::Failure)
            })
            .filter(|run| self.matches_workflow_name_filter(run))
            .take(lookback as usize)
            .collect();

        if failures.is_empty() {
            return Ok(vec![]);
        }

        let is_job_level = self.config.failure_tracking_level == FailureTrackingLevel::Job;

        // Fetch commit files (and optionally jobs) for each failure concurrently
        let futures: Vec<_> = failures
            .iter()
            .map(|run| async move {
                let sha = match self.interner.resolve(run.head_sha) {
                    Some(s) => s,
                    None => return None,
                };

                if is_job_level {
                    // Job-level: fetch files and jobs concurrently
                    let files_fut =
                        self.api_client
                            .get_commit_files(owner, repo, sha, self.interner);
                    let jobs_fut =
                        self.api_client
                            .list_workflow_jobs(owner, repo, run.id, self.interner);

                    let (files_result, jobs_result) =
                        futures::future::join(files_fut, jobs_fut).await;

                    let commit_files = files_result.ok()?;
                    let all_jobs = jobs_result.ok().unwrap_or_default();

                    // Partition: only files matching failed job patterns
                    let failed_jobs: Vec<WorkflowJob> = all_jobs
                        .into_iter()
                        .filter(|j| j.conclusion == Some(WorkflowConclusion::Failure))
                        .collect();

                    let partitioned_files =
                        self.partition_files_for_failed_jobs(&commit_files, &failed_jobs);

                    Some(WorkflowFailure {
                        run: **run,
                        files: partitioned_files,
                        failed_jobs,
                    })
                } else {
                    // Run-level: all commit files attributed to the failure
                    let files = self
                        .api_client
                        .get_commit_files(owner, repo, sha, self.interner)
                        .await
                        .ok()?;

                    Some(WorkflowFailure {
                        run: **run,
                        files,
                        failed_jobs: Vec::new(),
                    })
                }
            })
            .collect();

        let results = join_all(futures).await;
        let failure_results: Vec<WorkflowFailure> = results.into_iter().flatten().collect();

        Ok(failure_results)
    }

    /// Find recent successful workflows on the same branch (Phase 3).
    ///
    /// When `FailureTrackingLevel::Job`, fetches jobs and partitions files so only
    /// files matching succeeded job patterns go to `WorkflowSuccess.files`.
    async fn find_recent_successes(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
    ) -> Result<Vec<WorkflowSuccess>> {
        // Use generous per_page (same rationale as find_recent_failures)
        let lookback = self.config.workflow_success_lookback;
        let per_page = self.api_per_page(lookback);
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

        // Filter to successes only, optionally by workflow name, limited to lookback count
        let successes: Vec<&WorkflowRun> = completed
            .iter()
            .filter(|run| {
                run.status == WorkflowStatus::Completed
                    && run.conclusion == Some(WorkflowConclusion::Success)
            })
            .filter(|run| self.matches_workflow_name_filter(run))
            .take(lookback as usize)
            .collect();

        if successes.is_empty() {
            return Ok(vec![]);
        }

        let is_job_level = self.config.failure_tracking_level == FailureTrackingLevel::Job;

        let futures: Vec<_> = successes
            .iter()
            .map(|run| async move {
                let sha = match self.interner.resolve(run.head_sha) {
                    Some(s) => s,
                    None => return None,
                };

                if is_job_level {
                    // Job-level: fetch files and jobs concurrently
                    let files_fut =
                        self.api_client
                            .get_commit_files(owner, repo, sha, self.interner);
                    let jobs_fut =
                        self.api_client
                            .list_workflow_jobs(owner, repo, run.id, self.interner);

                    let (files_result, jobs_result) =
                        futures::future::join(files_fut, jobs_fut).await;

                    let commit_files = files_result.ok()?;
                    let all_jobs = jobs_result.ok().unwrap_or_default();

                    // Partition: only files matching succeeded job patterns
                    let succeeded_jobs: Vec<&WorkflowJob> = all_jobs
                        .iter()
                        .filter(|j| j.conclusion == Some(WorkflowConclusion::Success))
                        .collect();

                    let partitioned_files =
                        self.partition_files_for_succeeded_jobs(&commit_files, &succeeded_jobs);

                    Some(WorkflowSuccess {
                        run: **run,
                        jobs: all_jobs,
                        files: partitioned_files,
                    })
                } else {
                    // Run-level: all commit files attributed to the success
                    let files = self
                        .api_client
                        .get_commit_files(owner, repo, sha, self.interner)
                        .await
                        .ok()?;

                    Some(WorkflowSuccess {
                        run: **run,
                        jobs: Vec::new(),
                        files,
                    })
                }
            })
            .collect();

        let results = join_all(futures).await;
        let success_results: Vec<WorkflowSuccess> = results.into_iter().flatten().collect();

        Ok(success_results)
    }

    /// Unified partition: select commit files matching the given jobs' group patterns.
    ///
    /// - `include_unmatched=true`: files not claimed by ANY group are included (conservative — for failures)
    /// - `include_unmatched=false`: only files matching a job's group are included (strict — for successes)
    ///
    /// When no YAML groups are configured or no jobs can be mapped to a group,
    /// falls back to returning all commit files.
    fn partition_files_by_jobs(
        &self,
        commit_files: &[InternedString],
        jobs: &[WorkflowJob],
        include_unmatched: bool,
    ) -> Vec<InternedString> {
        if self.yaml_groups.is_empty() || jobs.is_empty() {
            return commit_files.to_vec();
        }

        let mut matched_files: HashSet<InternedString> = HashSet::new();
        let mut any_job_matched_group = false;

        for job in jobs {
            if let Some(job_name) = self.interner.resolve(job.name) {
                if let Some(key) = extract_job_key(job_name) {
                    if let Some(group) = self.yaml_groups.iter().find(|g| g.name == key) {
                        any_job_matched_group = true;
                        for &file in commit_files {
                            if let Some(path) = self.interner.resolve(file) {
                                if group.matcher.matches_sync(path) {
                                    matched_files.insert(file);
                                }
                            }
                        }
                    }
                }
            }
        }

        if !any_job_matched_group {
            return commit_files.to_vec();
        }

        if include_unmatched {
            // Conservative: files not claimed by ANY group → included
            let mut all_matched: HashSet<InternedString> = HashSet::new();
            for group in self.yaml_groups {
                for &file in commit_files {
                    if let Some(path) = self.interner.resolve(file) {
                        if group.matcher.matches_sync(path) {
                            all_matched.insert(file);
                        }
                    }
                }
            }
            for &file in commit_files {
                if !all_matched.contains(&file) {
                    matched_files.insert(file);
                }
            }
        }

        matched_files.into_iter().collect()
    }

    /// Partition commit files to those matching failed job patterns.
    ///
    /// Unmatched files are conservatively attributed to all failed jobs.
    pub fn partition_files_for_failed_jobs(
        &self,
        commit_files: &[InternedString],
        failed_jobs: &[WorkflowJob],
    ) -> Vec<InternedString> {
        self.partition_files_by_jobs(commit_files, failed_jobs, true)
    }

    /// Partition commit files to those matching succeeded job patterns.
    ///
    /// Only files attributed to a succeeded job are considered verified (strict).
    pub fn partition_files_for_succeeded_jobs(
        &self,
        commit_files: &[InternedString],
        succeeded_jobs: &[&WorkflowJob],
    ) -> Vec<InternedString> {
        // Copy job references into owned vec for unified API
        let jobs: Vec<WorkflowJob> = succeeded_jobs.iter().map(|&&j| j).collect();
        self.partition_files_by_jobs(commit_files, &jobs, false)
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
                        in_previous_success: false,
                    },
                });
            }
        }
    }

    /// Compute the API per_page value from a lookback count.
    ///
    /// The GitHub Actions runs endpoint returns runs across ALL workflows in
    /// the repo. When `workflow_name_filter` is set, most returned runs will
    /// be filtered out client-side. We use a generous multiplier so the page
    /// is large enough to contain `lookback` matching runs even when the repo
    /// has many other workflows. Capped at 100 (GitHub API maximum).
    fn api_per_page(&self, lookback: u32) -> u32 {
        if self.config.workflow_name_filter.is_some() {
            // With a name filter, most results will be discarded — fetch more
            (lookback * 10).min(100).max(lookback)
        } else {
            lookback
        }
    }

    /// Check if a workflow run matches the optional name filter
    fn matches_workflow_name_filter(&self, run: &WorkflowRun) -> bool {
        match &self.config.workflow_name_filter {
            Some(filter) => {
                if let Some(name) = self.interner.resolve(run.name) {
                    let filter_str = filter.as_ref();
                    // Simple glob matching: support * and exact match
                    if filter_str.contains('*') {
                        let parts: Vec<&str> = filter_str.split('*').collect();
                        if parts.len() == 2 {
                            let (prefix, suffix) = (parts[0], parts[1]);
                            name.starts_with(prefix) && name.ends_with(suffix)
                        } else {
                            // Fallback: exact match for complex patterns
                            name == filter_str
                        }
                    } else {
                        name == filter_str
                    }
                } else {
                    false
                }
            }
            None => true, // No filter = match all
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_job_key_with_brackets() {
        assert_eq!(extract_job_key("Deploy [prod]"), Some("prod"));
        assert_eq!(extract_job_key("Deploy [staging]"), Some("staging"));
        assert_eq!(extract_job_key("Deploy [dev]"), Some("dev"));
        assert_eq!(
            extract_job_key("Build [linux, x86_64]"),
            Some("linux, x86_64")
        );
    }

    #[test]
    fn test_extract_job_key_no_brackets() {
        assert_eq!(extract_job_key("Lint"), None);
        assert_eq!(extract_job_key("Build"), None);
        assert_eq!(extract_job_key(""), None);
    }

    #[test]
    fn test_extract_job_key_edge_cases() {
        assert_eq!(extract_job_key("[]"), Some(""));
        assert_eq!(extract_job_key("[prod]"), Some("prod"));
        assert_eq!(extract_job_key("Deploy [prod] extra"), Some("prod"));
    }

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
                    in_previous_success: false,
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
                    in_previous_success: false,
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
            failed_jobs: Vec::new(),
        }];

        let config = InputConfig::default();
        let api_client = WorkflowApiClient::new("https://api.github.com".to_string(), None);
        let tracker = WorkflowTracker::new(api_client, &config, &interner, &[]);

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
        // Save original value
        let original = std::env::var("GITHUB_REPOSITORY").ok();

        // Test valid format
        std::env::set_var("GITHUB_REPOSITORY", "owner/repo");
        let result = extract_owner_repo();
        assert!(result.is_ok());
        let (owner, repo) = result.unwrap();
        assert_eq!(owner, "owner");
        assert_eq!(repo, "repo");

        // Test invalid format
        std::env::set_var("GITHUB_REPOSITORY", "invalid");
        let result = extract_owner_repo();
        assert!(result.is_err());

        // Restore original value
        if let Some(val) = original {
            std::env::set_var("GITHUB_REPOSITORY", val);
        } else {
            std::env::remove_var("GITHUB_REPOSITORY");
        }
    }

    #[test]
    fn test_workflow_name_filter() {
        let interner = StringInterner::new();
        let api_client = WorkflowApiClient::new("https://api.github.com".to_string(), None);

        // Test with no filter (match all)
        let config = InputConfig::default();
        let tracker = WorkflowTracker::new(api_client, &config, &interner, &[]);

        let run = WorkflowRun {
            id: 1,
            name: interner.intern("CI"),
            status: WorkflowStatus::Completed,
            conclusion: Some(WorkflowConclusion::Success),
            branch: interner.intern("main"),
            head_sha: interner.intern("abc123"),
            created_at: 0,
        };
        assert!(tracker.matches_workflow_name_filter(&run));

        // Test with exact filter
        let api_client2 = WorkflowApiClient::new("https://api.github.com".to_string(), None);
        let config2 = InputConfig {
            workflow_name_filter: Some(std::borrow::Cow::Borrowed("CI")),
            ..Default::default()
        };
        let tracker2 = WorkflowTracker::new(api_client2, &config2, &interner, &[]);
        assert!(tracker2.matches_workflow_name_filter(&run));

        let run2 = WorkflowRun {
            id: 2,
            name: interner.intern("Deploy"),
            status: WorkflowStatus::Completed,
            conclusion: Some(WorkflowConclusion::Success),
            branch: interner.intern("main"),
            head_sha: interner.intern("def456"),
            created_at: 0,
        };
        assert!(!tracker2.matches_workflow_name_filter(&run2));

        // Test with glob filter
        let api_client3 = WorkflowApiClient::new("https://api.github.com".to_string(), None);
        let config3 = InputConfig {
            workflow_name_filter: Some(std::borrow::Cow::Borrowed("CI*")),
            ..Default::default()
        };
        let tracker3 = WorkflowTracker::new(api_client3, &config3, &interner, &[]);

        let run3 = WorkflowRun {
            id: 3,
            name: interner.intern("CI Build"),
            status: WorkflowStatus::Completed,
            conclusion: Some(WorkflowConclusion::Success),
            branch: interner.intern("main"),
            head_sha: interner.intern("ghi789"),
            created_at: 0,
        };
        assert!(tracker3.matches_workflow_name_filter(&run3));
        assert!(!tracker3.matches_workflow_name_filter(&run2)); // "Deploy" doesn't match "CI*"
    }

    #[test]
    fn test_partition_files_for_failed_jobs_no_groups() {
        let interner = StringInterner::new();
        let config = InputConfig::default();
        let api_client = WorkflowApiClient::new("https://api.github.com".to_string(), None);
        let tracker = WorkflowTracker::new(api_client, &config, &interner, &[]);

        let file_a = interner.intern("stacks/dev/config.yaml");
        let file_b = interner.intern("stacks/prod/config.yaml");
        let commit_files = vec![file_a, file_b];

        let failed_jobs = vec![WorkflowJob {
            id: 1,
            name: interner.intern("Deploy [prod]"),
            status: WorkflowStatus::Completed,
            conclusion: Some(WorkflowConclusion::Failure),
            run_id: 100,
            started_at: 0,
            completed_at: 0,
        }];

        // No groups → fallback to all files
        let result = tracker.partition_files_for_failed_jobs(&commit_files, &failed_jobs);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_partition_files_for_failed_jobs_with_groups() {
        use crate::patterns::matcher::PatternMatcher;

        let interner = StringInterner::new();
        let config = InputConfig {
            failure_tracking_level: FailureTrackingLevel::Job,
            ..Default::default()
        };

        let dev_matcher = PatternMatcher::new(&["stacks/dev/**"], &[], true).unwrap();
        let prod_matcher = PatternMatcher::new(&["stacks/prod/**"], &[], true).unwrap();
        let groups = vec![
            PatternGroup {
                name: "dev".to_string(),
                matcher: dev_matcher,
            },
            PatternGroup {
                name: "prod".to_string(),
                matcher: prod_matcher,
            },
        ];

        let api_client = WorkflowApiClient::new("https://api.github.com".to_string(), None);
        let tracker = WorkflowTracker::new(api_client, &config, &interner, &groups);

        let file_dev = interner.intern("stacks/dev/config.yaml");
        let file_prod = interner.intern("stacks/prod/config.yaml");
        let commit_files = vec![file_dev, file_prod];

        // Only prod failed
        let failed_jobs = vec![WorkflowJob {
            id: 1,
            name: interner.intern("Deploy [prod]"),
            status: WorkflowStatus::Completed,
            conclusion: Some(WorkflowConclusion::Failure),
            run_id: 100,
            started_at: 0,
            completed_at: 0,
        }];

        let result = tracker.partition_files_for_failed_jobs(&commit_files, &failed_jobs);

        // Only prod file should be in the result (not dev)
        assert_eq!(result.len(), 1);
        assert!(result.contains(&file_prod));
        assert!(!result.contains(&file_dev));
    }

    #[test]
    fn test_partition_files_unmatched_files_go_to_failed() {
        use crate::patterns::matcher::PatternMatcher;

        let interner = StringInterner::new();
        let config = InputConfig {
            failure_tracking_level: FailureTrackingLevel::Job,
            ..Default::default()
        };

        let dev_matcher = PatternMatcher::new(&["stacks/dev/**"], &[], true).unwrap();
        let prod_matcher = PatternMatcher::new(&["stacks/prod/**"], &[], true).unwrap();
        let groups = vec![
            PatternGroup {
                name: "dev".to_string(),
                matcher: dev_matcher,
            },
            PatternGroup {
                name: "prod".to_string(),
                matcher: prod_matcher,
            },
        ];

        let api_client = WorkflowApiClient::new("https://api.github.com".to_string(), None);
        let tracker = WorkflowTracker::new(api_client, &config, &interner, &groups);

        let file_dev = interner.intern("stacks/dev/config.yaml");
        let file_prod = interner.intern("stacks/prod/config.yaml");
        let file_root = interner.intern("README.md"); // doesn't match any group
        let commit_files = vec![file_dev, file_prod, file_root];

        // Only prod failed
        let failed_jobs = vec![WorkflowJob {
            id: 1,
            name: interner.intern("Deploy [prod]"),
            status: WorkflowStatus::Completed,
            conclusion: Some(WorkflowConclusion::Failure),
            run_id: 100,
            started_at: 0,
            completed_at: 0,
        }];

        let result = tracker.partition_files_for_failed_jobs(&commit_files, &failed_jobs);

        // prod file + README (unmatched → conservative fallback)
        assert_eq!(result.len(), 2);
        assert!(result.contains(&file_prod));
        assert!(result.contains(&file_root));
        assert!(!result.contains(&file_dev));
    }

    #[test]
    fn test_partition_files_for_succeeded_jobs() {
        use crate::patterns::matcher::PatternMatcher;

        let interner = StringInterner::new();
        let config = InputConfig {
            failure_tracking_level: FailureTrackingLevel::Job,
            ..Default::default()
        };

        let dev_matcher = PatternMatcher::new(&["stacks/dev/**"], &[], true).unwrap();
        let prod_matcher = PatternMatcher::new(&["stacks/prod/**"], &[], true).unwrap();
        let groups = vec![
            PatternGroup {
                name: "dev".to_string(),
                matcher: dev_matcher,
            },
            PatternGroup {
                name: "prod".to_string(),
                matcher: prod_matcher,
            },
        ];

        let api_client = WorkflowApiClient::new("https://api.github.com".to_string(), None);
        let tracker = WorkflowTracker::new(api_client, &config, &interner, &groups);

        let file_dev = interner.intern("stacks/dev/config.yaml");
        let file_prod = interner.intern("stacks/prod/config.yaml");
        let commit_files = vec![file_dev, file_prod];

        // Only dev succeeded
        let dev_job = WorkflowJob {
            id: 1,
            name: interner.intern("Deploy [dev]"),
            status: WorkflowStatus::Completed,
            conclusion: Some(WorkflowConclusion::Success),
            run_id: 100,
            started_at: 0,
            completed_at: 0,
        };
        let succeeded_jobs: Vec<&WorkflowJob> = vec![&dev_job];

        let result = tracker.partition_files_for_succeeded_jobs(&commit_files, &succeeded_jobs);

        // Only dev file should be verified
        assert_eq!(result.len(), 1);
        assert!(result.contains(&file_dev));
        assert!(!result.contains(&file_prod));
    }

    // ── Additional edge case tests ──────────────────────────────────

    #[test]
    fn test_extract_job_key_special_characters() {
        assert_eq!(
            extract_job_key("Deploy [prod-us-east-1]"),
            Some("prod-us-east-1")
        );
        assert_eq!(
            extract_job_key("Build [linux, x86_64, release]"),
            Some("linux, x86_64, release")
        );
        assert_eq!(extract_job_key("Test [v2.0.0]"), Some("v2.0.0"));
        assert_eq!(extract_job_key("Job [with spaces]"), Some("with spaces"));
    }

    #[test]
    fn test_extract_job_key_nested_brackets() {
        // First [ to first ] — gets the inner text up to first closing bracket
        assert_eq!(extract_job_key("Deploy [[inner]]"), Some("[inner"));
        assert_eq!(extract_job_key("A [B] [C]"), Some("B"));
    }

    #[test]
    fn test_extract_job_key_only_opening_bracket() {
        assert_eq!(extract_job_key("Deploy [unclosed"), None);
        assert_eq!(extract_job_key("["), None);
    }

    #[test]
    fn test_partition_files_multiple_failed_jobs() {
        use crate::patterns::matcher::PatternMatcher;

        let interner = StringInterner::new();
        let config = InputConfig {
            failure_tracking_level: FailureTrackingLevel::Job,
            ..Default::default()
        };

        let dev_matcher = PatternMatcher::new(&["stacks/dev/**"], &[], true).unwrap();
        let staging_matcher = PatternMatcher::new(&["stacks/staging/**"], &[], true).unwrap();
        let prod_matcher = PatternMatcher::new(&["stacks/prod/**"], &[], true).unwrap();
        let groups = vec![
            PatternGroup {
                name: "dev".to_string(),
                matcher: dev_matcher,
            },
            PatternGroup {
                name: "staging".to_string(),
                matcher: staging_matcher,
            },
            PatternGroup {
                name: "prod".to_string(),
                matcher: prod_matcher,
            },
        ];

        let api_client = WorkflowApiClient::new("https://api.github.com".to_string(), None);
        let tracker = WorkflowTracker::new(api_client, &config, &interner, &groups);

        let file_dev = interner.intern("stacks/dev/config.yaml");
        let file_staging = interner.intern("stacks/staging/config.yaml");
        let file_prod = interner.intern("stacks/prod/config.yaml");
        let commit_files = vec![file_dev, file_staging, file_prod];

        // Both staging and prod failed, dev succeeded
        let failed_jobs = vec![
            WorkflowJob {
                id: 1,
                name: interner.intern("Deploy [staging]"),
                status: WorkflowStatus::Completed,
                conclusion: Some(WorkflowConclusion::Failure),
                run_id: 100,
                started_at: 0,
                completed_at: 0,
            },
            WorkflowJob {
                id: 2,
                name: interner.intern("Deploy [prod]"),
                status: WorkflowStatus::Completed,
                conclusion: Some(WorkflowConclusion::Failure),
                run_id: 100,
                started_at: 0,
                completed_at: 0,
            },
        ];

        let result = tracker.partition_files_for_failed_jobs(&commit_files, &failed_jobs);

        // staging + prod should be in results, but NOT dev
        assert_eq!(result.len(), 2);
        assert!(result.contains(&file_staging));
        assert!(result.contains(&file_prod));
        assert!(!result.contains(&file_dev));
    }

    #[test]
    fn test_partition_files_job_no_matching_group() {
        use crate::patterns::matcher::PatternMatcher;

        let interner = StringInterner::new();
        let config = InputConfig {
            failure_tracking_level: FailureTrackingLevel::Job,
            ..Default::default()
        };

        let dev_matcher = PatternMatcher::new(&["stacks/dev/**"], &[], true).unwrap();
        let groups = vec![PatternGroup {
            name: "dev".to_string(),
            matcher: dev_matcher,
        }];

        let api_client = WorkflowApiClient::new("https://api.github.com".to_string(), None);
        let tracker = WorkflowTracker::new(api_client, &config, &interner, &groups);

        let file_dev = interner.intern("stacks/dev/config.yaml");
        let file_prod = interner.intern("stacks/prod/config.yaml");
        let commit_files = vec![file_dev, file_prod];

        // Job key "prod" doesn't match any YAML group
        let failed_jobs = vec![WorkflowJob {
            id: 1,
            name: interner.intern("Deploy [prod]"),
            status: WorkflowStatus::Completed,
            conclusion: Some(WorkflowConclusion::Failure),
            run_id: 100,
            started_at: 0,
            completed_at: 0,
        }];

        let result = tracker.partition_files_for_failed_jobs(&commit_files, &failed_jobs);

        // No group matched any failed job → fallback to ALL files
        assert_eq!(result.len(), 2);
        assert!(result.contains(&file_dev));
        assert!(result.contains(&file_prod));
    }

    #[test]
    fn test_partition_files_empty_commit_files() {
        use crate::patterns::matcher::PatternMatcher;

        let interner = StringInterner::new();
        let config = InputConfig {
            failure_tracking_level: FailureTrackingLevel::Job,
            ..Default::default()
        };

        let dev_matcher = PatternMatcher::new(&["stacks/dev/**"], &[], true).unwrap();
        let groups = vec![PatternGroup {
            name: "dev".to_string(),
            matcher: dev_matcher,
        }];

        let api_client = WorkflowApiClient::new("https://api.github.com".to_string(), None);
        let tracker = WorkflowTracker::new(api_client, &config, &interner, &groups);

        let commit_files: Vec<InternedString> = vec![];

        let failed_jobs = vec![WorkflowJob {
            id: 1,
            name: interner.intern("Deploy [dev]"),
            status: WorkflowStatus::Completed,
            conclusion: Some(WorkflowConclusion::Failure),
            run_id: 100,
            started_at: 0,
            completed_at: 0,
        }];

        let result = tracker.partition_files_for_failed_jobs(&commit_files, &failed_jobs);
        assert!(result.is_empty());
    }

    #[test]
    fn test_partition_files_empty_failed_jobs() {
        use crate::patterns::matcher::PatternMatcher;

        let interner = StringInterner::new();
        let config = InputConfig {
            failure_tracking_level: FailureTrackingLevel::Job,
            ..Default::default()
        };

        let dev_matcher = PatternMatcher::new(&["stacks/dev/**"], &[], true).unwrap();
        let groups = vec![PatternGroup {
            name: "dev".to_string(),
            matcher: dev_matcher,
        }];

        let api_client = WorkflowApiClient::new("https://api.github.com".to_string(), None);
        let tracker = WorkflowTracker::new(api_client, &config, &interner, &groups);

        let file_dev = interner.intern("stacks/dev/config.yaml");
        let commit_files = vec![file_dev];

        let failed_jobs: Vec<WorkflowJob> = vec![];

        // Empty failed jobs → fallback to all files
        let result = tracker.partition_files_for_failed_jobs(&commit_files, &failed_jobs);
        assert_eq!(result.len(), 1);
        assert!(result.contains(&file_dev));
    }

    #[test]
    fn test_partition_files_job_without_brackets() {
        use crate::patterns::matcher::PatternMatcher;

        let interner = StringInterner::new();
        let config = InputConfig {
            failure_tracking_level: FailureTrackingLevel::Job,
            ..Default::default()
        };

        let dev_matcher = PatternMatcher::new(&["stacks/dev/**"], &[], true).unwrap();
        let groups = vec![PatternGroup {
            name: "dev".to_string(),
            matcher: dev_matcher,
        }];

        let api_client = WorkflowApiClient::new("https://api.github.com".to_string(), None);
        let tracker = WorkflowTracker::new(api_client, &config, &interner, &groups);

        let file_dev = interner.intern("stacks/dev/config.yaml");
        let commit_files = vec![file_dev];

        // Job name has no brackets → can't extract key → fallback
        let failed_jobs = vec![WorkflowJob {
            id: 1,
            name: interner.intern("Lint"),
            status: WorkflowStatus::Completed,
            conclusion: Some(WorkflowConclusion::Failure),
            run_id: 100,
            started_at: 0,
            completed_at: 0,
        }];

        let result = tracker.partition_files_for_failed_jobs(&commit_files, &failed_jobs);

        // No bracket extraction → fallback to all files
        assert_eq!(result.len(), 1);
        assert!(result.contains(&file_dev));
    }

    #[test]
    fn test_partition_succeeded_no_groups() {
        let interner = StringInterner::new();
        let config = InputConfig::default();
        let api_client = WorkflowApiClient::new("https://api.github.com".to_string(), None);
        let tracker = WorkflowTracker::new(api_client, &config, &interner, &[]);

        let file_a = interner.intern("stacks/dev/config.yaml");
        let commit_files = vec![file_a];

        let dev_job = WorkflowJob {
            id: 1,
            name: interner.intern("Deploy [dev]"),
            status: WorkflowStatus::Completed,
            conclusion: Some(WorkflowConclusion::Success),
            run_id: 100,
            started_at: 0,
            completed_at: 0,
        };
        let succeeded_jobs: Vec<&WorkflowJob> = vec![&dev_job];

        // No groups → fallback to all files
        let result = tracker.partition_files_for_succeeded_jobs(&commit_files, &succeeded_jobs);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_partition_succeeded_multiple_jobs() {
        use crate::patterns::matcher::PatternMatcher;

        let interner = StringInterner::new();
        let config = InputConfig {
            failure_tracking_level: FailureTrackingLevel::Job,
            ..Default::default()
        };

        let dev_matcher = PatternMatcher::new(&["stacks/dev/**"], &[], true).unwrap();
        let staging_matcher = PatternMatcher::new(&["stacks/staging/**"], &[], true).unwrap();
        let prod_matcher = PatternMatcher::new(&["stacks/prod/**"], &[], true).unwrap();
        let groups = vec![
            PatternGroup {
                name: "dev".to_string(),
                matcher: dev_matcher,
            },
            PatternGroup {
                name: "staging".to_string(),
                matcher: staging_matcher,
            },
            PatternGroup {
                name: "prod".to_string(),
                matcher: prod_matcher,
            },
        ];

        let api_client = WorkflowApiClient::new("https://api.github.com".to_string(), None);
        let tracker = WorkflowTracker::new(api_client, &config, &interner, &groups);

        let file_dev = interner.intern("stacks/dev/config.yaml");
        let file_staging = interner.intern("stacks/staging/config.yaml");
        let file_prod = interner.intern("stacks/prod/config.yaml");
        let commit_files = vec![file_dev, file_staging, file_prod];

        // dev + staging succeeded, prod didn't
        let dev_job = WorkflowJob {
            id: 1,
            name: interner.intern("Deploy [dev]"),
            status: WorkflowStatus::Completed,
            conclusion: Some(WorkflowConclusion::Success),
            run_id: 100,
            started_at: 0,
            completed_at: 0,
        };
        let staging_job = WorkflowJob {
            id: 2,
            name: interner.intern("Deploy [staging]"),
            status: WorkflowStatus::Completed,
            conclusion: Some(WorkflowConclusion::Success),
            run_id: 100,
            started_at: 0,
            completed_at: 0,
        };
        let succeeded_jobs: Vec<&WorkflowJob> = vec![&dev_job, &staging_job];

        let result = tracker.partition_files_for_succeeded_jobs(&commit_files, &succeeded_jobs);

        // Only dev + staging verified, not prod
        assert_eq!(result.len(), 2);
        assert!(result.contains(&file_dev));
        assert!(result.contains(&file_staging));
        assert!(!result.contains(&file_prod));
    }

    #[test]
    fn test_partition_files_three_stacks_with_unmatched_root_file() {
        use crate::patterns::matcher::PatternMatcher;

        let interner = StringInterner::new();
        let config = InputConfig {
            failure_tracking_level: FailureTrackingLevel::Job,
            ..Default::default()
        };

        let dev_matcher = PatternMatcher::new(&["stacks/dev/**"], &[], true).unwrap();
        let staging_matcher = PatternMatcher::new(&["stacks/staging/**"], &[], true).unwrap();
        let prod_matcher = PatternMatcher::new(&["stacks/prod/**"], &[], true).unwrap();
        let groups = vec![
            PatternGroup {
                name: "dev".to_string(),
                matcher: dev_matcher,
            },
            PatternGroup {
                name: "staging".to_string(),
                matcher: staging_matcher,
            },
            PatternGroup {
                name: "prod".to_string(),
                matcher: prod_matcher,
            },
        ];

        let api_client = WorkflowApiClient::new("https://api.github.com".to_string(), None);
        let tracker = WorkflowTracker::new(api_client, &config, &interner, &groups);

        let file_dev = interner.intern("stacks/dev/config.yaml");
        let file_staging = interner.intern("stacks/staging/config.yaml");
        let file_prod = interner.intern("stacks/prod/config.yaml");
        let file_ci = interner.intern(".github/workflows/ci.yml");
        let commit_files = vec![file_dev, file_staging, file_prod, file_ci];

        // Only prod failed
        let failed_jobs = vec![WorkflowJob {
            id: 1,
            name: interner.intern("Deploy [prod]"),
            status: WorkflowStatus::Completed,
            conclusion: Some(WorkflowConclusion::Failure),
            run_id: 100,
            started_at: 0,
            completed_at: 0,
        }];

        let result = tracker.partition_files_for_failed_jobs(&commit_files, &failed_jobs);

        // prod file + ci.yml (unmatched → conservative fallback), NOT dev or staging
        assert_eq!(result.len(), 2);
        assert!(result.contains(&file_prod));
        assert!(result.contains(&file_ci));
        assert!(!result.contains(&file_dev));
        assert!(!result.contains(&file_staging));
    }

    #[test]
    fn test_merge_failed_files_with_job_names() {
        let interner = StringInterner::new();
        let path_dev = interner.intern("stacks/dev/config.yaml");
        let path_prod = interner.intern("stacks/prod/config.yaml");

        let mut current_files = vec![ChangedFile {
            path: path_dev,
            change_type: ChangeType::Modified,
            previous_path: None,
            is_symlink: false,
            submodule_depth: 0,
            origin: FileOrigin {
                in_current_changes: true,
                in_previous_failure: false,
                in_previous_success: false,
            },
        }];

        // Failure with job details
        let failures = vec![WorkflowFailure {
            run: WorkflowRun {
                id: 100,
                name: interner.intern("Deploy Stacks"),
                status: WorkflowStatus::Completed,
                conclusion: Some(WorkflowConclusion::Failure),
                branch: interner.intern("main"),
                head_sha: interner.intern("abc123"),
                created_at: 1000,
            },
            files: vec![path_prod],
            failed_jobs: vec![WorkflowJob {
                id: 10,
                name: interner.intern("Deploy [prod]"),
                status: WorkflowStatus::Completed,
                conclusion: Some(WorkflowConclusion::Failure),
                run_id: 100,
                started_at: 0,
                completed_at: 0,
            }],
        }];

        let config = InputConfig::default();
        let api_client = WorkflowApiClient::new("https://api.github.com".to_string(), None);
        let tracker = WorkflowTracker::new(api_client, &config, &interner, &[]);

        tracker.merge_failed_files(&mut current_files, &failures);

        assert_eq!(current_files.len(), 2);
        // dev is current only
        assert!(current_files[0].origin.in_current_changes);
        assert!(!current_files[0].origin.in_previous_failure);
        // prod is failure only
        assert!(!current_files[1].origin.in_current_changes);
        assert!(current_files[1].origin.in_previous_failure);
    }
}
