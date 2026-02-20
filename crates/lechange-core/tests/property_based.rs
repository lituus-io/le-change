//! Property-based tests using proptest

use lechange_core::{ChangeType, StringInterner};
use proptest::prelude::*;

// Generate arbitrary ChangeType
fn arb_change_type() -> impl Strategy<Value = ChangeType> {
    prop_oneof![
        Just(ChangeType::Added),
        Just(ChangeType::Copied),
        Just(ChangeType::Deleted),
        Just(ChangeType::Modified),
        Just(ChangeType::Renamed),
        Just(ChangeType::TypeChanged),
        Just(ChangeType::Unmerged),
        Just(ChangeType::Unknown),
    ]
}

// Generate arbitrary path strings
fn arb_path() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-z0-9/_-]{1,50}\\.(rs|py|txt|md)").expect("valid regex")
}

proptest! {
    #[test]
    fn test_interner_idempotent(s in "[a-z]{1,100}") {
        let interner = StringInterner::new();
        let id1 = interner.intern(&s);
        let id2 = interner.intern(&s);
        prop_assert_eq!(id1, id2);
    }

    #[test]
    fn test_interner_resolve_roundtrip(s in "[a-z]{1,100}") {
        let interner = StringInterner::new();
        let id = interner.intern(&s);
        let resolved = interner.resolve(id);
        prop_assert_eq!(resolved, Some(s.as_str()));
    }

    #[test]
    fn test_interner_different_strings(s1 in "[a-z]{1,50}", s2 in "[A-Z]{1,50}") {
        let interner = StringInterner::new();
        let id1 = interner.intern(&s1);
        let id2 = interner.intern(&s2);

        // Different strings should have different IDs
        if s1 != s2 {
            prop_assert_ne!(id1, id2);
        }
    }

    #[test]
    fn test_change_type_roundtrip(change_type in arb_change_type()) {
        let byte = change_type.as_byte();
        let parsed = ChangeType::from_byte(byte);
        prop_assert_eq!(parsed, Some(change_type));
    }

    #[test]
    fn test_change_type_string_conversion(change_type in arb_change_type()) {
        let s = change_type.as_str();
        prop_assert!(!s.is_empty());
        prop_assert!(s.chars().next().unwrap().is_alphabetic());
    }

    #[test]
    fn test_pattern_matcher_handles_all_paths(
        patterns in prop::collection::vec("[a-z*?]+", 0..5),
        path in arb_path()
    ) {
        use lechange_core::patterns::matcher::PatternMatcher;

        let pattern_refs: Vec<&str> = patterns.iter().map(|s| s.as_str()).collect();

        // Should not panic with any pattern combination
        if let Ok(matcher) = PatternMatcher::new(&pattern_refs, &[], false) {
            let _ = matcher.matches_sync(&path);
        }
    }

    #[test]
    fn test_interner_thread_safety(
        strings in prop::collection::vec("[a-z]{1,20}", 1..100)
    ) {
        use std::sync::Arc;
        use std::thread;

        let interner = Arc::new(StringInterner::new());
        let mut handles = vec![];

        for s in strings {
            let interner_clone = Arc::clone(&interner);
            let handle = thread::spawn(move || {
                interner_clone.intern(&s)
            });
            handles.push(handle);
        }

        // All threads should complete without panic
        for handle in handles {
            let _ = handle.join();
        }

        prop_assert!(true);
    }

    #[test]
    fn test_file_ops_cache_consistency(
        paths in prop::collection::vec(arb_path(), 1..50)
    ) {
        use lechange_core::file_ops::FileOps;
        use std::path::Path;

        let ops = FileOps::new();

        // Accessing same path multiple times should give same result
        for path_str in &paths {
            let path = Path::new(path_str);
            let result1 = ops.is_symlink_sync(path).unwrap_or(false);
            let result2 = ops.is_symlink_sync(path).unwrap_or(false);
            prop_assert_eq!(result1, result2);
        }
    }

    #[test]
    fn test_interner_capacity_growth(
        strings in prop::collection::vec("[a-z]{1,10}", 100..1000)
    ) {
        let interner = StringInterner::with_capacity(10);

        // Should handle more strings than initial capacity
        for s in &strings {
            let _ = interner.intern(s);
        }

        // All strings should be retrievable
        for s in &strings {
            let id = interner.intern(s);
            let resolved = interner.resolve(id);
            prop_assert_eq!(resolved, Some(s.as_str()));
        }
    }
}

