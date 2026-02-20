//! Computed output categories from ProcessedResult

use crate::interner::StringInterner;
use crate::types::{
    ChangeType, GroupDeployAction, GroupDeployDecision, GroupDeployReason, GroupResult,
    InternedString, ProcessedResult, RebuildReasonKind,
};
use std::collections::{HashMap, HashSet};

/// All derived output categories computed in a single pass
pub struct ComputedOutputs {
    /// Filtered added file indices
    pub filtered_added: Vec<u32>,
    /// Filtered copied file indices
    pub filtered_copied: Vec<u32>,
    /// Filtered deleted file indices
    pub filtered_deleted: Vec<u32>,
    /// Filtered modified file indices
    pub filtered_modified: Vec<u32>,
    /// Filtered renamed file indices
    pub filtered_renamed: Vec<u32>,
    /// Filtered type-changed file indices
    pub filtered_type_changed: Vec<u32>,
    /// Filtered unmerged file indices
    pub filtered_unmerged: Vec<u32>,
    /// Filtered unknown file indices
    pub filtered_unknown: Vec<u32>,
    /// "Other changed" indices: ACMR not in filter
    pub other_changed: Vec<u32>,
    /// "Other modified" indices: ACMRD not in filter
    pub other_modified: Vec<u32>,
    /// "Other deleted" indices: D not in filter
    pub other_deleted: Vec<u32>,
    /// All changed and modified superset
    pub all_changed_and_modified: Vec<u32>,
    /// Rename mapping: (index, previous_path)
    pub renamed_mapping: Vec<(u32, InternedString)>,
    /// When output_renamed_as_deleted_added is true, contains (index, previous_path)
    /// for renamed files split into deleted entries. The new name goes to filtered_added.
    pub rename_split_deletions: Vec<(u32, InternedString)>,
    /// YAML group keys that had modified matches
    pub modified_keys: Vec<InternedString>,
    /// YAML group keys that had any changed matches
    pub changed_keys: Vec<InternedString>,
    /// Per-group deploy decisions (populated when YAML groups are present)
    pub group_deploy_decisions: Vec<GroupDeployDecision>,
}

impl ComputedOutputs {
    /// Single-pass computation from a ProcessedResult
    ///
    /// When `output_renamed_as_deleted_added` is true, renamed files are split:
    /// the new path goes to `filtered_added` and the old path is stored in
    /// `rename_split_deletions` (the consumer should include these in deleted output).
    ///
    /// The `interner` parameter is used to look up group keys for concurrency tracking.
    pub fn compute(result: &ProcessedResult, output_renamed_as_deleted_added: bool) -> Self {
        Self::compute_with_concurrency(result, output_renamed_as_deleted_added, None, None)
    }

