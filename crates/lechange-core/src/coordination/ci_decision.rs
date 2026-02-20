//! CI rebuild/skip decision engine
//!
//! Computes intelligent rebuild and skip decisions based on workflow history.
//! Uses "latest run wins" semantics: for each file, the most recent workflow
//! run determines whether it's considered verified (skip) or needs rebuild.

use crate::interner::StringInterner;
use crate::types::{
    ChangedFile, CiDecision, InternedString, RebuildReason, RebuildReasonKind, WorkflowConclusion,
    WorkflowFailure, WorkflowSuccess,
};
use std::collections::{HashMap, HashSet};

/// CI decision engine that computes rebuild/skip lists
pub struct CiDecisionEngine<'a> {
    #[allow(dead_code)]
    interner: &'a StringInterner,
}

impl<'a> CiDecisionEngine<'a> {
    /// Create a new CI decision engine
    pub fn new(interner: &'a StringInterner) -> Self {
        Self { interner }
    }

    /// Compute the rebuild/skip decision from workflow analysis.
    ///
    /// Algorithm ("latest run wins"):
    /// 1. For each file across all recent workflow commits (ordered by time desc):
    ///    - Find the MOST RECENT workflow run that touched this file
    ///    - If that run succeeded -> file is "verified" (skip candidate)
    ///    - If that run failed -> file needs rebuild
    /// 2. Merge with current changes:
    ///    - files_to_rebuild = current_changes U failed_files \ verified_files
    ///    - files_to_skip = verified_files \ current_changes \ failed_files
    /// 3. A file in BOTH current_changes AND verified -> rebuild (current change takes priority)
    /// 4. A file in BOTH failed AND verified -> check timestamps (latest wins)
    pub fn compute(
        &self,
        current_files: &[ChangedFile],
        failures: &[WorkflowFailure],
        successes: &[WorkflowSuccess],
    ) -> CiDecision {
        // Build per-file latest-run map: file_path -> (run_id, created_at, passed: bool)
        let mut file_latest: HashMap<InternedString, (u64, i64, bool)> = HashMap::new();

        // Process successes
        for success in successes {
            let ts = success.run.created_at;
            let run_id = success.run.id;
            for &file in &success.files {
                let entry = file_latest.entry(file).or_insert((run_id, ts, true));
                if ts > entry.1 {
                    *entry = (run_id, ts, true); // More recent -> wins
                }
            }
        }

        // Process failures (may overwrite successes if more recent)
        for failure in failures {
            let ts = failure.run.created_at;
            let run_id = failure.run.id;
            for &file in &failure.files {
                let entry = file_latest.entry(file).or_insert((run_id, ts, false));
                if ts > entry.1 {
                    *entry = (run_id, ts, false); // More recent -> wins
                }
            }
        }

        // Current change set
        let current_set: HashSet<InternedString> = current_files
            .iter()
            .filter(|f| f.origin.in_current_changes)
            .map(|f| f.path)
            .collect();

        // Compute decisions
        let mut files_to_rebuild = Vec::new();
        let mut files_to_skip = Vec::new();
        let mut rebuild_reasons = Vec::new();

        // All current changes -> rebuild
        for file in current_files {
            if file.origin.in_current_changes {
                let kind = if file.origin.in_previous_failure {
                    RebuildReasonKind::BothNewAndFailed
                } else {
                    RebuildReasonKind::NewChange
                };
                files_to_rebuild.push(file.path);
                rebuild_reasons.push(RebuildReason {
                    file: file.path,
                    kind,
                    failed_run_id: None,
                    failed_job_name: None,
                });
            }
        }

        // Files from history not in current changes
        for (&file, &(run_id, _ts, passed)) in &file_latest {
            if current_set.contains(&file) {
                continue; // Already handled above
            }
            if passed {
                files_to_skip.push(file);
            } else {
                files_to_rebuild.push(file);
                // Find which job failed for this run
                let failed_job = failures
                    .iter()
                    .find(|f| f.run.id == run_id)
                    .and_then(|f| f.failed_jobs.first())
                    .map(|j| j.name);
                rebuild_reasons.push(RebuildReason {
                    file,
                    kind: RebuildReasonKind::PreviousFailure,
                    failed_run_id: Some(run_id),
                    failed_job_name: failed_job,
                });
            }
        }

        // Collect failed/successful job names
        let mut failed_jobs: HashSet<InternedString> = HashSet::new();
        let mut successful_jobs: HashSet<InternedString> = HashSet::new();

        for failure in failures {
            for job in &failure.failed_jobs {
                failed_jobs.insert(job.name);
            }
        }
        for success in successes {
            for job in &success.jobs {
                if job.conclusion == Some(WorkflowConclusion::Success) {
                    successful_jobs.insert(job.name);
                }
            }
        }

        CiDecision {
            files_to_rebuild,
            files_to_skip,
            failed_jobs: failed_jobs.into_iter().collect(),
            successful_jobs: successful_jobs.into_iter().collect(),
            rebuild_reasons,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChangeType, FileOrigin, WorkflowJob, WorkflowRun, WorkflowStatus};

    fn make_interner() -> StringInterner {
        StringInterner::new()
    }

    fn make_run(
        id: u64,
        ts: i64,
        conclusion: WorkflowConclusion,
        interner: &StringInterner,
    ) -> WorkflowRun {
        WorkflowRun {
            id,
            name: interner.intern("CI"),
            status: WorkflowStatus::Completed,
            conclusion: Some(conclusion),
            branch: interner.intern("main"),
            head_sha: interner.intern(&format!("sha{}", id)),
            created_at: ts,
        }
    }

    fn make_current_file(path: InternedString) -> ChangedFile {
        ChangedFile {
            path,
            change_type: ChangeType::Modified,
            previous_path: None,
            is_symlink: false,
            submodule_depth: 0,
            origin: FileOrigin {
                in_current_changes: true,
                in_previous_failure: false,
                in_previous_success: false,
            },
        }
    }

    #[test]
    fn test_new_changes_only() {
        let interner = make_interner();
        let file_a = interner.intern("a.rs");
        let file_b = interner.intern("b.rs");

        let current = vec![make_current_file(file_a), make_current_file(file_b)];

        let engine = CiDecisionEngine::new(&interner);
        let decision = engine.compute(&current, &[], &[]);

        assert_eq!(decision.files_to_rebuild.len(), 2);
        assert!(decision.files_to_skip.is_empty());
        assert!(decision
            .rebuild_reasons
            .iter()
            .all(|r| r.kind == RebuildReasonKind::NewChange));
    }

    #[test]
    fn test_previous_failure_only() {
        let interner = make_interner();
        let file_a = interner.intern("a.rs");

        let failures = vec![WorkflowFailure {
            run: make_run(1, 100, WorkflowConclusion::Failure, &interner),
            files: vec![file_a],
            failed_jobs: Vec::new(),
        }];

        let engine = CiDecisionEngine::new(&interner);
        let decision = engine.compute(&[], &failures, &[]);

        assert_eq!(decision.files_to_rebuild.len(), 1);
        assert!(decision.files_to_skip.is_empty());
        assert_eq!(
            decision.rebuild_reasons[0].kind,
            RebuildReasonKind::PreviousFailure
        );
    }

    #[test]
    fn test_previous_success_only() {
        let interner = make_interner();
        let file_a = interner.intern("a.rs");

        let successes = vec![WorkflowSuccess {
            run: make_run(1, 100, WorkflowConclusion::Success, &interner),
            jobs: Vec::new(),
            files: vec![file_a],
        }];

        let engine = CiDecisionEngine::new(&interner);
        let decision = engine.compute(&[], &[], &successes);

        assert!(decision.files_to_rebuild.is_empty());
        assert_eq!(decision.files_to_skip.len(), 1);
    }

    #[test]
    fn test_latest_run_wins_success_after_failure() {
        let interner = make_interner();
        let file_a = interner.intern("a.rs");

        let failures = vec![WorkflowFailure {
            run: make_run(1, 100, WorkflowConclusion::Failure, &interner),
            files: vec![file_a],
            failed_jobs: Vec::new(),
        }];

        let successes = vec![WorkflowSuccess {
            run: make_run(2, 200, WorkflowConclusion::Success, &interner), // More recent
            jobs: Vec::new(),
            files: vec![file_a],
        }];

        let engine = CiDecisionEngine::new(&interner);
        let decision = engine.compute(&[], &failures, &successes);

        // Success is more recent, so skip
        assert!(decision.files_to_rebuild.is_empty());
        assert_eq!(decision.files_to_skip.len(), 1);
    }

    #[test]
    fn test_latest_run_wins_failure_after_success() {
        let interner = make_interner();
        let file_a = interner.intern("a.rs");

        let successes = vec![WorkflowSuccess {
            run: make_run(1, 100, WorkflowConclusion::Success, &interner),
            jobs: Vec::new(),
            files: vec![file_a],
        }];

        let failures = vec![WorkflowFailure {
            run: make_run(2, 200, WorkflowConclusion::Failure, &interner), // More recent
            files: vec![file_a],
            failed_jobs: Vec::new(),
        }];

        let engine = CiDecisionEngine::new(&interner);
        let decision = engine.compute(&[], &failures, &successes);

        // Failure is more recent, so rebuild
        assert_eq!(decision.files_to_rebuild.len(), 1);
        assert!(decision.files_to_skip.is_empty());
    }

    #[test]
    fn test_current_changes_always_rebuild() {
        let interner = make_interner();
        let file_a = interner.intern("a.rs");

        let current = vec![make_current_file(file_a)];

        // Even with a recent success, current changes should rebuild
        let successes = vec![WorkflowSuccess {
            run: make_run(1, 200, WorkflowConclusion::Success, &interner),
            jobs: Vec::new(),
            files: vec![file_a],
        }];

        let engine = CiDecisionEngine::new(&interner);
        let decision = engine.compute(&current, &[], &successes);

        assert_eq!(decision.files_to_rebuild.len(), 1);
        // file_a appears in rebuild (current change) but NOT in skip
        assert!(!decision.files_to_skip.contains(&file_a));
    }

    #[test]
    fn test_rebuild_skip_disjoint() {
        let interner = make_interner();
        let file_a = interner.intern("a.rs");
        let file_b = interner.intern("b.rs");
        let file_c = interner.intern("c.rs");

        let current = vec![make_current_file(file_a)];

        let failures = vec![WorkflowFailure {
            run: make_run(1, 100, WorkflowConclusion::Failure, &interner),
            files: vec![file_b],
            failed_jobs: Vec::new(),
        }];

        let successes = vec![WorkflowSuccess {
            run: make_run(2, 200, WorkflowConclusion::Success, &interner),
            jobs: Vec::new(),
            files: vec![file_c],
        }];

        let engine = CiDecisionEngine::new(&interner);
        let decision = engine.compute(&current, &failures, &successes);

        // rebuild = {a (current), b (failed)}
        // skip = {c (success)}
        let rebuild_set: HashSet<InternedString> =
            decision.files_to_rebuild.iter().copied().collect();
        let skip_set: HashSet<InternedString> = decision.files_to_skip.iter().copied().collect();

        // Invariant: rebuild ∩ skip = empty
        assert!(rebuild_set.is_disjoint(&skip_set));

        assert!(rebuild_set.contains(&file_a));
        assert!(rebuild_set.contains(&file_b));
        assert!(skip_set.contains(&file_c));
    }

    #[test]
    fn test_empty_workflows() {
        let interner = make_interner();
        let engine = CiDecisionEngine::new(&interner);
        let decision = engine.compute(&[], &[], &[]);

        assert!(decision.files_to_rebuild.is_empty());
        assert!(decision.files_to_skip.is_empty());
        assert!(decision.rebuild_reasons.is_empty());
    }

    #[test]
    fn test_job_level_tracking() {
        let interner = make_interner();
        let file_a = interner.intern("a.rs");
        let job_name = interner.intern("build");
        let success_job_name = interner.intern("lint");

        let failures = vec![WorkflowFailure {
            run: make_run(1, 100, WorkflowConclusion::Failure, &interner),
            files: vec![file_a],
            failed_jobs: vec![WorkflowJob {
                id: 10,
                name: job_name,
                status: WorkflowStatus::Completed,
                conclusion: Some(WorkflowConclusion::Failure),
                run_id: 1,
                started_at: 90,
                completed_at: 100,
            }],
        }];

        let successes = vec![WorkflowSuccess {
            run: make_run(2, 50, WorkflowConclusion::Success, &interner), // Older
            jobs: vec![WorkflowJob {
                id: 20,
                name: success_job_name,
                status: WorkflowStatus::Completed,
                conclusion: Some(WorkflowConclusion::Success),
                run_id: 2,
                started_at: 40,
                completed_at: 50,
            }],
            files: vec![],
        }];

        let engine = CiDecisionEngine::new(&interner);
        let decision = engine.compute(&[], &failures, &successes);

        assert!(decision.failed_jobs.contains(&job_name));
        assert!(decision.successful_jobs.contains(&success_job_name));
    }

    /// End-to-end scenario: job-level tracking across two workflow runs.
    ///
    /// Run 1 (ts=100): Commit changes dev + prod configs.
    ///   - Matrix workflow "Deploy" runs with jobs [dev], [prod], [staging]
    ///   - Job "Deploy [dev]" succeeds
    ///   - Job "Deploy [prod]" fails
    ///   - The overall run is marked as "failure" (any job fail = run fail)
    ///   - With job-level partitioning:
    ///     - WorkflowFailure.files = [stacks/prod/config.yaml] (only prod's files)
    ///     - WorkflowSuccess.files = [stacks/dev/config.yaml]  (only dev's files)
    ///
    /// Run 2 (current): Commit changes only staging config.
    ///   - Current files: [stacks/staging/config.yaml]
    ///   - Workflow history from Run 1 is available
    ///
    /// Expected decisions for Run 2:
    ///   - stacks/staging/config.yaml → REBUILD (new change)
    ///   - stacks/prod/config.yaml    → REBUILD (previous failure, job "Deploy [prod]")
    ///   - stacks/dev/config.yaml     → SKIP    (previously succeeded, no new changes)
    #[test]
    fn test_job_level_two_run_scenario() {
        use crate::coordination::workflow_tracker::extract_job_key;
        use crate::http::WorkflowApiClient;
        use crate::patterns::loader::PatternGroup;
        use crate::patterns::matcher::PatternMatcher;
        use crate::types::FailureTrackingLevel;

        let interner = make_interner();

        // --- Setup YAML groups (same as what the workflow YAML would define) ---
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

        // --- Setup config ---
        let config = crate::types::InputConfig {
            failure_tracking_level: FailureTrackingLevel::Job,
            ..Default::default()
        };

        // --- Intern file paths ---
        let file_dev = interner.intern("stacks/dev/config.yaml");
        let file_staging = interner.intern("stacks/staging/config.yaml");
        let file_prod = interner.intern("stacks/prod/config.yaml");

        // ================================================================
        // Run 1 (ts=100): Commit changed dev + prod
        // The run-level conclusion is "failure" because prod job failed.
        // But job-level tracking partitions the files per job.
        // ================================================================
        let run1_commit_files = vec![file_dev, file_prod];

        // Jobs from Run 1
        let job_deploy_dev = WorkflowJob {
            id: 10,
            name: interner.intern("Deploy [dev]"),
            status: WorkflowStatus::Completed,
            conclusion: Some(WorkflowConclusion::Success),
            run_id: 1,
            started_at: 90,
            completed_at: 100,
        };
        let job_deploy_prod = WorkflowJob {
            id: 11,
            name: interner.intern("Deploy [prod]"),
            status: WorkflowStatus::Completed,
            conclusion: Some(WorkflowConclusion::Failure),
            run_id: 1,
            started_at: 90,
            completed_at: 100,
        };

        // Verify extract_job_key works on these names
        assert_eq!(extract_job_key("Deploy [dev]"), Some("dev"));
        assert_eq!(extract_job_key("Deploy [prod]"), Some("prod"));

        // Create the WorkflowTracker to use its partition logic
        let api_client = WorkflowApiClient::new("https://api.github.com".to_string(), None);
        let tracker =
            crate::coordination::WorkflowTracker::new(api_client, &config, &interner, &groups);

        // Partition files for the FAILED jobs from Run 1
        let failed_jobs = vec![job_deploy_prod];
        let failure_files =
            tracker.partition_files_for_failed_jobs(&run1_commit_files, &failed_jobs);

        // Partition files for the SUCCEEDED jobs from Run 1
        let succeeded_jobs_refs: Vec<&WorkflowJob> = vec![&job_deploy_dev];
        let success_files =
            tracker.partition_files_for_succeeded_jobs(&run1_commit_files, &succeeded_jobs_refs);

        // Verify partitioning: failure should only have prod, success should only have dev
        assert_eq!(
            failure_files.len(),
            1,
            "Failure should contain only prod file"
        );
        assert!(
            failure_files.contains(&file_prod),
            "Failure files must contain prod"
        );
        assert!(
            !failure_files.contains(&file_dev),
            "Failure files must NOT contain dev"
        );

        assert_eq!(
            success_files.len(),
            1,
            "Success should contain only dev file"
        );
        assert!(
            success_files.contains(&file_dev),
            "Success files must contain dev"
        );
        assert!(
            !success_files.contains(&file_prod),
            "Success files must NOT contain prod"
        );

        // Build WorkflowFailure / WorkflowSuccess from Run 1 (as the tracker would produce)
        let run1_failure = WorkflowFailure {
            run: WorkflowRun {
                id: 1,
                name: interner.intern("Deploy"),
                status: WorkflowStatus::Completed,
                conclusion: Some(WorkflowConclusion::Failure),
                branch: interner.intern("main"),
                head_sha: interner.intern("sha_run1"),
                created_at: 100,
            },
            files: failure_files, // Only prod (job-level partitioned)
            failed_jobs: vec![job_deploy_prod],
        };

        let run1_success = WorkflowSuccess {
            run: WorkflowRun {
                id: 1,
                name: interner.intern("Deploy"),
                status: WorkflowStatus::Completed,
                // Note: the run-level conclusion is Failure, but we create a separate
                // WorkflowSuccess for the succeeded jobs' files. In practice, a run
                // that has mixed results (some jobs pass, some fail) would appear in
                // both the failures list (with failed job files) and we synthesize
                // success entries for the passed jobs' files.
                conclusion: Some(WorkflowConclusion::Failure),
                branch: interner.intern("main"),
                head_sha: interner.intern("sha_run1"),
                created_at: 100,
            },
            jobs: vec![job_deploy_dev],
            files: success_files, // Only dev (job-level partitioned)
        };

        // ================================================================
        // Run 2 (current): Commit changes only staging
        // ================================================================
        let current_files = vec![make_current_file(file_staging)];

        // Feed into CiDecisionEngine
        let engine = CiDecisionEngine::new(&interner);
        let decision = engine.compute(&current_files, &[run1_failure], &[run1_success]);

        // --- Verify Run 2 decisions ---
        let rebuild_set: HashSet<InternedString> =
            decision.files_to_rebuild.iter().copied().collect();
        let skip_set: HashSet<InternedString> = decision.files_to_skip.iter().copied().collect();

        // Invariant: rebuild and skip are disjoint
        assert!(
            rebuild_set.is_disjoint(&skip_set),
            "rebuild and skip sets must be disjoint"
        );

        // staging: REBUILD (new change in current commit)
        assert!(
            rebuild_set.contains(&file_staging),
            "staging should REBUILD (new change)"
        );

        // prod: REBUILD (previous failure from Run 1, job Deploy [prod] failed)
        assert!(
            rebuild_set.contains(&file_prod),
            "prod should REBUILD (previous failure)"
        );

        // dev: SKIP (previously succeeded in Run 1, no new changes)
        assert!(
            skip_set.contains(&file_dev),
            "dev should SKIP (previously succeeded, not changed)"
        );

        // dev must NOT be in rebuild
        assert!(
            !rebuild_set.contains(&file_dev),
            "dev must NOT rebuild — it succeeded and has no new changes"
        );

        // Verify rebuild reasons
        let staging_reason = decision
            .rebuild_reasons
            .iter()
            .find(|r| r.file == file_staging)
            .expect("staging should have a rebuild reason");
        assert_eq!(staging_reason.kind, RebuildReasonKind::NewChange);

        let prod_reason = decision
            .rebuild_reasons
            .iter()
            .find(|r| r.file == file_prod)
            .expect("prod should have a rebuild reason");
        assert_eq!(prod_reason.kind, RebuildReasonKind::PreviousFailure);
        assert_eq!(prod_reason.failed_run_id, Some(1));
        assert_eq!(
            interner.resolve(prod_reason.failed_job_name.unwrap()),
            Some("Deploy [prod]"),
            "rebuild reason should reference the specific failed job"
        );

        // Verify job-level metadata
        assert!(
            decision
                .failed_jobs
                .iter()
                .any(|j| interner.resolve(*j) == Some("Deploy [prod]")),
            "failed_jobs should contain Deploy [prod]"
        );
        assert!(
            decision
                .successful_jobs
                .iter()
                .any(|j| interner.resolve(*j) == Some("Deploy [dev]")),
            "successful_jobs should contain Deploy [dev]"
        );

        // Total: 2 rebuild (staging + prod), 1 skip (dev)
        assert_eq!(
            decision.files_to_rebuild.len(),
            2,
            "should rebuild staging + prod"
        );
        assert_eq!(decision.files_to_skip.len(), 1, "should skip dev only");
    }
}
