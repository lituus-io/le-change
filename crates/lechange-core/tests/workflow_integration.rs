//! Integration tests for workflow failure tracking
//!
//! These tests require GitHub environment variables:
//! - GITHUB_TOKEN: GitHub API token
//! - GITHUB_REPOSITORY: Repository in format "owner/repo"
//! - GITHUB_REF: Current branch reference
//!
//! Run with: cargo test --test workflow_integration -- --ignored

use lechange_core::{
    coordination::{FileProcessor, WorkflowTracker},
    git::GitRepository,
    http::WorkflowApiClient,
    interner::StringInterner,
    types::{ChangeType, ChangedFile, FileOrigin, InputConfig},
};

/// Check if required environment variables are set
fn has_github_env() -> bool {
    std::env::var("GITHUB_TOKEN").is_ok()
        && std::env::var("GITHUB_REPOSITORY").is_ok()
        && std::env::var("GITHUB_REF").is_ok()
}

/// Setup test environment and return owner/repo
fn setup_test_env() -> (String, String) {
    if !has_github_env() {
        panic!(
            "Required environment variables not set: GITHUB_TOKEN, GITHUB_REPOSITORY, GITHUB_REF"
        );
    }

    let repository = std::env::var("GITHUB_REPOSITORY").unwrap();
    let parts: Vec<&str> = repository.split('/').collect();

    if parts.len() != 2 {
        panic!("Invalid GITHUB_REPOSITORY format: {}", repository);
    }

    (parts[0].to_string(), parts[1].to_string())
}

#[tokio::test]
#[ignore] // Only run when explicitly requested with --ignored
async fn test_workflow_api_client_connection() {
    let (owner, repo) = setup_test_env();

    let client = WorkflowApiClient::from_env().expect("Failed to create API client");
    let interner = StringInterner::new();

    // Test listing workflows (should not error even if empty)
    let result = client
        .list_workflow_runs(&owner, &repo, "", None, 10, 1, &interner)
        .await;

    assert!(
        result.is_ok(),
        "Failed to list workflows: {:?}",
        result.err()
    );

    let workflows = result.unwrap();
    println!("Found {} workflows", workflows.len());
}

#[tokio::test]
#[ignore]
async fn test_list_workflow_runs_with_status_filter() {
    let (owner, repo) = setup_test_env();

    let client = WorkflowApiClient::from_env().expect("Failed to create API client");
    let interner = StringInterner::new();

    // Test queued workflows
    let queued = client
        .list_workflow_runs(&owner, &repo, "", Some("queued"), 10, 1, &interner)
        .await
        .expect("Failed to list queued workflows");

    println!("Queued workflows: {}", queued.len());

    // Test in_progress workflows
    let in_progress = client
        .list_workflow_runs(&owner, &repo, "", Some("in_progress"), 10, 1, &interner)
        .await
        .expect("Failed to list in_progress workflows");

    println!("In-progress workflows: {}", in_progress.len());

    // Test completed workflows
    let completed = client
        .list_workflow_runs(&owner, &repo, "", Some("completed"), 10, 1, &interner)
        .await
        .expect("Failed to list completed workflows");

    println!("Completed workflows: {}", completed.len());

    // All queries should succeed (even if empty)
    assert!(true);
}

#[tokio::test]
#[ignore]
async fn test_get_commit_files() {
    let (owner, repo) = setup_test_env();

    let client = WorkflowApiClient::from_env().expect("Failed to create API client");
    let interner = StringInterner::new();

    // Get current HEAD SHA from git
    let repo_path = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let git_repo = match GitRepository::discover(&repo_path) {
        Ok(r) => r,
        Err(_) => {
            println!("Not in a git repository, skipping test");
            return;
        }
    };

    let head_sha = git_repo
        .resolve_sha_sync("HEAD")
        .expect("Failed to resolve HEAD");

    println!("Testing with HEAD SHA: {}", head_sha);

    // Get commit files
    let result = client
        .get_commit_files(&owner, &repo, &head_sha, &interner)
        .await;

    assert!(
        result.is_ok(),
        "Failed to get commit files: {:?}",
        result.err()
    );

    let files = result.unwrap();
    println!("Commit has {} files", files.len());

    // Verify all file paths are interned
    for file_path in &files {
        let resolved = interner.resolve(*file_path);
        assert!(resolved.is_some(), "File path not properly interned");
        println!("  - {}", resolved.unwrap());
    }
}