    /// Compute with concurrency information from workflow check.
    ///
    /// `blocked_groups` maps group keys to blocking run IDs.
    /// `interner` resolves InternedString keys for matching.
    pub fn compute_with_concurrency(
        result: &ProcessedResult,
        output_renamed_as_deleted_added: bool,
        blocked_groups: Option<&HashMap<InternedString, Vec<u64>>>,
        _interner: Option<&StringInterner>,
    ) -> Self {
        let filtered_set: HashSet<u32> = result.filtered_indices.iter().copied().collect();
        let unmatched_set: HashSet<u32> = result.unmatched_indices.iter().copied().collect();

        let mut out = Self {
            filtered_added: Vec::new(),
            filtered_copied: Vec::new(),
            filtered_deleted: Vec::new(),
            filtered_modified: Vec::new(),
            filtered_renamed: Vec::new(),
            filtered_type_changed: Vec::new(),
            filtered_unmerged: Vec::new(),
            filtered_unknown: Vec::new(),
            other_changed: Vec::new(),
            other_modified: Vec::new(),
            other_deleted: Vec::new(),
            all_changed_and_modified: Vec::new(),
            renamed_mapping: Vec::new(),
            rename_split_deletions: Vec::new(),
            modified_keys: Vec::new(),
            changed_keys: Vec::new(),
            group_deploy_decisions: Vec::new(),
        };

        for (i, file) in result.all_files.iter().enumerate() {
            let idx = i as u32;
            let in_filter = filtered_set.contains(&idx);
            let in_unmatched = unmatched_set.contains(&idx);

            // All changed and modified includes everything
            out.all_changed_and_modified.push(idx);

            if in_filter {
                match file.change_type {
                    ChangeType::Added => out.filtered_added.push(idx),
                    ChangeType::Copied => out.filtered_copied.push(idx),
                    ChangeType::Deleted => out.filtered_deleted.push(idx),
                    ChangeType::Modified => out.filtered_modified.push(idx),
                    ChangeType::Renamed => {
                        if output_renamed_as_deleted_added {
                            // Split: new path → added, old path → deleted (stored separately)
                            out.filtered_added.push(idx);
                            if let Some(prev) = file.previous_path {
                                out.rename_split_deletions.push((idx, prev));
                            }
                        } else {
                            out.filtered_renamed.push(idx);
                            if let Some(prev) = file.previous_path {
                                out.renamed_mapping.push((idx, prev));
                            }
                        }
                    }
                    ChangeType::TypeChanged => out.filtered_type_changed.push(idx),
                    ChangeType::Unmerged => out.filtered_unmerged.push(idx),
                    ChangeType::Unknown => out.filtered_unknown.push(idx),
                }
            }

            if in_unmatched {
                // "Other changed" = ACMR not in filter
                match file.change_type {
                    ChangeType::Added
                    | ChangeType::Copied
                    | ChangeType::Modified
                    | ChangeType::Renamed => {
                        out.other_changed.push(idx);
                    }
                    _ => {}
                }

                // "Other modified" = ACMRD not in filter
                match file.change_type {
                    ChangeType::Added
                    | ChangeType::Copied
                    | ChangeType::Modified
                    | ChangeType::Renamed
                    | ChangeType::Deleted => {
                        out.other_modified.push(idx);
                    }
                    _ => {}
                }

                // "Other deleted" = D not in filter
                if file.change_type == ChangeType::Deleted {
                    out.other_deleted.push(idx);
                }
            }
        }

        // Compute group keys (InternedString is Copy — no allocation)
        for group in &result.group_results {
            if !group.matched_indices.is_empty() {
                out.changed_keys.push(group.key);

                let has_modified = group.matched_indices.iter().any(|&idx| {
                    result
                        .all_files
                        .get(idx as usize)
                        .map(|f| f.change_type == ChangeType::Modified)
                        .unwrap_or(false)
                });

                if has_modified {
                    out.modified_keys.push(group.key);
                }
            }
        }

        // Helper: look up concurrency info for a group key
        let concurrency_for = |key: InternedString| -> (bool, u32) {
            blocked_groups
                .and_then(|bg| bg.get(&key))
                .map(|ids| (true, ids.len() as u32))
                .unwrap_or((false, 0))
        };

        // Helper: resolve group matched indices to file paths
        let resolve_group_paths = |group: &GroupResult| -> Vec<InternedString> {
            group
                .matched_indices
                .iter()
                .filter_map(|&idx| result.all_files.get(idx as usize).map(|f| f.path))
                .collect()
        };

        // Compute group deploy decisions
        if !result.group_results.is_empty() {
            if let Some(ref ci) = result.ci_decision {
                // Build lookup sets from CiDecision
                let rebuild_set: HashSet<InternedString> =
                    ci.files_to_rebuild.iter().copied().collect();
                let skip_set: HashSet<InternedString> = ci.files_to_skip.iter().copied().collect();
                let reasons_map: HashMap<InternedString, RebuildReasonKind> = ci
                    .rebuild_reasons
                    .iter()
                    .map(|r| (r.file, r.kind))
                    .collect();

                for group in &result.group_results {
                    let group_paths = resolve_group_paths(group);

                    if group_paths.is_empty() {
                        continue;
                    }

                    // Partition into rebuild/skip
                    let mut group_rebuild = Vec::new();
                    let mut group_skip = Vec::new();
                    for &path in &group_paths {
                        if rebuild_set.contains(&path) {
                            group_rebuild.push(path);
                        } else if skip_set.contains(&path) {
                            group_skip.push(path);
                        } else {
                            // File not in CI decision — treat as needing rebuild
                            group_rebuild.push(path);
                        }
                    }

                    let total_files = group_paths.len() as u32;

                    let (cb, cb_by) = concurrency_for(group.key);

                    if group_rebuild.is_empty() {
                        out.group_deploy_decisions.push(GroupDeployDecision {
                            key: group.key,
                            action: GroupDeployAction::Skip,
                            reason: None,
                            files_to_rebuild: Vec::new(),
                            files_to_skip: group_skip,
                            total_files,
                            concurrency_blocked: cb,
                            concurrency_blocked_by: cb_by,
                        });
                    } else {
                        let has_new = group_rebuild.iter().any(|p| {
                            matches!(
                                reasons_map.get(p),
                                Some(RebuildReasonKind::NewChange)
                                    | Some(RebuildReasonKind::BothNewAndFailed)
                            )
                        });
                        let has_failure = group_rebuild.iter().any(|p| {
                            matches!(
                                reasons_map.get(p),
                                Some(RebuildReasonKind::PreviousFailure)
                                    | Some(RebuildReasonKind::BothNewAndFailed)
                            )
                        });

                        let reason = match (has_new, has_failure) {
                            (true, true) => GroupDeployReason::BothNewAndFailed,
                            (false, true) => GroupDeployReason::PreviousFailure,
                            _ => GroupDeployReason::NewChange,
                        };

                        out.group_deploy_decisions.push(GroupDeployDecision {
                            key: group.key,
                            action: GroupDeployAction::Deploy,
                            reason: Some(reason),
                            files_to_rebuild: group_rebuild,
                            files_to_skip: group_skip,
                            total_files,
                            concurrency_blocked: cb,
                            concurrency_blocked_by: cb_by,
                        });
                    }
                }
            } else {
                // No CI decision — all groups with files get Deploy/NewChange
                for group in &result.group_results {
                    let group_paths = resolve_group_paths(group);

                    if group_paths.is_empty() {
                        continue;
                    }

                    let total_files = group_paths.len() as u32;
                    let (cb, cb_by) = concurrency_for(group.key);
                    out.group_deploy_decisions.push(GroupDeployDecision {
                        key: group.key,
                        action: GroupDeployAction::Deploy,
                        reason: Some(GroupDeployReason::NewChange),
                        files_to_rebuild: group_paths,
                        files_to_skip: Vec::new(),
                        total_files,
                        concurrency_blocked: cb,
                        concurrency_blocked_by: cb_by,
                    });
                }
            }
        }

        out
    }

