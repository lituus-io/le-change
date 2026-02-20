//! Integration tests for the LeChange processing pipeline

use lechange_core::interner::StringInterner;
use lechange_core::output::ComputedOutputs;
use lechange_core::patterns::loader::PatternLoader;
use lechange_core::types::{ChangeType, ChangedFile, FileOrigin, InputConfig, ProcessedResult};
use std::borrow::Cow;
use std::fs;
use tempfile::TempDir;

fn create_test_repo() -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();
    let repo_path = dir.path().to_path_buf();

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

    (dir, repo_path)
}

fn commit(repo_path: &std::path::Path, message: &str) {
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(repo_path)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", message])
        .current_dir(repo_path)
        .output()
        .unwrap();
}

fn get_sha(repo_path: &std::path::Path) -> String {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_path)
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

// 6A: YAML pattern groups end-to-end
#[test]
fn test_yaml_pattern_groups_end_to_end() {
    let interner = StringInterner::new();

    let yaml = r#"
frontend:
  - "src/components/**"
  - "src/pages/**"
backend:
  - "src/api/**"
  - "src/models/**"
tests:
  - "tests/**"
"#;

    let groups = PatternLoader::load_yaml_groups(yaml, true).unwrap();

    // Create simulated files across groups
    let files = vec![
        ChangedFile {
            path: interner.intern("src/components/Button.tsx"),
            change_type: ChangeType::Modified,
            previous_path: None,
            is_symlink: false,
            submodule_depth: 0,
            origin: FileOrigin::default(),
        },
        ChangedFile {
            path: interner.intern("src/api/routes.ts"),
            change_type: ChangeType::Added,
            previous_path: None,
            is_symlink: false,
            submodule_depth: 0,
            origin: FileOrigin::default(),
        },
        ChangedFile {
            path: interner.intern("tests/test_api.rs"),
            change_type: ChangeType::Modified,
            previous_path: None,
            is_symlink: false,
            submodule_depth: 0,
            origin: FileOrigin::default(),
        },
        ChangedFile {
            path: interner.intern("README.md"),
            change_type: ChangeType::Modified,
            previous_path: None,
            is_symlink: false,
            submodule_depth: 0,
            origin: FileOrigin::default(),
        },
    ];

    let filtered_indices: Vec<u32> = (0..files.len() as u32).collect();

    // Match each group
    let mut group_results = Vec::new();
    for group in &groups {
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
        group_results.push(lechange_core::types::GroupResult {
            key: interner.intern(&group.name),
            matched_indices: matched,
        });
    }

    let result = ProcessedResult {
        all_files: files,
        filtered_indices,
        unmatched_indices: Vec::new(),
        pattern_applied: false,
        group_results,
        additions: 0,
        deletions: 0,
        diagnostics: Vec::new(),
        workflow_result: None,
        ci_decision: None,
    };

    let outputs = ComputedOutputs::compute(&result, false);

    // Verify group membership (resolve InternedString keys via interner)
    let find_group = |name: &str| -> &lechange_core::types::GroupResult {
        result
            .group_results
            .iter()
            .find(|g| interner.resolve(g.key) == Some(name))
            .unwrap()
    };

    let frontend = find_group("frontend");
    assert_eq!(frontend.matched_indices.len(), 1);

    let backend = find_group("backend");
    assert_eq!(backend.matched_indices.len(), 1);

    let tests_group = find_group("tests");
    assert_eq!(tests_group.matched_indices.len(), 1);

    // Verify changed_keys and modified_keys (InternedString)
    let frontend_key = interner.intern("frontend");
    let backend_key = interner.intern("backend");
    let tests_key = interner.intern("tests");

    assert!(outputs.changed_keys.contains(&frontend_key));
    assert!(outputs.changed_keys.contains(&backend_key));
    assert!(outputs.changed_keys.contains(&tests_key));

    // frontend and tests have Modified files, backend has Added
    assert!(outputs.modified_keys.contains(&frontend_key));
    assert!(outputs.modified_keys.contains(&tests_key));
    assert!(!outputs.modified_keys.contains(&backend_key));
}

