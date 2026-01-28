# Workflow Failure Tracking - Testing Guide

This document describes the comprehensive test suite for the workflow failure tracking feature.

## Overview

The workflow tracking feature integrates with GitHub Actions API to:
- Detect active workflows running on the same files
- Track workflow failures within configurable history
- Merge failed files with current changes
- Handle API rate limits and timeouts gracefully

## Test Structure

### Integration Tests (`tests/workflow_integration.rs`)

These tests verify the feature works correctly with the actual GitHub API in a real CI environment.

#### Test Categories

**1. API Connection Tests**
- `test_workflow_api_client_connection`: Verifies basic API connectivity
- `test_list_workflow_runs_with_status_filter`: Tests workflow querying with filters
- `test_get_commit_files`: Tests fetching files from specific commits

**2. Workflow Tracking Tests**
- `test_workflow_tracker_basic`: Tests the main workflow tracking logic
- `test_workflow_tracker_file_merging`: Tests file deduplication and origin flags
- `test_full_pipeline_with_workflow_tracking`: End-to-end pipeline integration

**3. Error Handling Tests**
- `test_rate_limit_detection`: Verifies rate limit error handling
- `test_exponential_backoff_timeout`: Tests timeout configuration
- `test_workflow_tracker_environment_parsing`: Tests environment variable parsing

#### Running Integration Tests

**Locally (without GitHub environment):**
```bash
# Run only tests that don't require GitHub API
cargo test --test workflow_integration test_workflow_tracker_environment_parsing
cargo test --test workflow_integration test_workflow_tracker_file_merging -- --ignored
```

**With GitHub environment variables:**
```bash
# Set required environment variables
export GITHUB_TOKEN="your_token_here"
export GITHUB_REPOSITORY="owner/repo"
export GITHUB_REF="refs/heads/main"

# Run all integration tests
cargo test --test workflow_integration -- --ignored --nocapture
```

**In GitHub Actions:**
```bash
# Tests run automatically with proper environment
# See .github/workflows/workflow-integration-test.yml
```

### GitHub Actions Workflow (`.github/workflows/workflow-integration-test.yml`)

Comprehensive CI pipeline that runs on every push/PR affecting workflow tracking code.

#### Test Jobs

**1. `workflow-api-tests`**
- Tests all API client functions
- Verifies connection, filtering, and data retrieval
- Runs 8 test scenarios in sequence

**2. `workflow-tracking-e2e`**
- End-to-end test with real git commits
- Creates test branch with multiple commits
- Verifies workflow tracking with actual API data
- Queries and reports workflow statistics

**3. `verify-error-handling`**
- Tests behavior without GITHUB_TOKEN
- Tests invalid input handling
- Verifies timeout configuration

**4. `performance-check`**
- Measures API call latency
- Measures full pipeline performance
- Verifies performance is under acceptable thresholds:
  - API calls: < 5 seconds
  - Full pipeline: < 10 seconds

**5. `final-summary`**
- Aggregates results from all jobs
- Generates comprehensive summary

## Test Coverage

### Unit Tests (in source files)

**`http/workflows.rs`:**
- Client creation
- Status/conclusion conversion
- Workflow run parsing

**`coordination/workflow_tracker.rs`:**
- File merging algorithm
- Environment variable parsing
- Deduplication logic

### Integration Tests

**API Integration:**
- Real HTTP requests to GitHub API
- Pagination handling
- Rate limit detection
- Error response handling

**Pipeline Integration:**
- Full detection pipeline with workflow tracking
- File origin flag verification
- Cross-module integration

## Environment Variables

Required for integration tests:

| Variable | Description | Example |
|----------|-------------|---------|
| `GITHUB_TOKEN` | GitHub API token (increases rate limit) | `ghp_xxxxx` |
| `GITHUB_REPOSITORY` | Repository in owner/repo format | `lituus-io/le-change` |
| `GITHUB_REF` | Current branch reference | `refs/heads/main` |
| `GITHUB_API_URL` | GitHub API base URL (optional) | `https://api.github.com` |

**Note:** In GitHub Actions, these variables are automatically set by the runner.

## Expected Test Results

### Success Criteria

✅ All API calls succeed (or fail with proper error handling)
✅ Workflow data is parsed correctly
✅ File merging produces correct origin flags
✅ Rate limits are detected and reported
✅ Timeouts are respected
✅ Performance is within acceptable ranges

