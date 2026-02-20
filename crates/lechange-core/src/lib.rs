//! # LeChange Core
//!
//! Ultra-fast Git change detection library with zero-cost abstractions.
//!
//! This library provides high-performance git diff operations using:
//! - **GATs (Generic Associated Types)** for zero-cost async
//! - **Lifetimes over Arc** for zero-copy string handling
//! - **String interning** for path deduplication
//! - **Rayon** for CPU-bound parallel processing
//! - **Tokio** for async I/O operations
//!
//! ## Example
//!
//! ```no_run
//! use lechange_core::{InputConfig, detect_changes};
//! use std::borrow::Cow;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = InputConfig {
//!     base_sha: Some(Cow::Borrowed("HEAD^")),
//!     sha: Some(Cow::Borrowed("HEAD")),
//!     ..Default::default()
//! };
//!
//! let result = detect_changes(config).await?;
//! println!("Changed files: {}", result.all_files.len());
//! # Ok(())
//! # }
//! ```

#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
#![warn(missing_docs, rust_2018_idioms)]

pub mod coordination;
pub mod error;
pub mod file_ops;
pub mod git;
pub mod http;
pub mod interner;
pub mod output;
pub mod patterns;
pub mod platform;
pub mod traits;
pub mod types;

pub use error::{Error, Result};
pub use interner::StringInterner;
pub use types::{
    ChangeType, ChangedFile, CiDecision, DiffResult, FailureTrackingLevel, GroupDeployAction,
    GroupDeployDecision, GroupDeployReason, InputConfig, InternedString, ProcessedResult,
};

/// Detect changed files between two git references
///
/// This is the main entry point for the library. It handles:
/// - SHA resolution
/// - Diff computation
/// - Pattern filtering
/// - Submodule processing
/// - Symlink detection
/// - Workflow intelligence
///
/// Returns a `ProcessedResult` with index-based partitioning for both
/// filtered and unfiltered file sets.
///
/// # Example
///
/// ```no_run
/// use lechange_core::{InputConfig, detect_changes};
/// use std::borrow::Cow;
///
/// # async fn example() -> lechange_core::Result<()> {
/// let config = InputConfig {
///     base_sha: Some(Cow::Borrowed("main")),
///     sha: Some(Cow::Borrowed("HEAD")),
///     ..Default::default()
/// };
///
/// let result = detect_changes(config).await?;
/// println!("Files changed: {}", result.all_files.len());
/// # Ok(())
/// # }
/// ```
pub async fn detect_changes(config: InputConfig<'_>) -> Result<ProcessedResult> {
    // Initialize string interner with reasonable capacity
    let interner = StringInterner::with_capacity(2048);

    // Open git repository
    let repo = git::repository::GitRepository::discover(".")?;

    // Ensure sufficient depth if configured
    if config.fetch_depth > 0 {
        repo.ensure_depth(config.fetch_depth).await?;
    }

    // Create processor and run
    let processor = coordination::processor::FileProcessor::new(&repo, &interner, &config);
    let result = processor.process().await?;

    Ok(result)
}

/// Synchronous variant of `detect_changes`
///
/// This creates a new Tokio runtime and blocks on the async version.
/// Prefer the async version if you're already in an async context.
pub fn detect_changes_sync(config: InputConfig<'_>) -> Result<ProcessedResult> {
    tokio::runtime::Runtime::new()
        .map_err(|e| Error::Runtime(e.to_string()))?
        .block_on(detect_changes(config))
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_library_version() {
        // Smoke test to ensure library compiles
        let _ = env!("CARGO_PKG_VERSION");
    }
}