// 6B: Rename splitting end-to-end
#[test]
fn test_rename_splitting_end_to_end() {
    let interner = StringInterner::new();

    let files = vec![
        ChangedFile {
            path: interner.intern("new_name.rs"),
            change_type: ChangeType::Renamed,
            previous_path: Some(interner.intern("old_name.rs")),
            is_symlink: false,
            submodule_depth: 0,
            origin: FileOrigin::default(),
        },
        ChangedFile {
            path: interner.intern("other.rs"),
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
        group_results: Vec::new(),
        additions: 0,
        deletions: 0,
        diagnostics: Vec::new(),
        workflow_result: None,
        ci_decision: None,
    };

    // With rename splitting
    let outputs = ComputedOutputs::compute(&result, true);
    assert!(outputs.filtered_renamed.is_empty());
    assert_eq!(outputs.filtered_added, vec![0]); // new_name.rs → added
    assert_eq!(outputs.rename_split_deletions.len(), 1); // old_name.rs → deleted
    let (idx, prev_path) = outputs.rename_split_deletions[0];
    assert_eq!(idx, 0);
    assert_eq!(interner.resolve(prev_path), Some("old_name.rs"));

    // Without rename splitting
    let outputs = ComputedOutputs::compute(&result, false);
    assert_eq!(outputs.filtered_renamed, vec![0]);
    assert!(outputs.filtered_added.is_empty());
    assert!(outputs.rename_split_deletions.is_empty());
}

// 6C: Soft-fail modes — verify skip_same_sha generates diagnostic instead of error
#[tokio::test]
async fn test_soft_fail_skip_same_sha() {
    let (_dir, repo_path) = create_test_repo();

    // Create initial commit
    fs::write(repo_path.join("file1.txt"), "content1").unwrap();
    commit(&repo_path, "Initial commit");

    let sha = get_sha(&repo_path);

    std::env::set_current_dir(&repo_path).unwrap();

    let repo = lechange_core::git::repository::GitRepository::discover(&repo_path).unwrap();
    let interner = StringInterner::new();

    // Use same SHA for base and head with skip_same_sha = true
    let config = InputConfig {
        base_sha: Some(Cow::Owned(sha.clone())),
        sha: Some(Cow::Owned(sha)),
        skip_same_sha: true,
        ..Default::default()
    };

    let processor =
        lechange_core::coordination::processor::FileProcessor::new(&repo, &interner, &config);
    let result = processor.process().await;

    // Should succeed with a diagnostic about same SHA
    assert!(result.is_ok());
    let result = result.unwrap();
    assert!(!result.diagnostics.is_empty());
    assert!(result
        .diagnostics
        .iter()
        .any(|d| { d.category == lechange_core::types::DiagnosticCategory::SkippedSameSha }));
    assert!(result.all_files.is_empty());
}

// 6C variant: Test that initial commit (no parent) falls back to empty tree diff
#[tokio::test]
async fn test_initial_commit_empty_tree_fallback() {
    let (_dir, repo_path) = create_test_repo();

    // Create initial commit (only commit = no parent for HEAD^)
    fs::write(repo_path.join("file1.txt"), "content1").unwrap();
    commit(&repo_path, "Initial commit");

    std::env::set_current_dir(&repo_path).unwrap();

    let repo = lechange_core::git::repository::GitRepository::discover(&repo_path).unwrap();
    let interner = StringInterner::new();

    // Default config uses HEAD^..HEAD, but there's no parent commit
    // Should fall back to empty tree → all files appear as "added"
    let config = InputConfig::default();

    let processor =
        lechange_core::coordination::processor::FileProcessor::new(&repo, &interner, &config);
    let result = processor.process().await;

    // With empty tree fallback, initial commit succeeds and shows all files
    assert!(result.is_ok());
    let processed = result.unwrap();
    assert!(
        !processed.all_files.is_empty(),
        "Initial commit should show all files as changed"
    );
}

// 6D: Pattern source file loading
#[tokio::test]
async fn test_pattern_source_file_loading() {
    let (_dir, repo_path) = create_test_repo();

    // Create initial commit with files in different dirs
    fs::create_dir_all(repo_path.join("src")).unwrap();
    fs::write(repo_path.join("src/main.rs"), "fn main() {}").unwrap();
    fs::write(repo_path.join("README.md"), "# readme").unwrap();
    commit(&repo_path, "Initial commit");

    // Create second commit
    fs::write(repo_path.join("src/main.rs"), "fn main() { println!() }").unwrap();
    fs::write(repo_path.join("README.md"), "# updated readme").unwrap();
    commit(&repo_path, "Update files");

    std::env::set_current_dir(&repo_path).unwrap();

    // Create a pattern source file
    let pattern_file = repo_path.join("patterns.txt");
    fs::write(&pattern_file, "**/*.rs\n").unwrap();

    let repo = lechange_core::git::repository::GitRepository::discover(&repo_path).unwrap();
    let interner = StringInterner::new();
    let config = InputConfig {
        files_from_source_file: Some(Cow::Owned(pattern_file.to_str().unwrap().to_string())),
        ..Default::default()
    };

    let processor =
        lechange_core::coordination::processor::FileProcessor::new(&repo, &interner, &config);
    let result = processor.process().await.unwrap();

    // Only .rs files should match
    assert!(result.pattern_applied);
    let matched_paths: Vec<&str> = result
        .filtered_indices
        .iter()
        .filter_map(|&i| interner.resolve(result.all_files[i as usize].path))
        .collect();
    assert!(matched_paths.iter().all(|p| p.ends_with(".rs")));

    // README.md should be in unmatched
    let unmatched_paths: Vec<&str> = result
        .unmatched_indices
        .iter()
        .filter_map(|&i| interner.resolve(result.all_files[i as usize].path))
        .collect();
    assert!(unmatched_paths.iter().any(|p| p.contains("README")));
}

// 6E: File recovery integration
#[test]
fn test_file_recovery_integration() {
    let (_dir, repo_path) = create_test_repo();

    // Create initial commit with a file
    fs::create_dir_all(repo_path.join("src")).unwrap();
    fs::write(repo_path.join("src/deleted.rs"), "fn deleted() {}").unwrap();
    fs::write(repo_path.join("keep.rs"), "fn keep() {}").unwrap();
    commit(&repo_path, "Initial commit");

    let sha_with_file = get_sha(&repo_path);

    // Delete the file and commit
    fs::remove_file(repo_path.join("src/deleted.rs")).unwrap();
    commit(&repo_path, "Delete src/deleted.rs");

    // Recover the file
    let output_dir = TempDir::new().unwrap();
    let recovery = lechange_core::git::recovery::FileRecovery::new(&repo_path);
    let result = recovery.recover_file(&sha_with_file, "src/deleted.rs", output_dir.path());

    assert!(result.is_ok());
    let output_path = result.unwrap();
    assert!(output_path.exists());
    let content = fs::read_to_string(&output_path).unwrap();
    assert_eq!(content, "fn deleted() {}");
}