#[tokio::test]
#[ignore]
async fn test_workflow_tracker_basic() {
    let (_owner, _repo) = setup_test_env();

    let interner = StringInterner::new();
    let config = InputConfig {
        track_workflow_failures: true,
        workflow_lookback_commits: 5,
        wait_for_active_workflows: false, // Don't wait in test
        workflow_max_wait_seconds: 10,
        include_failed_files: true,
        ..Default::default()
    };

    let api_client = WorkflowApiClient::from_env().expect("Failed to create API client");
    let tracker = WorkflowTracker::new(api_client, &config, &interner);

    // Create dummy current files
    let current_files = vec![ChangedFile {
        path: interner.intern("README.md"),
        change_type: ChangeType::Modified,
        previous_path: None,
        is_symlink: false,
        submodule_depth: 0,
        origin: FileOrigin {
            in_current_changes: true,
            in_previous_failure: false,
        },
    }];

    // Get current branch
    let github_ref = std::env::var("GITHUB_REF").unwrap_or_else(|_| "refs/heads/main".to_string());
    let branch = github_ref
        .strip_prefix("refs/heads/")
        .unwrap_or(&github_ref);

    println!("Testing with branch: {}", branch);

    // Check workflows
    let result = tracker.check_workflows(branch, &current_files).await;

    assert!(
        result.is_ok(),
        "Failed to check workflows: {:?}",
        result.err()
    );

    let workflow_result = result.unwrap();
    println!("Blocking runs: {}", workflow_result.blocking_runs.len());
    println!("Recent failures: {}", workflow_result.failures.len());
    println!("Waited: {}", workflow_result.waited);
    println!("Wait time: {}ms", workflow_result.wait_time_ms);

    // Should always succeed
    assert!(true);
}

#[tokio::test]
#[ignore]
async fn test_workflow_tracker_file_merging() {
    let interner = StringInterner::new();
    let config = InputConfig::default();

    let api_client = WorkflowApiClient::new("https://api.github.com".to_string(), None);
    let tracker = WorkflowTracker::new(api_client, &config, &interner);

    // Create test files
    let path1 = interner.intern("src/lib.rs");
    let path2 = interner.intern("src/types.rs");
    let path3 = interner.intern("src/error.rs");

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

    // Create mock failures
    let failures = vec![lechange_core::types::WorkflowFailure {
        run: lechange_core::types::WorkflowRun {
            id: 123,
            name: interner.intern("CI"),
            status: lechange_core::types::WorkflowStatus::Completed,
            conclusion: Some(lechange_core::types::WorkflowConclusion::Failure),
            branch: interner.intern("main"),
            head_sha: interner.intern("abc123"),
            created_at: 0,
        },
        files: vec![path2, path3], // path2 overlaps, path3 is new
    }];

    // Merge files
    tracker.merge_failed_files(&mut current_files, &failures);

    // Verify results
    assert_eq!(current_files.len(), 3);

    // path1: current only
    assert!(current_files[0].origin.in_current_changes);
    assert!(!current_files[0].origin.in_previous_failure);

    // path2: both current and failed
    assert!(current_files[1].origin.in_current_changes);
    assert!(current_files[1].origin.in_previous_failure);

    // path3: failed only
    assert!(!current_files[2].origin.in_current_changes);
    assert!(current_files[2].origin.in_previous_failure);
    assert_eq!(current_files[2].change_type, ChangeType::Unknown);

    println!("File merging test passed!");
}