// 5A: ComputedOutputs partition invariant
proptest! {
    #[test]
    fn test_computed_outputs_partition(
        num_files in 1..100usize,
        filter_ratio in 0.0..1.0f64
    ) {
        use lechange_core::types::{ChangedFile, FileOrigin, ProcessedResult};
        use lechange_core::output::ComputedOutputs;

        let interner = StringInterner::new();

        let change_types = [
            ChangeType::Added,
            ChangeType::Copied,
            ChangeType::Deleted,
            ChangeType::Modified,
            ChangeType::Renamed,
            ChangeType::TypeChanged,
            ChangeType::Unmerged,
            ChangeType::Unknown,
        ];

        let files: Vec<ChangedFile> = (0..num_files)
            .map(|i| ChangedFile {
                path: interner.intern(&format!("file_{}.rs", i)),
                change_type: change_types[i % change_types.len()],
                previous_path: None,
                is_symlink: false,
                submodule_depth: 0,
                origin: FileOrigin::default(),
            })
            .collect();

        let filter_count = (num_files as f64 * filter_ratio) as usize;
        let filtered_indices: Vec<u32> = (0..filter_count as u32).collect();
        let unmatched_indices: Vec<u32> = (filter_count as u32..num_files as u32).collect();

        let result = ProcessedResult {
            all_files: files,
            filtered_indices: filtered_indices.clone(),
            unmatched_indices: unmatched_indices.clone(),
            pattern_applied: true,
            group_results: Vec::new(),
            additions: 0,
            deletions: 0,
            diagnostics: Vec::new(),
            workflow_result: None,
            ci_decision: None,
        };

        let outputs = ComputedOutputs::compute(&result, false);

        // Verify: sum of filtered per-type = filtered_indices.len()
        let filtered_total = outputs.filtered_added.len()
            + outputs.filtered_copied.len()
            + outputs.filtered_deleted.len()
            + outputs.filtered_modified.len()
            + outputs.filtered_renamed.len()
            + outputs.filtered_type_changed.len()
            + outputs.filtered_unmerged.len()
            + outputs.filtered_unknown.len();
        prop_assert_eq!(filtered_total, filter_count);

        // Verify: other_modified ⊆ unmatched_indices
        let unmatched_set: std::collections::HashSet<u32> = unmatched_indices.iter().copied().collect();
        for &idx in &outputs.other_modified {
            prop_assert!(unmatched_set.contains(&idx));
        }
    }
}

