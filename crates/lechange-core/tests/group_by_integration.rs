//! Integration tests for files_group_by template discovery and enriched deploy matrix

use lechange_core::interner::StringInterner;
use lechange_core::output::computed::ComputedOutputs;
use lechange_core::output::json_format::format_deploy_matrix;
use lechange_core::patterns::loader::PatternLoader;
use lechange_core::types::{
    ChangeType, ChangedFile, FileOrigin, GroupByKey, GroupDeployAction, GroupResult,
    InternedString, ProcessedResult,
};
use std::collections::HashMap;

/// Helper: build a ProcessedResult from files matched against discovered groups
fn build_result_from_groups(
    interner: &StringInterner,
    file_paths: &[&str],
    groups: &[lechange_core::patterns::loader::PatternGroup],
) -> ProcessedResult {
    let files: Vec<ChangedFile> = file_paths
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

    let filtered_indices: Vec<u32> = (0..files.len() as u32).collect();

    let group_results: Vec<GroupResult> = groups
        .iter()
        .map(|group| {
            let matched: Vec<u32> = filtered_indices
                .iter()
                .copied()
                .filter(|&i| {
                    interner
                        .resolve(files[i as usize].path)
                        .map(|path| group.matcher.matches_sync(path))
                        .unwrap_or(false)
                })
                .collect();
            GroupResult {
                key: interner.intern(&group.name),
                matched_indices: matched,
            }
        })
        .collect();

    ProcessedResult {
        all_files: files,
        filtered_indices,
        unmatched_indices: Vec::new(),
        pattern_applied: true,
        group_results,
        additions: 0,
        deletions: 0,
        diagnostics: Vec::new(),
        workflow_result: None,
        ci_decision: None,
    }
}

// --- files_group_by end-to-end ---

#[test]
fn test_files_group_by_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("stacks/dev")).unwrap();
    std::fs::create_dir_all(dir.path().join("stacks/prod")).unwrap();

    let template = PatternLoader::parse_group_by_template("stacks/{group}/**").unwrap();
    let groups =
        PatternLoader::discover_groups_from_template(&template, dir.path(), true, GroupByKey::Name)
            .unwrap();

    assert_eq!(groups.len(), 2);

    let interner = StringInterner::new();
    let result = build_result_from_groups(
        &interner,
        &["stacks/dev/config.yaml", "stacks/prod/config.yaml"],
        &groups,
    );

    let outputs = ComputedOutputs::compute(&result, false);

    // Both groups should have changes
    assert_eq!(outputs.changed_keys.len(), 2);

    // Verify group names
    let key_names: Vec<&str> = outputs
        .changed_keys
        .iter()
        .filter_map(|k| interner.resolve(*k))
        .collect();
    assert!(key_names.contains(&"dev"));
    assert!(key_names.contains(&"prod"));
}

#[test]
fn test_files_group_by_with_key_path() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("stacks/dev")).unwrap();
    std::fs::create_dir_all(dir.path().join("stacks/prod")).unwrap();

    let template = PatternLoader::parse_group_by_template("stacks/{group}/**").unwrap();
    let groups =
        PatternLoader::discover_groups_from_template(&template, dir.path(), true, GroupByKey::Path)
            .unwrap();

    let interner = StringInterner::new();
    let result = build_result_from_groups(
        &interner,
        &["stacks/dev/config.yaml", "stacks/prod/config.yaml"],
        &groups,
    );

    let outputs = ComputedOutputs::compute(&result, false);

    let key_names: Vec<&str> = outputs
        .changed_keys
        .iter()
        .filter_map(|k| interner.resolve(*k))
        .collect();
    assert!(key_names.contains(&"stacks/dev"));
    assert!(key_names.contains(&"stacks/prod"));
}