    /// Any files in the changed category (filtered)
    pub fn any_changed(&self) -> bool {
        !self.filtered_added.is_empty()
            || !self.filtered_copied.is_empty()
            || !self.filtered_modified.is_empty()
            || !self.filtered_renamed.is_empty()
    }

    /// Only one file in the changed category
    pub fn only_changed(&self) -> bool {
        let count = self.filtered_added.len()
            + self.filtered_copied.len()
            + self.filtered_modified.len()
            + self.filtered_renamed.len();
        count == 1
    }

    /// Any modified files (filtered)
    pub fn any_modified(&self) -> bool {
        !self.filtered_modified.is_empty()
    }

    /// Only one modified file
    pub fn only_modified(&self) -> bool {
        self.filtered_modified.len() == 1
            && self.filtered_added.is_empty()
            && self.filtered_copied.is_empty()
            && self.filtered_renamed.is_empty()
            && self.filtered_deleted.is_empty()
    }

    /// Any groups with Deploy action
    pub fn has_deployable_groups(&self) -> bool {
        self.group_deploy_decisions
            .iter()
            .any(|d| d.action == GroupDeployAction::Deploy)
    }

    /// Any deleted files (filtered)
    pub fn any_deleted(&self) -> bool {
        !self.filtered_deleted.is_empty()
    }

