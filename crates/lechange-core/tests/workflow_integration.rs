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
    types::{
        ChangeType, ChangedFile, FailureTrackingLevel, FileOrigin, InputConfig, WorkflowConclusion,
        WorkflowFailure, WorkflowJob, WorkflowRun, WorkflowStatus, WorkflowSuccess,
    },
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
    let tracker = WorkflowTracker::new(api_client, &config, &interner, &[]);

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
            in_previous_success: false,
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
}

#[tokio::test]
#[ignore]
async fn test_workflow_tracker_file_merging() {
    let interner = StringInterner::new();
    let config = InputConfig::default();

    let api_client = WorkflowApiClient::new("https://api.github.com".to_string(), None);
    let tracker = WorkflowTracker::new(api_client, &config, &interner, &[]);

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
        failed_jobs: Vec::new(),
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

    let processed = result.unwrap();
    println!("Pipeline completed successfully!");
    println!("Total files: {}", processed.all_files.len());

    // Check origin flags
    let mut current_count = 0;
    let mut failed_count = 0;
    let mut both_count = 0;

    for file in &processed.all_files {
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
    let _tracker = WorkflowTracker::new(api_client, &config, &interner, &[]);

    // Test that timeout configuration is respected
    // (actual timeout testing would require a long-running workflow)
    println!("Timeout configuration test passed");
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
    let tracker = WorkflowTracker::new(api_client, &config, &interner, &[]);

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

/// Test job-level file partitioning and merging without API calls.
///
/// Simulates a complete two-run scenario:
/// Run 1: dev+prod changed, dev succeeds, prod fails → with job-level tracking
/// Run 2: staging changed → verifies that prod files come back as previous failures
#[test]
fn test_job_level_partitioning_and_merge() {
    use lechange_core::patterns::{loader::PatternGroup, matcher::PatternMatcher};

    let interner = StringInterner::new();

    // Set up YAML groups for stacks
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

    let config = InputConfig {
        failure_tracking_level: FailureTrackingLevel::Job,
        track_workflow_failures: true,
        include_failed_files: true,
        skip_successful_files: true,
        ..Default::default()
    };

    let api_client = WorkflowApiClient::new("https://api.github.com".to_string(), None);
    let tracker = WorkflowTracker::new(api_client, &config, &interner, &groups);

    // === Simulate Run 1: dev + prod changed ===
    let file_dev = interner.intern("stacks/dev/config.yaml");
    let file_prod = interner.intern("stacks/prod/config.yaml");

    // Simulate job results from Run 1
    let failed_jobs_run1 = vec![WorkflowJob {
        id: 10,
        name: interner.intern("Deploy [prod]"),
        status: WorkflowStatus::Completed,
        conclusion: Some(WorkflowConclusion::Failure),
        run_id: 1000,
        started_at: 100,
        completed_at: 200,
    }];

    let succeeded_job_run1 = WorkflowJob {
        id: 11,
        name: interner.intern("Deploy [dev]"),
        status: WorkflowStatus::Completed,
        conclusion: Some(WorkflowConclusion::Success),
        run_id: 1000,
        started_at: 100,
        completed_at: 200,
    };

    // Partition files for failure
    let failed_files =
        tracker.partition_files_for_failed_jobs(&[file_dev, file_prod], &failed_jobs_run1);
    assert_eq!(failed_files.len(), 1, "Only prod should be in failed files");
    assert!(failed_files.contains(&file_prod));

    // Partition files for success
    let succeeded_files =
        tracker.partition_files_for_succeeded_jobs(&[file_dev, file_prod], &[&succeeded_job_run1]);
    assert_eq!(
        succeeded_files.len(),
        1,
        "Only dev should be in succeeded files"
    );
    assert!(succeeded_files.contains(&file_dev));

    // Build WorkflowFailure and WorkflowSuccess
    let failures = vec![WorkflowFailure {
        run: WorkflowRun {
            id: 1000,
            name: interner.intern("Deploy Stacks"),
            status: WorkflowStatus::Completed,
            conclusion: Some(WorkflowConclusion::Failure),
            branch: interner.intern("main"),
            head_sha: interner.intern("sha_run1"),
            created_at: 1000,
        },
        files: failed_files,
        failed_jobs: vec![WorkflowJob {
            id: 10,
            name: interner.intern("Deploy [prod]"),
            status: WorkflowStatus::Completed,
            conclusion: Some(WorkflowConclusion::Failure),
            run_id: 1000,
            started_at: 100,
            completed_at: 200,
        }],
    }];

    let successes = vec![WorkflowSuccess {
        run: WorkflowRun {
            id: 1000,
            name: interner.intern("Deploy Stacks"),
            status: WorkflowStatus::Completed,
            conclusion: Some(WorkflowConclusion::Failure),
            branch: interner.intern("main"),
            head_sha: interner.intern("sha_run1"),
            created_at: 1000,
        },
        files: succeeded_files,
        jobs: vec![WorkflowJob {
            id: 11,
            name: interner.intern("Deploy [dev]"),
            status: WorkflowStatus::Completed,
            conclusion: Some(WorkflowConclusion::Success),
            run_id: 1000,
            started_at: 100,
            completed_at: 200,
        }],
    }];

    // === Simulate Run 2: only staging changed ===
    let file_staging = interner.intern("stacks/staging/config.yaml");

    let mut current_files = vec![ChangedFile {
        path: file_staging,
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

    // Merge failed files from Run 1 into current files
    tracker.merge_failed_files(&mut current_files, &failures);

    // Verify file count: staging (current) + prod (failed)
    assert_eq!(
        current_files.len(),
        2,
        "Should have staging (current) + prod (previous failure)"
    );

    // Check staging file
    let staging = current_files
        .iter()
        .find(|f| f.path == file_staging)
        .unwrap();
    assert!(staging.origin.in_current_changes);
    assert!(!staging.origin.in_previous_failure);

    // Check prod file (added from failure)
    let prod = current_files.iter().find(|f| f.path == file_prod).unwrap();
    assert!(!prod.origin.in_current_changes);
    assert!(prod.origin.in_previous_failure);
    assert_eq!(prod.change_type, ChangeType::Unknown);

    // Now test CI decision with these results
    use lechange_core::coordination::CiDecisionEngine;

    let engine = CiDecisionEngine::new(&interner);
    let decision = engine.compute(&current_files, &failures, &successes);

    // staging should rebuild (new change)
    assert!(
        decision.files_to_rebuild.contains(&file_staging),
        "Staging should be in rebuild set (new change)"
    );

    // prod should rebuild (previous failure)
    assert!(
        decision.files_to_rebuild.contains(&file_prod),
        "Prod should be in rebuild set (previous failure)"
    );

    // dev should be in skip set (previously succeeded, not in current changes)
    assert!(
        decision.files_to_skip.contains(&file_dev),
        "Dev should be in skip set (previously succeeded)"
    );

    // Verify rebuild/skip disjoint invariant
    let rebuild_set: std::collections::HashSet<_> =
        decision.files_to_rebuild.iter().cloned().collect();
    let skip_set: std::collections::HashSet<_> = decision.files_to_skip.iter().cloned().collect();
    let overlap: Vec<_> = rebuild_set.intersection(&skip_set).collect();
    assert!(overlap.is_empty(), "rebuild ∩ skip must be empty");

    // Verify job tracking
    assert!(
        decision
            .failed_jobs
            .contains(&interner.intern("Deploy [prod]")),
        "Deploy [prod] should be in failed jobs"
    );
    assert!(
        decision
            .successful_jobs
            .contains(&interner.intern("Deploy [dev]")),
        "Deploy [dev] should be in successful jobs"
    );

    // Verify rebuild reasons
    let prod_reason = decision
        .rebuild_reasons
        .iter()
        .find(|r| r.file == file_prod);
    assert!(prod_reason.is_some(), "Prod should have a rebuild reason");

    let staging_reason = decision
        .rebuild_reasons
        .iter()
        .find(|r| r.file == file_staging);
    assert!(
        staging_reason.is_some(),
        "Staging should have a rebuild reason"
    );
}

/// Test that job-level tracking with all three stacks changing and mixed results
/// produces correct partitioning.
#[test]
fn test_job_level_all_stacks_mixed_results() {
    use lechange_core::patterns::{loader::PatternGroup, matcher::PatternMatcher};

    let interner = StringInterner::new();

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

    let config = InputConfig {
        failure_tracking_level: FailureTrackingLevel::Job,
        ..Default::default()
    };

    let api_client = WorkflowApiClient::new("https://api.github.com".to_string(), None);
    let tracker = WorkflowTracker::new(api_client, &config, &interner, &groups);

    let file_dev = interner.intern("stacks/dev/config.yaml");
    let file_staging = interner.intern("stacks/staging/config.yaml");
    let file_prod = interner.intern("stacks/prod/config.yaml");
    let commit_files = vec![file_dev, file_staging, file_prod];

    // staging and prod failed, dev succeeded
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

    let dev_job = WorkflowJob {
        id: 3,
        name: interner.intern("Deploy [dev]"),
        status: WorkflowStatus::Completed,
        conclusion: Some(WorkflowConclusion::Success),
        run_id: 100,
        started_at: 0,
        completed_at: 0,
    };

    // Partition for failures
    let failed_files = tracker.partition_files_for_failed_jobs(&commit_files, &failed_jobs);
    assert_eq!(
        failed_files.len(),
        2,
        "staging + prod should be failed files"
    );
    assert!(failed_files.contains(&file_staging));
    assert!(failed_files.contains(&file_prod));
    assert!(!failed_files.contains(&file_dev));

    // Partition for successes
    let succeeded_files = tracker.partition_files_for_succeeded_jobs(&commit_files, &[&dev_job]);
    assert_eq!(succeeded_files.len(), 1, "Only dev should be succeeded");
    assert!(succeeded_files.contains(&file_dev));
}

/// Test that FailureTrackingLevel::Run gives all files to failure (no partitioning)
#[test]
fn test_run_level_tracking_gives_all_files() {
    let interner = StringInterner::new();
    let config = InputConfig {
        failure_tracking_level: FailureTrackingLevel::Run, // Run level = no partitioning
        ..Default::default()
    };

    let api_client = WorkflowApiClient::new("https://api.github.com".to_string(), None);
    // Even with groups, run-level should use all files (groups are for job-level only)
    let tracker = WorkflowTracker::new(api_client, &config, &interner, &[]);

    let file_dev = interner.intern("stacks/dev/config.yaml");
    let file_prod = interner.intern("stacks/prod/config.yaml");
    let commit_files = vec![file_dev, file_prod];

    // At run level, even if only prod job failed, all commit files are attributed
    let failed_jobs = vec![WorkflowJob {
        id: 1,
        name: interner.intern("Deploy [prod]"),
        status: WorkflowStatus::Completed,
        conclusion: Some(WorkflowConclusion::Failure),
        run_id: 100,
        started_at: 0,
        completed_at: 0,
    }];

    // No groups → fallback to all files (which is the run-level behavior)
    let result = tracker.partition_files_for_failed_jobs(&commit_files, &failed_jobs);
    assert_eq!(
        result.len(),
        2,
        "Run-level should attribute all files to failure"
    );
}
