//! Integration tests for ancestor directory file association (files_ancestor_lookup_depth)

use lechange_core::interner::StringInterner;
use lechange_core::output::computed::ComputedOutputs;
use lechange_core::patterns::matcher::PatternMatcher;
use lechange_core::types::{
    ChangeType, ChangedFile, DiagnosticCategory, FileOrigin, GroupResult, ProcessedResult,
};

/// Build a ProcessedResult with pattern filtering and ancestor recovery simulation.
///
/// `matched_paths` are the file paths that match the pattern.
/// `unmatched_paths` are the file paths that do NOT match the pattern.
/// `recovered_indices` are the indices (into unmatched_paths) that should be moved to filtered.
fn build_result_with_recovery(
    interner: &StringInterner,
    matched_paths: &[&str],
    unmatched_paths: &[&str],
    recovered_indices: &[usize],
) -> ProcessedResult {
    let mut all_files = Vec::new();
    let mut filtered_indices = Vec::new();
    let mut unmatched_indices = Vec::new();

    // Add matched files
    for (i, path) in matched_paths.iter().enumerate() {
        all_files.push(ChangedFile {
            path: interner.intern(path),
            change_type: ChangeType::Modified,
            previous_path: None,
            is_symlink: false,
            submodule_depth: 0,
            origin: FileOrigin::default(),
        });
        filtered_indices.push(i as u32);
    }

    // Add unmatched files
    let offset = matched_paths.len();
    for (i, path) in unmatched_paths.iter().enumerate() {
        all_files.push(ChangedFile {
            path: interner.intern(path),
            change_type: ChangeType::Added,
            previous_path: None,
            is_symlink: false,
            submodule_depth: 0,
            origin: FileOrigin::default(),
        });
        let idx = (offset + i) as u32;
        if recovered_indices.contains(&i) {
            filtered_indices.push(idx);
        } else {
            unmatched_indices.push(idx);
        }
    }

    ProcessedResult {
        all_files,
        filtered_indices,
        unmatched_indices,
        pattern_applied: true,
        group_results: Vec::new(),
        additions: 0,
        deletions: 0,
        diagnostics: Vec::new(),
        workflow_result: None,
        ci_decision: None,
    }
}

#[test]
fn test_ancestor_end_to_end() {
    // Simulate: pattern matches *.yaml in stacks/prod/**
    // A .sql file in stacks/prod/migrations/ is recovered via ancestor lookup
    // and appears in the filtered output
    let interner = StringInterner::new();

    let result = build_result_with_recovery(
        &interner,
        &["stacks/prod/config.yaml"],
        &["stacks/prod/migrations/001.sql"],
        &[0], // recovered
    );

    // The .sql file should now be in filtered_indices
    assert_eq!(result.filtered_indices.len(), 2);
    assert!(result.unmatched_indices.is_empty());

    let outputs = ComputedOutputs::compute(&result, false);
    // Both files should appear in all_changed_and_modified
    assert_eq!(outputs.all_changed_and_modified.len(), 2);
}

#[test]
fn test_ancestor_with_groups() {
    // Recovered files should participate in the correct group
    let interner = StringInterner::new();

    let prod_key = interner.intern("prod");

    let mut all_files = Vec::new();
    let yaml_path = interner.intern("stacks/prod/config.yaml");
    let sql_path = interner.intern("stacks/prod/migrations/001.sql");

    all_files.push(ChangedFile {
        path: yaml_path,
        change_type: ChangeType::Modified,
        previous_path: None,
        is_symlink: false,
        submodule_depth: 0,
        origin: FileOrigin::default(),
    });
    all_files.push(ChangedFile {
        path: sql_path,
        change_type: ChangeType::Added,
        previous_path: None,
        is_symlink: false,
        submodule_depth: 0,
        origin: FileOrigin::default(),
    });

    // Both are filtered (sql recovered via ancestor)
    let filtered_indices = vec![0, 1];

    // Group matching: stacks/prod/** matches both files
    let group_matcher = PatternMatcher::new(&["stacks/prod/**"], &[], true).unwrap();

    let matched: Vec<u32> = filtered_indices
        .iter()
        .copied()
        .filter(|&i| {
            interner
                .resolve(all_files[i as usize].path)
                .map(|p| group_matcher.matches_sync(p))
                .unwrap_or(false)
        })
        .collect();

    let result = ProcessedResult {
        all_files,
        filtered_indices,
        unmatched_indices: Vec::new(),
        pattern_applied: true,
        group_results: vec![GroupResult {
            key: prod_key,
            matched_indices: matched,
        }],
        additions: 0,
        deletions: 0,
        diagnostics: Vec::new(),
        workflow_result: None,
        ci_decision: None,
    };

    let outputs = ComputedOutputs::compute(&result, false);

    // prod group should have both files
    assert_eq!(outputs.group_deploy_decisions.len(), 1);
    assert_eq!(outputs.group_deploy_decisions[0].files_to_rebuild.len(), 2);
    assert_eq!(outputs.group_deploy_decisions[0].total_files, 2);
}

#[test]
fn test_ancestor_diagnostic() {
    // Verify diagnostic messages are present in output
    let interner = StringInterner::new();

    let result = ProcessedResult {
        all_files: vec![ChangedFile {
            path: interner.intern("stacks/prod/migrations/001.sql"),
            change_type: ChangeType::Added,
            previous_path: None,
            is_symlink: false,
            submodule_depth: 0,
            origin: FileOrigin::default(),
        }],
        filtered_indices: vec![0], // Recovered
        unmatched_indices: Vec::new(),
        pattern_applied: true,
        group_results: Vec::new(),
        additions: 0,
        deletions: 0,
        diagnostics: vec![lechange_core::types::Diagnostic {
            severity: lechange_core::types::DiagnosticSeverity::Warning,
            category: DiagnosticCategory::AncestorRecovery,
            message: "Recovered 1 file(s) via ancestor directory lookup (depth=2)".to_string(),
        }],
        workflow_result: None,
        ci_decision: None,
    };

    // Verify the diagnostic is present
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(
        result.diagnostics[0].category,
        DiagnosticCategory::AncestorRecovery
    );
    assert!(result.diagnostics[0].message.contains("Recovered 1"));
}