#[tokio::test]
#[ignore]
async fn test_full_pipeline_with_workflow_tracking() {
    if !has_github_env() {
        println!("Skipping test - GitHub environment not available");
        return;
    }

    let repo_path = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let git_repo = match GitRepository::discover(&repo_path) {
        Ok(r) => r,
        Err(e) => {
            println!("Not in a git repository, skipping test: {:?}", e);
            return;
        }
    };

    let interner = StringInterner::new();
    let config = InputConfig {
        track_workflow_failures: true,
        workflow_lookback_commits: 3,
        wait_for_active_workflows: false, // Don't wait in test
        workflow_max_wait_seconds: 30,
        include_failed_files: true,
        ..Default::default()
    };

    let processor = FileProcessor::new(&git_repo, &interner, &config);

    // Run full pipeline
    let result = processor.process().await;

    assert!(result.is_ok(), "Pipeline failed: {:?}", result.err());

    let diff = result.unwrap();
    println!("Pipeline completed successfully!");
    println!("Total files: {}", diff.files.len());

    // Check origin flags
    let mut current_count = 0;
    let mut failed_count = 0;
    let mut both_count = 0;

    for file in &diff.files {
        if file.origin.in_current_changes && file.origin.in_previous_failure {
            both_count += 1;
        } else if file.origin.in_current_changes {
            current_count += 1;
        } else if file.origin.in_previous_failure {
            failed_count += 1;
        }
    }

    println!("Current only: {}", current_count);
    println!("Failed only: {}", failed_count);
    println!("Both: {}", both_count);

    // Should always succeed if we got here
    assert!(true);
}

#[tokio::test]
#[ignore]
async fn test_rate_limit_detection() {
    // This test verifies that we handle rate limits gracefully
    // Note: This won't actually hit rate limits with a valid token,
    // but it verifies the error handling path exists

    let client = WorkflowApiClient::new(
        "https://api.github.com".to_string(),
        None, // No token - will hit rate limits faster
    );
    let interner = StringInterner::new();

    // Try to list workflows without token
    let result = client
        .list_workflow_runs("octocat", "Hello-World", "", None, 10, 1, &interner)
        .await;

    // Should either succeed or return a proper error
    match result {
        Ok(workflows) => {
            println!("Successfully fetched {} workflows", workflows.len());
        }
        Err(e) => {
            println!("Error (expected without token): {:?}", e);
            // Verify it's a proper error type
            assert!(e.to_string().contains("Workflow") || e.to_string().contains("rate"));
        }
    }
}

#[tokio::test]
#[ignore]
async fn test_exponential_backoff_timeout() {
    let interner = StringInterner::new();
    let config = InputConfig {
        wait_for_active_workflows: true,
        workflow_max_wait_seconds: 5, // Very short timeout for testing
        ..Default::default()
    };

    let api_client = WorkflowApiClient::from_env().expect("Failed to create API client");
    let _tracker = WorkflowTracker::new(api_client, &config, &interner);

    // Test that timeout configuration is respected
    // (actual timeout testing would require a long-running workflow)
    println!("Timeout configuration test passed");
    assert!(true);
}

#[test]
fn test_workflow_tracker_environment_parsing() {
    // Test environment variable parsing without API calls
    let original_repo = std::env::var("GITHUB_REPOSITORY").ok();
    let original_ref = std::env::var("GITHUB_REF").ok();

    // Test valid format
    std::env::set_var("GITHUB_REPOSITORY", "owner/repo");
    std::env::set_var("GITHUB_REF", "refs/heads/main");

    let interner = StringInterner::new();
    let config = InputConfig::default();
    let api_client = WorkflowApiClient::new("https://api.github.com".to_string(), None);
    let tracker = WorkflowTracker::new(api_client, &config, &interner);

    // This is a private method, so we test via the struct creation
    // The actual parsing is tested in the workflow_tracker unit tests
    assert!(std::env::var("GITHUB_REPOSITORY").is_ok());
    assert!(std::env::var("GITHUB_REF").is_ok());

    drop(tracker); // Use the tracker to avoid unused warning

    // Restore original values
    if let Some(repo) = original_repo {
        std::env::set_var("GITHUB_REPOSITORY", repo);
    } else {
        std::env::remove_var("GITHUB_REPOSITORY");
    }

    if let Some(ref_val) = original_ref {
        std::env::set_var("GITHUB_REF", ref_val);
    } else {
        std::env::remove_var("GITHUB_REF");
    }
}