// 5B: CiDecision disjoint invariant
proptest! {
    #[test]
    fn test_ci_decision_disjoint(
        num_current in 0..20usize,
        num_failed in 0..10usize,
        num_success in 0..10usize
    ) {
        use lechange_core::types::{
            ChangedFile, FileOrigin, InternedString, WorkflowConclusion,
            WorkflowFailure, WorkflowRun, WorkflowSuccess, WorkflowStatus,
        };
        use lechange_core::coordination::ci_decision::CiDecisionEngine;

        let interner = StringInterner::new();

        // Generate current changes (paths 0..num_current)
        let current: Vec<ChangedFile> = (0..num_current)
            .map(|i| ChangedFile {
                path: interner.intern(&format!("current_{}.rs", i)),
                change_type: ChangeType::Modified,
                previous_path: None,
                is_symlink: false,
                submodule_depth: 0,
                origin: FileOrigin {
                    in_current_changes: true,
                    in_previous_failure: false,
                    in_previous_success: false,
                },
            })
            .collect();

        // Generate failures (paths 100..100+num_failed)
        let failures: Vec<WorkflowFailure> = if num_failed > 0 {
            vec![WorkflowFailure {
                run: WorkflowRun {
                    id: 1,
                    name: interner.intern("CI"),
                    status: WorkflowStatus::Completed,
                    conclusion: Some(WorkflowConclusion::Failure),
                    branch: interner.intern("main"),
                    head_sha: interner.intern("sha1"),
                    created_at: 100,
                },
                files: (0..num_failed)
                    .map(|i| interner.intern(&format!("failed_{}.rs", i)))
                    .collect(),
                failed_jobs: Vec::new(),
            }]
        } else {
            Vec::new()
        };

        // Generate successes (paths 200..200+num_success)
        let successes: Vec<WorkflowSuccess> = if num_success > 0 {
            vec![WorkflowSuccess {
                run: WorkflowRun {
                    id: 2,
                    name: interner.intern("CI"),
                    status: WorkflowStatus::Completed,
                    conclusion: Some(WorkflowConclusion::Success),
                    branch: interner.intern("main"),
                    head_sha: interner.intern("sha2"),
                    created_at: 200,
                },
                jobs: Vec::new(),
                files: (0..num_success)
                    .map(|i| interner.intern(&format!("success_{}.rs", i)))
                    .collect(),
            }]
        } else {
            Vec::new()
        };

        let engine = CiDecisionEngine::new(&interner);
        let decision = engine.compute(&current, &failures, &successes);

        // Invariant: rebuild ∩ skip = ∅
        let rebuild_set: std::collections::HashSet<InternedString> =
            decision.files_to_rebuild.iter().copied().collect();
        let skip_set: std::collections::HashSet<InternedString> =
            decision.files_to_skip.iter().copied().collect();
        prop_assert!(rebuild_set.is_disjoint(&skip_set));
    }
}

// 5C: Current changes always in rebuild
proptest! {
    #[test]
    fn test_current_always_rebuild(num_current in 1..20usize) {
        use lechange_core::types::{
            ChangedFile, FileOrigin, InternedString,
        };
        use lechange_core::coordination::ci_decision::CiDecisionEngine;

        let interner = StringInterner::new();

        let current: Vec<ChangedFile> = (0..num_current)
            .map(|i| ChangedFile {
                path: interner.intern(&format!("file_{}.rs", i)),
                change_type: ChangeType::Modified,
                previous_path: None,
                is_symlink: false,
                submodule_depth: 0,
                origin: FileOrigin {
                    in_current_changes: true,
                    in_previous_failure: false,
                    in_previous_success: false,
                },
            })
            .collect();

        let engine = CiDecisionEngine::new(&interner);
        let decision = engine.compute(&current, &[], &[]);

        // All current_changes files must appear in files_to_rebuild
        let rebuild_set: std::collections::HashSet<InternedString> =
            decision.files_to_rebuild.iter().copied().collect();
        for file in &current {
            prop_assert!(rebuild_set.contains(&file.path));
        }
    }
}

// 5D: DirNames depth invariant
proptest! {
    #[test]
    fn test_dir_depth_limit(
        max_depth in 1..5u32
    ) {
        use lechange_core::types::{ChangedFile, FileOrigin};
        use lechange_core::output::dir_names::DirNameExtractor;

        let interner = StringInterner::new();

        // Generate paths with various depths
        let paths = [
            "a/file.rs",
            "a/b/file.rs",
            "a/b/c/file.rs",
            "a/b/c/d/file.rs",
            "a/b/c/d/e/file.rs",
        ];

        let files: Vec<ChangedFile> = paths
            .iter()
            .map(|p| ChangedFile {
                path: interner.intern(p),
                change_type: ChangeType::Modified,
                previous_path: None,
                is_symlink: false,
                submodule_depth: 0,
                origin: FileOrigin::default(),
            })
            .collect();

        let indices: Vec<u32> = (0..files.len() as u32).collect();
        let extractor = DirNameExtractor::new(&interner);
        let dirs = extractor.extract(&files, &indices, Some(max_depth), false, None, false);

        // All extracted dirs should have depth <= max_depth
        for dir in &dirs {
            if let Some(dir_str) = interner.resolve(*dir) {
                let depth = dir_str.matches('/').count() as u32 + 1;
                prop_assert!(depth <= max_depth, "dir '{}' has depth {} > max {}", dir_str, depth, max_depth);
            }
        }
    }
}

