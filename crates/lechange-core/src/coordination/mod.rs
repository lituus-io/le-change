//! Main coordination logic

pub mod ci_decision;
pub mod processor;
pub mod workflow_tracker;

pub use ci_decision::CiDecisionEngine;
pub use processor::FileProcessor;
pub use workflow_tracker::WorkflowTracker;

use crate::error::{Error, Result};

/// Extract owner and repo from the GITHUB_REPOSITORY environment variable.
///
/// Shared utility used by both WorkflowTracker and FileProcessor.
pub fn extract_owner_repo() -> Result<(String, String)> {
    let repository = std::env::var("GITHUB_REPOSITORY")
        .map_err(|_| Error::Config("GITHUB_REPOSITORY not set".to_string()))?;

    let parts: Vec<&str> = repository.split('/').collect();
    if parts.len() != 2 {
        return Err(Error::Config(format!(
            "Invalid GITHUB_REPOSITORY format: {}",
            repository
        )));
    }

    Ok((parts[0].to_string(), parts[1].to_string()))
}