### Common Test Scenarios

**Scenario 1: No Active Workflows**
- Result: Empty `blocking_runs`, workflow check completes immediately
- Wait time: ~0ms

**Scenario 2: Active Workflows (Different Files)**
- Result: Empty `blocking_runs`, no overlap detected
- Wait time: ~200-500ms (API query time)

**Scenario 3: Active Workflows (Same Files)**
- Result: Non-empty `blocking_runs`, waits for completion
- Wait time: Variable (depends on workflow duration)
- Exponential backoff: 1s → 2s → 4s → 8s → 16s → 30s (max)

**Scenario 4: Recent Failures**
- Result: Non-empty `failures` list
- Files: Merged with current changes
- Origin flags: Properly set for deduplication

**Scenario 5: Rate Limit Hit (No Token)**
- Result: `RateLimitExceeded` error
- Message: Helpful guidance to use GITHUB_TOKEN

## Debugging Tests

### Verbose Output

```bash
# Run with nocapture to see println! output
cargo test --test workflow_integration -- --ignored --nocapture
```

### Single Test

```bash
# Run specific test
cargo test --test workflow_integration test_workflow_api_client_connection -- --ignored --nocapture
```

### GitHub Actions Logs

1. Go to Actions tab in repository
2. Click on "Workflow Integration Tests" workflow
3. Click on specific run
4. Expand job steps to see detailed output

### Common Issues

**Issue: Tests fail with "GITHUB_TOKEN not set"**
- Solution: Set environment variables before running tests
- In CI: Verify secrets are properly configured

**Issue: Rate limit exceeded**
- Solution: Use GITHUB_TOKEN to increase rate limit from 60 to 5000 req/hr
- Without token: Tests may fail on public runners

**Issue: Timeout errors**
- Solution: Increase `workflow_max_wait_seconds` in test config
- Or: Wait for active workflows to complete before running tests

**Issue: "Not in a git repository"**
- Solution: Run tests from repository root
- Or: Tests will skip git-dependent assertions

## Continuous Integration

### Automated Testing

The workflow integration tests run automatically on:
- Every push to `main` or `develop` branches
- Every pull request to `main` or `develop` branches
- Manual trigger via workflow_dispatch
- When workflow tracking code is modified

### Pull Request Checks

Before merging PRs that modify workflow tracking:
1. All integration tests must pass
2. Performance checks must be within thresholds
3. Error handling tests must succeed
4. Full pipeline test must complete

### Monitoring

CI runs generate summaries showing:
- Number of workflows found
- Active workflow count
- Recent failure count
- API call latency
- Pipeline performance

## Manual Testing

### Local Development

```bash
# 1. Set up environment
export GITHUB_TOKEN="your_token"
export GITHUB_REPOSITORY="your_org/your_repo"
export GITHUB_REF="refs/heads/your_branch"

# 2. Run specific test suite
cargo test --test workflow_integration -- --ignored --nocapture

# 3. Verify output
# Look for:
# - "Found X workflows"
# - "Queued workflows: X"
# - "In-progress workflows: X"
# - "Completed workflows: X"
# - "Commit has X files"
# - "Pipeline completed successfully!"
```

### Production Verification

```bash
# In a real GitHub Actions workflow
- name: Test workflow tracking
  env:
    GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
  run: |
    cargo test --test workflow_integration -- --ignored --nocapture
```

## Future Enhancements

Potential test additions:
- [ ] Mock server tests (no API dependency)
- [ ] Concurrency stress tests (many parallel API calls)
- [ ] Large commit tests (>300 files, pagination)
- [ ] Multiple simultaneous workflows
- [ ] Long-running workflow timeout scenarios
- [ ] Network failure retry logic
- [ ] Cache invalidation tests

## Contributing

When adding new workflow tracking features:

1. Add unit tests in the source module
2. Add integration tests in `workflow_integration.rs`
3. Update GitHub Actions workflow if needed
4. Update this document with new test descriptions
5. Ensure all tests pass before submitting PR

## Resources

- [GitHub Actions API Documentation](https://docs.github.com/en/rest/actions)
- [GitHub Actions Environment Variables](https://docs.github.com/en/actions/learn-github-actions/environment-variables)
- [Rust Testing Documentation](https://doc.rust-lang.org/book/ch11-00-testing.html)
- [Tokio Testing Guide](https://tokio.rs/tokio/topics/testing)