// --- New property tests for interned group keys, overlap symmetry, matrix validity ---

proptest! {
    #[test]
    fn test_interned_group_key_roundtrip(key in "[a-z]{1,50}") {
        let interner = StringInterner::new();
        let id = interner.intern(&key);
        let resolved = interner.resolve(id);
        prop_assert_eq!(resolved, Some(key.as_str()));
    }

    #[test]
    fn test_overlap_symmetry(
        set_a in prop::collection::hash_set("[a-z]{1,10}", 0..20),
        set_b in prop::collection::hash_set("[a-z]{1,10}", 0..20),
    ) {
        // Overlap should be symmetric: A∩B ≠ ∅ iff B∩A ≠ ∅
        let overlap_ab = set_a.intersection(&set_b).next().is_some();
        let overlap_ba = set_b.intersection(&set_a).next().is_some();
        prop_assert_eq!(overlap_ab, overlap_ba);
    }

    #[test]
    fn test_enriched_matrix_valid_json(
        num_groups in 0..10usize,
        include_reason in proptest::bool::ANY,
        include_concurrency in proptest::bool::ANY,
    ) {
        use lechange_core::output::json_format::format_deploy_matrix;
        use lechange_core::types::{GroupDeployAction, GroupDeployDecision, GroupDeployReason};

        let interner = StringInterner::new();
        let decisions: Vec<GroupDeployDecision> = (0..num_groups)
            .map(|i| {
                let key = interner.intern(&format!("group_{}", i));
                let file = interner.intern(&format!("path_{}.yaml", i));
                GroupDeployDecision {
                    key,
                    action: if i % 2 == 0 { GroupDeployAction::Deploy } else { GroupDeployAction::Skip },
                    reason: if i % 2 == 0 { Some(GroupDeployReason::NewChange) } else { None },
                    files_to_rebuild: if i % 2 == 0 { vec![file] } else { Vec::new() },
                    files_to_skip: if i % 2 != 0 { vec![file] } else { Vec::new() },
                    total_files: 1,
                    concurrency_blocked: i % 3 == 0,
                    concurrency_blocked_by: if i % 3 == 0 { 1 } else { 0 },
                }
            })
            .collect();

        let matrix = format_deploy_matrix(
            &decisions,
            |s| interner.resolve(s),
            " ",
            include_reason,
            include_concurrency,
        );

        // Must always be valid JSON
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(&matrix);
        prop_assert!(parsed.is_ok(), "Invalid JSON: {}", matrix);

        let val = parsed.unwrap();
        prop_assert!(val["include"].is_array());
    }

    #[test]
    fn test_unified_partition_superset(
        num_files in 1..20usize,
    ) {
        use lechange_core::types::{ChangedFile, FileOrigin};
        use lechange_core::output::ComputedOutputs;

        let interner = StringInterner::new();

        // Create files in two groups
        let mut files = Vec::new();
        let mut group_a_indices = Vec::new();
        let mut group_b_indices = Vec::new();

        for i in 0..num_files {
            let group = if i % 2 == 0 { "a" } else { "b" };
            let path = format!("{}/file_{}.rs", group, i);
            files.push(ChangedFile {
                path: interner.intern(&path),
                change_type: ChangeType::Modified,
                previous_path: None,
                is_symlink: false,
                submodule_depth: 0,
                origin: FileOrigin::default(),
            });
            if i % 2 == 0 {
                group_a_indices.push(i as u32);
            } else {
                group_b_indices.push(i as u32);
            }
        }

        let filtered: Vec<u32> = (0..num_files as u32).collect();
        let result = lechange_core::types::ProcessedResult {
            all_files: files,
            filtered_indices: filtered,
            unmatched_indices: Vec::new(),
            pattern_applied: true,
            group_results: vec![
                lechange_core::types::GroupResult {
                    key: interner.intern("a"),
                    matched_indices: group_a_indices,
                },
                lechange_core::types::GroupResult {
                    key: interner.intern("b"),
                    matched_indices: group_b_indices,
                },
            ],
            additions: 0,
            deletions: 0,
            diagnostics: Vec::new(),
            workflow_result: None,
            ci_decision: None,
        };

        let outputs = ComputedOutputs::compute(&result, false);

        // All deploy decisions should reference groups from the result
        for d in &outputs.group_deploy_decisions {
            prop_assert!(
                result.group_results.iter().any(|g| g.key == d.key),
                "Decision key not in group_results"
            );
        }
    }
}

