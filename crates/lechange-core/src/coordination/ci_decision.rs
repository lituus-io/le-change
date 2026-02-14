//! CI rebuild/skip decision engine
//!
//! Computes intelligent rebuild and skip decisions based on workflow history.
//! Uses "latest run wins" semantics: for each file, the most recent workflow
//! run determines whether it's considered verified (skip) or needs rebuild.

use crate::interner::StringInterner;
use crate::types::{
    ChangedFile, CiDecision, InternedString, RebuildReason, RebuildReasonKind,
    WorkflowConclusion, WorkflowFailure, WorkflowSuccess,
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
    use crate::types::{
        FileOrigin, ChangeType, WorkflowJob, WorkflowRun, WorkflowStatus,
    };

    fn make_interner() -> StringInterner {
        StringInterner::new()
    }

    fn make_run(id: u64, ts: i64, conclusion: WorkflowConclusion, interner: &StringInterner) -> WorkflowRun {
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
        assert!(decision.rebuild_reasons.iter().all(|r| r.kind == RebuildReasonKind::NewChange));
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
        assert_eq!(decision.rebuild_reasons[0].kind, RebuildReasonKind::PreviousFailure);
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
        let rebuild_set: HashSet<InternedString> = decision.files_to_rebuild.iter().copied().collect();
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
}
