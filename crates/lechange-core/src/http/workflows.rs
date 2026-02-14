//! GitHub Actions Workflow API client

use crate::error::{Error, Result};
use crate::interner::StringInterner;
use crate::types::{InternedString, WorkflowConclusion, WorkflowJob, WorkflowRun, WorkflowStatus};
use serde::Deserialize;

/// GitHub API response for workflow runs list
#[derive(Debug, Deserialize)]
struct WorkflowRunsResponse {
    #[allow(dead_code)]
    total_count: u32,
    workflow_runs: Vec<GitHubWorkflowRun>,
}

/// GitHub API workflow run object
#[derive(Debug, Deserialize)]
struct GitHubWorkflowRun {
    id: u64,
    name: String,
    status: String,
    conclusion: Option<String>,
    head_branch: String,
    head_sha: String,
    created_at: String,
}

/// GitHub API response for workflow jobs list
#[derive(Debug, Deserialize)]
struct WorkflowJobsResponse {
    #[allow(dead_code)]
    total_count: u32,
    jobs: Vec<GitHubWorkflowJob>,
}

/// GitHub API job object
#[derive(Debug, Deserialize)]
struct GitHubWorkflowJob {
    id: u64,
    name: String,
    status: String,
    conclusion: Option<String>,
    #[allow(dead_code)]
    run_id: u64,
    started_at: Option<String>,
    completed_at: Option<String>,
}

/// GitHub API response for commit details
#[derive(Debug, Deserialize)]
struct GitHubCommit {
    #[allow(dead_code)]
    sha: String,
    files: Option<Vec<GitHubCommitFile>>,
}

#[derive(Debug, Deserialize)]
struct GitHubCommitFile {
    filename: String,
    #[allow(dead_code)]
    status: String,
}

/// GitHub Actions workflow API client
pub struct WorkflowApiClient {
    client: reqwest::Client,
    base_url: String,
    token: Option<String>,
}