#[test]
fn test_files_group_by_with_key_hash() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("stacks/dev")).unwrap();
    std::fs::create_dir_all(dir.path().join("stacks/prod")).unwrap();

    let template = PatternLoader::parse_group_by_template("stacks/{group}/**").unwrap();
    let groups =
        PatternLoader::discover_groups_from_template(&template, dir.path(), true, GroupByKey::Hash)
            .unwrap();

    let interner = StringInterner::new();
    let result = build_result_from_groups(
        &interner,
        &["stacks/dev/config.yaml", "stacks/prod/config.yaml"],
        &groups,
    );

    let outputs = ComputedOutputs::compute(&result, false);

    // All keys should be 8-char hex hashes
    for key in &outputs.changed_keys {
        let key_str = interner.resolve(*key).unwrap();
        assert_eq!(key_str.len(), 8);
        assert!(key_str.chars().all(|c| c.is_ascii_hexdigit()));
    }
}

#[test]
fn test_files_yaml_takes_precedence() {
    // When both files_yaml and files_group_by are set, YAML should win.
    // This is a unit-level check of the logic — processor integration would
    // require a full git repo, but we verify the pattern groups directly.
    let yaml = r#"
frontend:
  - "src/components/**"
backend:
  - "src/api/**"
"#;
    let yaml_groups = PatternLoader::load_yaml_groups(yaml, true).unwrap();
    assert_eq!(yaml_groups.len(), 2);

    // Verify the YAML groups are functional
    let frontend = yaml_groups.iter().find(|g| g.name == "frontend").unwrap();
    assert!(frontend.matcher.matches_sync("src/components/Button.tsx"));
    assert!(!frontend.matcher.matches_sync("src/api/handler.ts"));
}

// --- Enriched matrix end-to-end ---

#[test]
fn test_enriched_matrix_end_to_end() {
    let interner = StringInterner::new();

    let prod_key = interner.intern("prod");
    let dev_key = interner.intern("dev");
    let file1 = interner.intern("stacks/prod/config.yaml");
    let file2 = interner.intern("stacks/dev/config.yaml");

    // Simulate blocked_groups: prod is blocked by run 42
    let mut blocked_groups: HashMap<InternedString, Vec<u64>> = HashMap::new();
    blocked_groups.insert(prod_key, vec![42]);

    let files = vec![
        ChangedFile {
            path: file1,
            change_type: ChangeType::Modified,
            previous_path: None,
            is_symlink: false,
            submodule_depth: 0,
            origin: FileOrigin::default(),
        },
        ChangedFile {
            path: file2,
            change_type: ChangeType::Modified,
            previous_path: None,
            is_symlink: false,
            submodule_depth: 0,
            origin: FileOrigin::default(),
        },
    ];

    let result = ProcessedResult {
        all_files: files,
        filtered_indices: vec![0, 1],
        unmatched_indices: Vec::new(),
        pattern_applied: true,
        group_results: vec![
            GroupResult {
                key: prod_key,
                matched_indices: vec![0],
            },
            GroupResult {
                key: dev_key,
                matched_indices: vec![1],
            },
        ],
        additions: 0,
        deletions: 0,
        diagnostics: Vec::new(),
        workflow_result: None,
        ci_decision: None,
    };

    let outputs = ComputedOutputs::compute_with_concurrency(
        &result,
        false,
        Some(&blocked_groups),
        Some(&interner),
    );

    // Verify deploy decisions have concurrency info
    assert_eq!(outputs.group_deploy_decisions.len(), 2);

    let prod_decision = outputs
        .group_deploy_decisions
        .iter()
        .find(|d| d.key == prod_key)
        .unwrap();
    assert!(prod_decision.concurrency_blocked);
    assert_eq!(prod_decision.concurrency_blocked_by, 1);

    let dev_decision = outputs
        .group_deploy_decisions
        .iter()
        .find(|d| d.key == dev_key)
        .unwrap();
    assert!(!dev_decision.concurrency_blocked);
    assert_eq!(dev_decision.concurrency_blocked_by, 0);

    // Format as matrix with all enrichment flags
    let matrix = format_deploy_matrix(
        &outputs.group_deploy_decisions,
        |s| interner.resolve(s),
        " ",
        true,
        true,
    );

    // Parse and validate
    let parsed: serde_json::Value = serde_json::from_str(&matrix).unwrap();
    let include = parsed["include"].as_array().unwrap();
    assert_eq!(include.len(), 2);

    // Find prod entry
    let prod_entry = include
        .iter()
        .find(|e| e["stack"].as_str() == Some("prod"))
        .unwrap();
    assert_eq!(prod_entry["action"].as_str(), Some("deploy"));
    assert_eq!(prod_entry["reason"].as_str(), Some("new_change"));
    assert_eq!(prod_entry["concurrency_blocked"].as_bool(), Some(true));
    assert_eq!(prod_entry["concurrency_blocked_by"].as_u64(), Some(1));

    // Find dev entry
    let dev_entry = include
        .iter()
        .find(|e| e["stack"].as_str() == Some("dev"))
        .unwrap();
    assert_eq!(dev_entry["action"].as_str(), Some("deploy"));
    assert_eq!(dev_entry["concurrency_blocked"].as_bool(), Some(false));
    assert_eq!(dev_entry["concurrency_blocked_by"].as_u64(), Some(0));
}

