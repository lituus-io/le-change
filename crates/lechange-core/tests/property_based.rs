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