impl WorkflowApiClient {
    /// Create new workflow API client
    pub fn new(base_url: String, token: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .user_agent("lechange/0.1.0")
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            client,
            base_url,
            token,
        }
    }

    /// Create from environment variables
    pub fn from_env() -> Result<Self> {
        let base_url = std::env::var("GITHUB_API_URL")
            .unwrap_or_else(|_| "https://api.github.com".to_string());

        let token = std::env::var("GITHUB_TOKEN").ok();

        Ok(Self::new(base_url, token))
    }

    /// List workflow runs for a branch with filtering
    ///
    /// Endpoint: GET /repos/{owner}/{repo}/actions/runs
    /// Query params: branch, status, per_page, page
    #[allow(clippy::too_many_arguments)]
    pub async fn list_workflow_runs(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
        status: Option<&str>,
        per_page: u32,
        page: u32,
        interner: &StringInterner,
    ) -> Result<Vec<WorkflowRun>> {
        let url = format!("{}/repos/{}/{}/actions/runs", self.base_url, owner, repo);

        let mut request = self.client.get(&url).query(&[
            ("per_page", per_page.to_string().as_str()),
            ("page", page.to_string().as_str()),
        ]);

        // Add branch filter if not empty (empty = all branches)
        if !branch.is_empty() {
            request = request.query(&[("branch", branch)]);
        }

        if let Some(status_filter) = status {
            request = request.query(&[("status", status_filter)]);
        }

        if let Some(ref token) = self.token {
            request = request.header("Authorization", format!("Bearer {}", token));
        }

        let response = request
            .send()
            .await
            .map_err(|e| Error::Workflow(format!("Failed to fetch workflow runs: {}", e)))?;

        // Check for rate limiting
        if response.status() == reqwest::StatusCode::FORBIDDEN {
            let remaining = response
                .headers()
                .get("x-ratelimit-remaining")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("0");

            return Err(Error::RateLimitExceeded(format!(
                "GitHub API rate limit exceeded. Remaining: {}. Consider using GITHUB_TOKEN.",
                remaining
            )));
        }

        if !response.status().is_success() {
            return Err(Error::Workflow(format!(
                "GitHub API returned error: {}",
                response.status()
            )));
        }

        let runs_response: WorkflowRunsResponse = response.json().await.map_err(|e| {
            Error::Workflow(format!("Failed to parse workflow runs response: {}", e))
        })?;

        // Convert to our types
        Ok(runs_response
            .workflow_runs
            .into_iter()
            .map(|run| self.convert_workflow_run(run, interner))
            .collect())
    }

    /// Get files changed in a specific commit
    ///
    /// Endpoint: GET /repos/{owner}/{repo}/commits/{sha}
    /// Handles pagination for commits with >300 files
    pub async fn get_commit_files(
        &self,
        owner: &str,
        repo: &str,
        sha: &str,
        interner: &StringInterner,
    ) -> Result<Vec<InternedString>> {
        let url = format!("{}/repos/{}/{}/commits/{}", self.base_url, owner, repo, sha);

        let mut all_files = Vec::new();
        let mut page = 1;

        loop {
            let mut request = self
                .client
                .get(&url)
                .query(&[("per_page", "100"), ("page", &page.to_string())]);

            if let Some(ref token) = self.token {
                request = request.header("Authorization", format!("Bearer {}", token));
            }

            let response = request
                .send()
                .await
                .map_err(|e| Error::Workflow(format!("Failed to fetch commit: {}", e)))?;

            if !response.status().is_success() {
                return Err(Error::Workflow(format!(
                    "GitHub API returned error for commit: {}",
                    response.status()
                )));
            }

            // Check for pagination via Link header
            let has_next = response
                .headers()
                .get("Link")
                .and_then(|v| v.to_str().ok())
                .map(|link| link.contains("rel=\"next\""))
                .unwrap_or(false);

            let commit: GitHubCommit = response
                .json()
                .await
                .map_err(|e| Error::Workflow(format!("Failed to parse commit response: {}", e)))?;

            // Extract file paths
            if let Some(files) = commit.files {
                all_files.extend(files.into_iter().map(|f| interner.intern(&f.filename)));
            }

            if !has_next {
                break;
            }

            page += 1;

            // Safety limit
            if page > 100 {
                return Err(Error::Workflow(
                    "Commit has too many files (>10000)".to_string(),
                ));
            }
        }

        Ok(all_files)
    }

    /// Get a specific workflow run
    ///
    /// Endpoint: GET /repos/{owner}/{repo}/actions/runs/{run_id}
    pub async fn get_workflow_run(
        &self,
        owner: &str,
        repo: &str,
        run_id: u64,
        interner: &StringInterner,
    ) -> Result<WorkflowRun> {
        let url = format!(
            "{}/repos/{}/{}/actions/runs/{}",
            self.base_url, owner, repo, run_id
        );

        let mut request = self.client.get(&url);

        if let Some(ref token) = self.token {
            request = request.header("Authorization", format!("Bearer {}", token));
        }

        let response = request
            .send()
            .await
            .map_err(|e| Error::Workflow(format!("Failed to fetch workflow run: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::Workflow(format!(
                "GitHub API error checking workflow: {}",
                response.status()
            )));
        }

        let run: GitHubWorkflowRun = response
            .json()
            .await
            .map_err(|e| Error::Workflow(format!("Failed to parse workflow run: {}", e)))?;

        Ok(self.convert_workflow_run(run, interner))
    }

    /// Wait for a specific workflow to complete with exponential backoff
    ///
    /// Returns when workflow completes or timeout is reached
    pub async fn wait_for_workflow(
        &self,
        owner: &str,
        repo: &str,
        run_id: u64,
        max_wait_seconds: u32,
        interner: &StringInterner,
    ) -> Result<WorkflowRun> {
        let start = std::time::Instant::now();
        let mut backoff_ms = 1000u64; // Start at 1 second
        const MAX_BACKOFF_MS: u64 = 30000; // Cap at 30 seconds

        loop {
            // Check timeout
            if start.elapsed().as_secs() >= max_wait_seconds as u64 {
                return Err(Error::WorkflowTimeout(format!(
                    "Workflow {} did not complete within {} seconds. Consider increasing workflow_max_wait_seconds.",
                    run_id, max_wait_seconds
                )));
            }

            // Fetch current workflow status
            let workflow_run = self.get_workflow_run(owner, repo, run_id, interner).await?;

            // Check if completed
            if workflow_run.status == WorkflowStatus::Completed {
                return Ok(workflow_run);
            }

            // Exponential backoff
            tokio::time::sleep(tokio::time::Duration::from_millis(backoff_ms)).await;
            backoff_ms = (backoff_ms * 2).min(MAX_BACKOFF_MS);
        }
    }

    /// List jobs for a workflow run
    ///
    /// Endpoint: GET /repos/{owner}/{repo}/actions/runs/{run_id}/jobs
    pub async fn list_workflow_jobs(
        &self,
        owner: &str,
        repo: &str,
        run_id: u64,
        interner: &StringInterner,
    ) -> Result<Vec<WorkflowJob>> {
        let url = format!(
            "{}/repos/{}/{}/actions/runs/{}/jobs",
            self.base_url, owner, repo, run_id
        );

        let mut request = self.client.get(&url).query(&[("per_page", "100")]);

        if let Some(ref token) = self.token {
            request = request.header("Authorization", format!("Bearer {}", token));
        }

        let response = request
            .send()
            .await
            .map_err(|e| Error::Workflow(format!("Failed to fetch workflow jobs: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::Workflow(format!(
                "GitHub API error fetching jobs: {}",
                response.status()
            )));
        }

        let jobs_response: WorkflowJobsResponse = response.json().await.map_err(|e| {
            Error::Workflow(format!("Failed to parse workflow jobs response: {}", e))
        })?;

        Ok(jobs_response
            .jobs
            .into_iter()
            .map(|job| self.convert_workflow_job(job, run_id, interner))
            .collect())
    }

    /// Convert GitHub API job to our type
    fn convert_workflow_job(
        &self,
        job: GitHubWorkflowJob,
        run_id: u64,
        interner: &StringInterner,
    ) -> WorkflowJob {
        let status = match job.status.as_str() {
            "queued" => WorkflowStatus::Queued,
            "in_progress" => WorkflowStatus::InProgress,
            _ => WorkflowStatus::Completed,
        };

        let conclusion = job.conclusion.as_ref().map(|c| match c.as_str() {
            "success" => WorkflowConclusion::Success,
            "failure" => WorkflowConclusion::Failure,
            "cancelled" => WorkflowConclusion::Cancelled,
            "skipped" => WorkflowConclusion::Skipped,
            "timed_out" => WorkflowConclusion::TimedOut,
            _ => WorkflowConclusion::Neutral,
        });

        let started_at = job
            .started_at
            .as_ref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.timestamp())
            .unwrap_or(0);

        let completed_at = job
            .completed_at
            .as_ref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.timestamp())
            .unwrap_or(0);

        WorkflowJob {
            id: job.id,
            name: interner.intern(&job.name),
            status,
            conclusion,
            run_id,
            started_at,
            completed_at,
        }
    }

    /// Convert GitHub API workflow run to our type
    fn convert_workflow_run(
        &self,
        run: GitHubWorkflowRun,
        interner: &StringInterner,
    ) -> WorkflowRun {
        let status = match run.status.as_str() {
            "queued" => WorkflowStatus::Queued,
            "in_progress" => WorkflowStatus::InProgress,
            _ => WorkflowStatus::Completed,
        };

        let conclusion = run.conclusion.as_ref().map(|c| match c.as_str() {
            "success" => WorkflowConclusion::Success,
            "failure" => WorkflowConclusion::Failure,
            "cancelled" => WorkflowConclusion::Cancelled,
            "skipped" => WorkflowConclusion::Skipped,
            "timed_out" => WorkflowConclusion::TimedOut,
            _ => WorkflowConclusion::Neutral,
        });

        // Parse ISO 8601 timestamp to Unix epoch
        let created_at = chrono::DateTime::parse_from_rfc3339(&run.created_at)
            .map(|dt| dt.timestamp())
            .unwrap_or(0);

        WorkflowRun {
            id: run.id,
            name: interner.intern(&run.name),
            status,
            conclusion,
            branch: interner.intern(&run.head_branch),
            head_sha: interner.intern(&run.head_sha),
            created_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workflow_api_client_creation() {
        let client = WorkflowApiClient::new("https://api.github.com".to_string(), None);
        assert_eq!(client.base_url, "https://api.github.com");
        assert!(client.token.is_none());
    }

    #[test]
    fn test_workflow_api_client_with_token() {
        let client = WorkflowApiClient::new(
            "https://api.github.com".to_string(),
            Some("test_token".to_string()),
        );
        assert_eq!(client.token, Some("test_token".to_string()));
    }

    #[test]
    fn test_workflow_status_conversion() {
        let interner = StringInterner::new();
        let client = WorkflowApiClient::new("https://api.github.com".to_string(), None);

        let run = GitHubWorkflowRun {
            id: 123,
            name: "CI".to_string(),
            status: "queued".to_string(),
            conclusion: None,
            head_branch: "main".to_string(),
            head_sha: "abc123".to_string(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        };

        let converted = client.convert_workflow_run(run, &interner);
        assert_eq!(converted.status, WorkflowStatus::Queued);
        assert_eq!(converted.conclusion, None);
    }

    #[test]
    fn test_workflow_conclusion_conversion() {
        let interner = StringInterner::new();
        let client = WorkflowApiClient::new("https://api.github.com".to_string(), None);

        let run = GitHubWorkflowRun {
            id: 456,
            name: "Test".to_string(),
            status: "completed".to_string(),
            conclusion: Some("failure".to_string()),
            head_branch: "feature".to_string(),
            head_sha: "def456".to_string(),
            created_at: "2024-01-01T12:00:00Z".to_string(),
        };

        let converted = client.convert_workflow_run(run, &interner);
        assert_eq!(converted.status, WorkflowStatus::Completed);
        assert_eq!(converted.conclusion, Some(WorkflowConclusion::Failure));
    }

    #[test]
    fn test_convert_workflow_job_success() {
        let interner = StringInterner::new();
        let client = WorkflowApiClient::new("https://api.github.com".to_string(), None);

        let job = GitHubWorkflowJob {
            id: 100,
            name: "build".to_string(),
            status: "completed".to_string(),
            conclusion: Some("success".to_string()),
            run_id: 1,
            started_at: Some("2024-01-01T10:00:00Z".to_string()),
            completed_at: Some("2024-01-01T10:05:00Z".to_string()),
        };

        let converted = client.convert_workflow_job(job, 1, &interner);
        assert_eq!(converted.id, 100);
        assert_eq!(converted.status, WorkflowStatus::Completed);
        assert_eq!(converted.conclusion, Some(WorkflowConclusion::Success));
        assert_eq!(converted.run_id, 1);
        assert!(converted.started_at > 0);
        assert!(converted.completed_at > 0);
        assert!(converted.completed_at > converted.started_at);
        assert_eq!(interner.resolve(converted.name), Some("build"));
    }

    #[test]
    fn test_convert_workflow_job_failure() {
        let interner = StringInterner::new();
        let client = WorkflowApiClient::new("https://api.github.com".to_string(), None);

        let job = GitHubWorkflowJob {
            id: 200,
            name: "test".to_string(),
            status: "completed".to_string(),
            conclusion: Some("failure".to_string()),
            run_id: 2,
            started_at: Some("2024-01-01T10:00:00Z".to_string()),
            completed_at: Some("2024-01-01T10:03:00Z".to_string()),
        };

        let converted = client.convert_workflow_job(job, 2, &interner);
        assert_eq!(converted.conclusion, Some(WorkflowConclusion::Failure));
    }

    #[test]
    fn test_convert_workflow_job_no_timestamps() {
        let interner = StringInterner::new();
        let client = WorkflowApiClient::new("https://api.github.com".to_string(), None);

        let job = GitHubWorkflowJob {
            id: 300,
            name: "pending".to_string(),
            status: "queued".to_string(),
            conclusion: None,
            run_id: 3,
            started_at: None,
            completed_at: None,
        };

        let converted = client.convert_workflow_job(job, 3, &interner);
        assert_eq!(converted.started_at, 0);
        assert_eq!(converted.completed_at, 0);
        assert_eq!(converted.status, WorkflowStatus::Queued);
        assert_eq!(converted.conclusion, None);
    }
}