// --- Concurrent overlap scenario tests ---

#[test]
fn test_concurrent_same_group_different_files() {
    // Branch A modifies stacks/prod/config.yaml
    // Branch B modifies stacks/prod/networking.yaml
    // Both in same "prod" group → B should be blocked
    let interner = StringInterner::new();
    let prod_key = interner.intern("prod");

    // Simulate: B's compute sees prod blocked by A's run_id=100
    let mut blocked_groups: HashMap<InternedString, Vec<u64>> = HashMap::new();
    blocked_groups.insert(prod_key, vec![100]);

    let file_b = interner.intern("stacks/prod/networking.yaml");
    let files = vec![ChangedFile {
        path: file_b,
        change_type: ChangeType::Modified,
        previous_path: None,
        is_symlink: false,
        submodule_depth: 0,
        origin: FileOrigin::default(),
    }];

    let result = ProcessedResult {
        all_files: files,
        filtered_indices: vec![0],
        unmatched_indices: Vec::new(),
        pattern_applied: true,
        group_results: vec![GroupResult {
            key: prod_key,
            matched_indices: vec![0],
        }],
        additions: 0,
        deletions: 0,
        diagnostics: Vec::new(),
        workflow_result: None,
        ci_decision: None,
    };

    let outputs = ComputedOutputs::compute_with_concurrency(
        &result,
        false,
        Some(&blocked_groups),
        Some(&interner),
    );

    let prod = &outputs.group_deploy_decisions[0];
    assert_eq!(prod.action, GroupDeployAction::Deploy);
    assert!(prod.concurrency_blocked);
    assert_eq!(prod.concurrency_blocked_by, 1);
}