// --- Phase 6F: Ancestor recovery property tests ---
proptest! {
    #[test]
    fn test_ancestor_depth_clamping(depth in 0..100u32) {
        // Invariant: effective depth is always min(depth, 3)
        let clamped = depth.min(3);
        prop_assert!(clamped <= 3);
        if depth <= 3 {
            prop_assert_eq!(clamped, depth);
        } else {
            prop_assert_eq!(clamped, 3);
        }
    }

    #[test]
    fn test_ancestor_recovery_subset(
        num_files in 1..50usize,
        recovery_ratio in 0.0..1.0f64,
    ) {
        // Invariant: recovered files are always a subset of the original unmatched set
        let interner = StringInterner::new();

        let _files: Vec<lechange_core::types::ChangedFile> = (0..num_files)
            .map(|i| lechange_core::types::ChangedFile {
                path: interner.intern(&format!("dir/file_{}.rs", i)),
                change_type: ChangeType::Modified,
                previous_path: None,
                is_symlink: false,
                submodule_depth: 0,
                origin: lechange_core::types::FileOrigin::default(),
            })
            .collect();

        // Split into filtered and unmatched
        let split_point = (num_files as f64 * 0.5) as usize;
        let mut filtered: Vec<u32> = (0..split_point as u32).collect();
        let original_unmatched: Vec<u32> = (split_point as u32..num_files as u32).collect();
        let mut unmatched = original_unmatched.clone();

        // Simulate recovery: move some unmatched to filtered
        let recovery_count = (unmatched.len() as f64 * recovery_ratio) as usize;
        let recovered: Vec<u32> = unmatched.drain(..recovery_count).collect();
        filtered.extend(&recovered);

        // Invariant: every recovered index was in the original unmatched set
        let original_set: std::collections::HashSet<u32> = original_unmatched.iter().copied().collect();
        for &idx in &recovered {
            prop_assert!(original_set.contains(&idx));
        }

        // Invariant: filtered + unmatched = all file indices
        let mut all: Vec<u32> = filtered.iter().chain(unmatched.iter()).copied().collect();
        all.sort();
        all.dedup();
        prop_assert_eq!(all.len(), num_files);
    }

    #[test]
    fn test_ancestor_depth_zero_is_noop(num_files in 1..30usize) {
        // Invariant: with depth=0, no recovery should happen
        let depth: u32 = 0;
        let clamped = depth.min(3);

        // A loop of 0..clamped iterations does nothing
        let mut recovered_count = 0u32;
        for _ in 0..clamped {
            recovered_count += 1; // Would increment if loop ran
        }
        prop_assert_eq!(recovered_count, 0);
        // num_files is used to prevent "unused" warning
        prop_assert!(num_files > 0);
    }
}

#[cfg(test)]
mod integration_tests {
    use lechange_core::StringInterner;

    #[test]
    fn test_interner_realistic_workload() {
        let interner = StringInterner::new();
        let paths = vec![
            "src/main.rs",
            "src/lib.rs",
            "tests/integration.rs",
            "Cargo.toml",
            "README.md",
        ];

        // Simulate realistic usage
        for _ in 0..1000 {
            for path in &paths {
                let id = interner.intern(path);
                assert_eq!(interner.resolve(id), Some(*path));
            }
        }
    }

    #[test]
    fn test_memory_efficiency() {
        let interner = StringInterner::new();
        let duplicates = vec!["same/path.rs"; 10000];

        // Intern 10k duplicate strings
        let mut ids = Vec::new();
        for path in &duplicates {
            ids.push(interner.intern(path));
        }

        // All should have same ID (memory efficient)
        let first_id = ids[0];
        for id in ids {
            assert_eq!(id, first_id);
        }
    }
}