    /// Only one deleted file
    pub fn only_deleted(&self) -> bool {
        self.filtered_deleted.len() == 1
            && self.filtered_added.is_empty()
            && self.filtered_copied.is_empty()
            && self.filtered_modified.is_empty()
            && self.filtered_renamed.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interner::StringInterner;
    use crate::types::{ChangedFile, CiDecision, FileOrigin, GroupResult, RebuildReason};

    fn make_file(change_type: ChangeType, idx: u32) -> ChangedFile {
        ChangedFile {
            path: InternedString(idx),
            change_type,
            previous_path: None,
            is_symlink: false,
            submodule_depth: 0,
            origin: FileOrigin::default(),
        }
    }

    #[test]
    fn test_compute_basic() {
        let result = ProcessedResult {
            all_files: vec![
                make_file(ChangeType::Added, 0),
                make_file(ChangeType::Modified, 1),
                make_file(ChangeType::Deleted, 2),
            ],
            filtered_indices: vec![0, 1],
            unmatched_indices: vec![2],
            pattern_applied: true,
            group_results: Vec::new(),
            additions: 0,
            deletions: 0,
            diagnostics: Vec::new(),
            workflow_result: None,
            ci_decision: None,
        };

        let outputs = ComputedOutputs::compute(&result, false);
        assert_eq!(outputs.filtered_added, vec![0]);
        assert_eq!(outputs.filtered_modified, vec![1]);
        assert!(outputs.filtered_deleted.is_empty());
        assert_eq!(outputs.other_deleted, vec![2]);
        assert!(outputs.any_changed());
        assert!(!outputs.only_changed());
    }

    #[test]
    fn test_compute_unfiltered() {
        let result = ProcessedResult {
            all_files: vec![make_file(ChangeType::Modified, 0)],
            filtered_indices: vec![0],
            unmatched_indices: Vec::new(),
            pattern_applied: false,
            group_results: Vec::new(),
            additions: 0,
            deletions: 0,
            diagnostics: Vec::new(),
            workflow_result: None,
            ci_decision: None,
        };

        let outputs = ComputedOutputs::compute(&result, false);
        assert!(outputs.any_modified());
        assert!(outputs.only_modified());
    }

    #[test]
    fn test_compute_rename_splitting() {
        let result = ProcessedResult {
            all_files: vec![
                ChangedFile {
                    path: InternedString(0), // new name
                    change_type: ChangeType::Renamed,
                    previous_path: Some(InternedString(10)), // old name
                    is_symlink: false,
                    submodule_depth: 0,
                    origin: FileOrigin::default(),
                },
                make_file(ChangeType::Modified, 1),
            ],
            filtered_indices: vec![0, 1],
            unmatched_indices: Vec::new(),
            pattern_applied: true,
            group_results: Vec::new(),
            additions: 0,
            deletions: 0,
            diagnostics: Vec::new(),
            workflow_result: None,
            ci_decision: None,
        };

        // Without splitting
        let outputs = ComputedOutputs::compute(&result, false);
        assert_eq!(outputs.filtered_renamed, vec![0]);
        assert!(outputs.filtered_added.is_empty());
        assert!(outputs.rename_split_deletions.is_empty());
        assert_eq!(outputs.renamed_mapping.len(), 1);

        // With splitting
        let outputs = ComputedOutputs::compute(&result, true);
        assert!(outputs.filtered_renamed.is_empty());
        assert_eq!(outputs.filtered_added, vec![0]); // new name → added
        assert_eq!(outputs.rename_split_deletions.len(), 1); // old name → deleted
        assert_eq!(outputs.rename_split_deletions[0], (0, InternedString(10)));
        assert!(outputs.renamed_mapping.is_empty());
    }

    #[test]
    fn test_group_keys() {
        let interner = StringInterner::new();
        let frontend_key = interner.intern("frontend");
        let backend_key = interner.intern("backend");

        let result = ProcessedResult {
            all_files: vec![
                make_file(ChangeType::Modified, 0),
                make_file(ChangeType::Added, 1),
            ],
            filtered_indices: vec![0, 1],
            unmatched_indices: Vec::new(),
            pattern_applied: true,
            group_results: vec![
                GroupResult {
                    key: frontend_key,
                    matched_indices: vec![0],
                },
                GroupResult {
                    key: backend_key,
                    matched_indices: vec![1],
                },
            ],
            additions: 0,
            deletions: 0,
            diagnostics: Vec::new(),
            workflow_result: None,
            ci_decision: None,
        };

        let outputs = ComputedOutputs::compute(&result, false);
        assert_eq!(outputs.changed_keys, vec![frontend_key, backend_key]);
        assert_eq!(outputs.modified_keys, vec![frontend_key]); // Only Modified type
    }

    #[test]
    fn test_deploy_decisions_with_ci_decision_mixed() {
        let interner = StringInterner::new();
        let dev_key = interner.intern("dev");
        let staging_key = interner.intern("staging");
        let prod_key = interner.intern("prod");

        let result = ProcessedResult {
            all_files: vec![
                make_file(ChangeType::Modified, 0),
                make_file(ChangeType::Modified, 1),
                make_file(ChangeType::Modified, 2),
            ],
            filtered_indices: vec![0, 1, 2],
            unmatched_indices: Vec::new(),
            pattern_applied: true,
            group_results: vec![
                GroupResult {
                    key: dev_key,
                    matched_indices: vec![0],
                },
                GroupResult {
                    key: staging_key,
                    matched_indices: vec![1],
                },
                GroupResult {
                    key: prod_key,
                    matched_indices: vec![2],
                },
            ],
            additions: 0,
            deletions: 0,
            diagnostics: Vec::new(),
            workflow_result: None,
            ci_decision: Some(CiDecision {
                files_to_rebuild: vec![InternedString(0), InternedString(2)],
                files_to_skip: vec![InternedString(1)],
                failed_jobs: Vec::new(),
                successful_jobs: Vec::new(),
                rebuild_reasons: vec![
                    RebuildReason {
                        file: InternedString(0),
                        kind: RebuildReasonKind::NewChange,
                        failed_run_id: None,
                        failed_job_name: None,
                    },
                    RebuildReason {
                        file: InternedString(2),
                        kind: RebuildReasonKind::PreviousFailure,
                        failed_run_id: Some(100),
                        failed_job_name: None,
                    },
                ],
            }),
        };

        let outputs = ComputedOutputs::compute(&result, false);
        assert_eq!(outputs.group_deploy_decisions.len(), 3);

        assert_eq!(outputs.group_deploy_decisions[0].key, dev_key);
        assert_eq!(
            outputs.group_deploy_decisions[0].action,
            GroupDeployAction::Deploy
        );
        assert_eq!(
            outputs.group_deploy_decisions[0].reason,
            Some(GroupDeployReason::NewChange)
        );

        assert_eq!(outputs.group_deploy_decisions[1].key, staging_key);
        assert_eq!(
            outputs.group_deploy_decisions[1].action,
            GroupDeployAction::Skip
        );
        assert!(outputs.group_deploy_decisions[1].reason.is_none());

        assert_eq!(outputs.group_deploy_decisions[2].key, prod_key);
        assert_eq!(
            outputs.group_deploy_decisions[2].action,
            GroupDeployAction::Deploy
        );
        assert_eq!(
            outputs.group_deploy_decisions[2].reason,
            Some(GroupDeployReason::PreviousFailure)
        );

        assert!(outputs.has_deployable_groups());
    }

    #[test]
    fn test_deploy_decisions_without_ci_decision() {
        let interner = StringInterner::new();
        let dev_key = interner.intern("dev");
        let prod_key = interner.intern("prod");

        let result = ProcessedResult {
            all_files: vec![
                make_file(ChangeType::Added, 0),
                make_file(ChangeType::Modified, 1),
            ],
            filtered_indices: vec![0, 1],
            unmatched_indices: Vec::new(),
            pattern_applied: true,
            group_results: vec![
                GroupResult {
                    key: dev_key,
                    matched_indices: vec![0],
                },
                GroupResult {
                    key: prod_key,
                    matched_indices: vec![1],
                },
            ],
            additions: 0,
            deletions: 0,
            diagnostics: Vec::new(),
            workflow_result: None,
            ci_decision: None,
        };

        let outputs = ComputedOutputs::compute(&result, false);
        assert_eq!(outputs.group_deploy_decisions.len(), 2);

        for d in &outputs.group_deploy_decisions {
            assert_eq!(d.action, GroupDeployAction::Deploy);
            assert_eq!(d.reason, Some(GroupDeployReason::NewChange));
        }
        assert!(outputs.has_deployable_groups());
    }

    #[test]
    fn test_deploy_decisions_empty_groups() {
        let result = ProcessedResult {
            all_files: vec![make_file(ChangeType::Modified, 0)],
            filtered_indices: vec![0],
            unmatched_indices: Vec::new(),
            pattern_applied: false,
            group_results: Vec::new(),
            additions: 0,
            deletions: 0,
            diagnostics: Vec::new(),
            workflow_result: None,
            ci_decision: None,
        };

        let outputs = ComputedOutputs::compute(&result, false);
        assert!(outputs.group_deploy_decisions.is_empty());
        assert!(!outputs.has_deployable_groups());
    }

    #[test]
    fn test_deploy_decisions_both_new_and_failed() {
        let interner = StringInterner::new();
        let mixed_key = interner.intern("mixed");

        let result = ProcessedResult {
            all_files: vec![
                make_file(ChangeType::Modified, 0),
                make_file(ChangeType::Modified, 1),
            ],
            filtered_indices: vec![0, 1],
            unmatched_indices: Vec::new(),
            pattern_applied: true,
            group_results: vec![GroupResult {
                key: mixed_key,
                matched_indices: vec![0, 1],
            }],
            additions: 0,
            deletions: 0,
            diagnostics: Vec::new(),
            workflow_result: None,
            ci_decision: Some(CiDecision {
                files_to_rebuild: vec![InternedString(0), InternedString(1)],
                files_to_skip: Vec::new(),
                failed_jobs: Vec::new(),
                successful_jobs: Vec::new(),
                rebuild_reasons: vec![
                    RebuildReason {
                        file: InternedString(0),
                        kind: RebuildReasonKind::NewChange,
                        failed_run_id: None,
                        failed_job_name: None,
                    },
                    RebuildReason {
                        file: InternedString(1),
                        kind: RebuildReasonKind::PreviousFailure,
                        failed_run_id: Some(200),
                        failed_job_name: None,
                    },
                ],
            }),
        };

        let outputs = ComputedOutputs::compute(&result, false);
        assert_eq!(outputs.group_deploy_decisions.len(), 1);
        assert_eq!(
            outputs.group_deploy_decisions[0].action,
            GroupDeployAction::Deploy
        );
        assert_eq!(
            outputs.group_deploy_decisions[0].reason,
            Some(GroupDeployReason::BothNewAndFailed)
        );
        assert_eq!(outputs.group_deploy_decisions[0].files_to_rebuild.len(), 2);
    }

    #[test]
    fn test_deploy_decisions_all_skip() {
        let interner = StringInterner::new();
        let staging_key = interner.intern("staging");

        let result = ProcessedResult {
            all_files: vec![make_file(ChangeType::Modified, 0)],
            filtered_indices: vec![0],
            unmatched_indices: Vec::new(),
            pattern_applied: true,
            group_results: vec![GroupResult {
                key: staging_key,
                matched_indices: vec![0],
            }],
            additions: 0,
            deletions: 0,
            diagnostics: Vec::new(),
            workflow_result: None,
            ci_decision: Some(CiDecision {
                files_to_rebuild: Vec::new(),
                files_to_skip: vec![InternedString(0)],
                failed_jobs: Vec::new(),
                successful_jobs: Vec::new(),
                rebuild_reasons: Vec::new(),
            }),
        };

        let outputs = ComputedOutputs::compute(&result, false);
        assert_eq!(outputs.group_deploy_decisions.len(), 1);
        assert_eq!(
            outputs.group_deploy_decisions[0].action,
            GroupDeployAction::Skip
        );
        assert!(outputs.group_deploy_decisions[0].reason.is_none());
        assert!(!outputs.has_deployable_groups());
    }
}