#[test]
fn test_concurrent_different_groups_parallel() {
    // Branch A: stacks/dev/config.yaml
    // Branch B: stacks/prod/config.yaml
    // Different groups → no blocking
    let interner = StringInterner::new();
    let dev_key = interner.intern("dev");
    let prod_key = interner.intern("prod");

    // No blocked_groups for B since dev and prod don't overlap
    let blocked_groups: HashMap<InternedString, Vec<u64>> = HashMap::new();

    let files = vec![
        ChangedFile {
            path: interner.intern("stacks/dev/config.yaml"),
            change_type: ChangeType::Modified,
            previous_path: None,
            is_symlink: false,
            submodule_depth: 0,
            origin: FileOrigin::default(),
        },
        ChangedFile {
            path: interner.intern("stacks/prod/config.yaml"),
            change_type: ChangeType::Modified,
            previous_path: None,
            is_symlink: false,
            submodule_depth: 0,
            origin: FileOrigin::default(),
        },
    ];

    let result = ProcessedResult {
        all_files: files,
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

    let outputs = ComputedOutputs::compute_with_concurrency(
        &result,
        false,
        Some(&blocked_groups),
        Some(&interner),
    );

    for d in &outputs.group_deploy_decisions {
        assert!(!d.concurrency_blocked);
        assert_eq!(d.concurrency_blocked_by, 0);
    }
}

#[test]
fn test_concurrent_partial_overlap() {
    // Branch A: stacks/dev + stacks/prod
    // Branch B: stacks/prod + stacks/staging
    // Overlap on prod only; staging not blocked
    let interner = StringInterner::new();
    let prod_key = interner.intern("prod");
    let staging_key = interner.intern("staging");

    let mut blocked_groups: HashMap<InternedString, Vec<u64>> = HashMap::new();
    blocked_groups.insert(prod_key, vec![200]); // prod blocked by A

    let files = vec![
        ChangedFile {
            path: interner.intern("stacks/prod/config.yaml"),
            change_type: ChangeType::Modified,
            previous_path: None,
            is_symlink: false,
            submodule_depth: 0,
            origin: FileOrigin::default(),
        },
        ChangedFile {
            path: interner.intern("stacks/staging/config.yaml"),
            change_type: ChangeType::Modified,
            previous_path: None,
            is_symlink: false,
            submodule_depth: 0,
            origin: FileOrigin::default(),
        },
    ];

    let result = ProcessedResult {
        all_files: files,
        filtered_indices: vec![0, 1],
        unmatched_indices: Vec::new(),
        pattern_applied: true,
        group_results: vec![
            GroupResult {
                key: prod_key,
                matched_indices: vec![0],
            },
            GroupResult {
                key: staging_key,
                matched_indices: vec![1],
            },
        ],
        additions: 0,
        deletions: 0,
        diagnostics: Vec::new(),
        workflow_result: None,
        ci_decision: None,
    };

    let outputs = ComputedOutputs::compute_with_concurrency(
        &result,
        false,
        Some(&blocked_groups),
        Some(&interner),
    );

    let prod = outputs
        .group_deploy_decisions
        .iter()
        .find(|d| d.key == prod_key)
        .unwrap();
    assert!(prod.concurrency_blocked);
    assert_eq!(prod.concurrency_blocked_by, 1);

    let staging = outputs
        .group_deploy_decisions
        .iter()
        .find(|d| d.key == staging_key)
        .unwrap();
    assert!(!staging.concurrency_blocked);
    assert_eq!(staging.concurrency_blocked_by, 0);
}

#[test]
fn test_concurrent_three_branches_cascade() {
    // A pushes prod, B pushes prod+dev, C pushes prod+staging
    // B and C both see prod blocked by A (run 1)
    let interner = StringInterner::new();
    let prod_key = interner.intern("prod");
    let dev_key = interner.intern("dev");

    // Simulate B's view: prod blocked by run 1
    let mut blocked_groups: HashMap<InternedString, Vec<u64>> = HashMap::new();
    blocked_groups.insert(prod_key, vec![1]);

    let files = vec![
        ChangedFile {
            path: interner.intern("stacks/prod/config.yaml"),
            change_type: ChangeType::Modified,
            previous_path: None,
            is_symlink: false,
            submodule_depth: 0,
            origin: FileOrigin::default(),
        },
        ChangedFile {
            path: interner.intern("stacks/dev/config.yaml"),
            change_type: ChangeType::Modified,
            previous_path: None,
            is_symlink: false,
            submodule_depth: 0,
            origin: FileOrigin::default(),
        },
    ];

    let result = ProcessedResult {
        all_files: files,
        filtered_indices: vec![0, 1],
        unmatched_indices: Vec::new(),
        pattern_applied: true,
        group_results: vec![
            GroupResult {
                key: prod_key,
                matched_indices: vec![0],
            },
            GroupResult {
                key: dev_key,
                matched_indices: vec![1],
            },
        ],
        additions: 0,
        deletions: 0,
        diagnostics: Vec::new(),
        workflow_result: None,
        ci_decision: None,
    };

    let outputs = ComputedOutputs::compute_with_concurrency(
        &result,
        false,
        Some(&blocked_groups),
        Some(&interner),
    );

    // prod blocked, dev not
    let prod = outputs
        .group_deploy_decisions
        .iter()
        .find(|d| d.key == prod_key)
        .unwrap();
    assert!(prod.concurrency_blocked);

    let dev = outputs
        .group_deploy_decisions
        .iter()
        .find(|d| d.key == dev_key)
        .unwrap();
    assert!(!dev.concurrency_blocked);
}

#[test]
fn test_concurrent_no_groups_no_blocking() {
    // No files_group_by → no blocked_groups
    let interner = StringInterner::new();

    let files = vec![ChangedFile {
        path: interner.intern("src/main.rs"),
        change_type: ChangeType::Modified,
        previous_path: None,
        is_symlink: false,
        submodule_depth: 0,
        origin: FileOrigin::default(),
    }];

    let result = ProcessedResult {
        all_files: files,
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

    // compute without concurrency (None)
    let outputs = ComputedOutputs::compute(&result, false);
    assert!(outputs.group_deploy_decisions.is_empty());
}

#[test]
fn test_concurrent_matrix_output_shows_blocked() {
    // Verify the matrix JSON correctly shows concurrency fields
    let interner = StringInterner::new();
    let prod_key = interner.intern("prod");
    let dev_key = interner.intern("dev");

    let mut blocked_groups: HashMap<InternedString, Vec<u64>> = HashMap::new();
    blocked_groups.insert(prod_key, vec![10, 20]); // blocked by 2 runs

    let files = vec![
        ChangedFile {
            path: interner.intern("stacks/prod/config.yaml"),
            change_type: ChangeType::Modified,
            previous_path: None,
            is_symlink: false,
            submodule_depth: 0,
            origin: FileOrigin::default(),
        },
        ChangedFile {
            path: interner.intern("stacks/dev/config.yaml"),
            change_type: ChangeType::Modified,
            previous_path: None,
            is_symlink: false,
            submodule_depth: 0,
            origin: FileOrigin::default(),
        },
    ];

    let result = ProcessedResult {
        all_files: files,
        filtered_indices: vec![0, 1],
        unmatched_indices: Vec::new(),
        pattern_applied: true,
        group_results: vec![
            GroupResult {
                key: prod_key,
                matched_indices: vec![0],
            },
            GroupResult {
                key: dev_key,
                matched_indices: vec![1],
            },
        ],
        additions: 0,
        deletions: 0,
        diagnostics: Vec::new(),
        workflow_result: None,
        ci_decision: None,
    };

    let outputs = ComputedOutputs::compute_with_concurrency(
        &result,
        false,
        Some(&blocked_groups),
        Some(&interner),
    );

    let matrix = format_deploy_matrix(
        &outputs.group_deploy_decisions,
        |s| interner.resolve(s),
        " ",
        false,
        true,
    );

    let parsed: serde_json::Value = serde_json::from_str(&matrix).unwrap();
    let include = parsed["include"].as_array().unwrap();

    let prod_entry = include
        .iter()
        .find(|e| e["stack"].as_str() == Some("prod"))
        .unwrap();
    assert_eq!(prod_entry["concurrency_blocked"].as_bool(), Some(true));
    assert_eq!(prod_entry["concurrency_blocked_by"].as_u64(), Some(2));

    let dev_entry = include
        .iter()
        .find(|e| e["stack"].as_str() == Some("dev"))
        .unwrap();
    assert_eq!(dev_entry["concurrency_blocked"].as_bool(), Some(false));
    assert_eq!(dev_entry["concurrency_blocked_by"].as_u64(), Some(0));
}
