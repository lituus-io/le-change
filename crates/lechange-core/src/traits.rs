//! GATs trait definitions for zero-cost async operations

use std::future::Future;
use crate::types::DiffResult;
use crate::interner::StringInterner;

/// Git operations trait using GATs for zero-cost async
/// No boxing, no dynamic dispatch - pure compile-time polymorphism
pub trait AsyncGitOps {
    /// Error type for git operations
    type Error: std::error::Error + Send + Sync + 'static;

    /// GAT for async diff operation
    type DiffFuture<'a>: Future<Output = std::result::Result<DiffResult, Self::Error>> + Send + 'a
    where
        Self: 'a;

    /// GAT for async SHA resolution
    type ResolveShaFuture<'a>: Future<Output = std::result::Result<String, Self::Error>> + Send + 'a
    where
        Self: 'a;

    /// GAT for async submodule listing
    type SubmodulesFuture<'a>: Future<Output = std::result::Result<Vec<String>, Self::Error>> + Send + 'a
    where
        Self: 'a;

    /// Compute diff between two SHAs
    fn diff<'a>(
        &'a self,
        base_sha: &'a str,
        head_sha: &'a str,
        interner: &'a StringInterner,
        diff_filter: &'a str,
    ) -> Self::DiffFuture<'a>;

    /// Resolve a reference to a SHA
    fn resolve_sha<'a>(&'a self, reference: &'a str) -> Self::ResolveShaFuture<'a>;

    /// Get list of submodule paths
    fn submodules<'a>(&'a self) -> Self::SubmodulesFuture<'a>;
}

/// Pattern matching trait with GATs
pub trait AsyncPatternMatcher {
    /// Error type for pattern operations
    type Error: std::error::Error + Send + Sync + 'static;

    /// GAT for async match operation
    type MatchFuture<'a>: Future<Output = std::result::Result<bool, Self::Error>> + Send + 'a
    where
        Self: 'a;

    /// Check if a path matches any pattern (async)
    fn matches<'a>(&'a self, path: &'a str) -> Self::MatchFuture<'a>;

    /// Synchronous variant for parallel processing with rayon
    fn matches_sync(&self, path: &str) -> std::result::Result<bool, Self::Error>;
}

/// File operations trait with GATs
pub trait AsyncFileOps {
    /// Error type for file operations
    type Error: std::error::Error + Send + Sync + 'static;

    /// GAT for async symlink check
    type IsSymlinkFuture<'a>: Future<Output = std::result::Result<bool, Self::Error>> + Send + 'a
    where
        Self: 'a;

    /// Check if a path is a symlink using lstat (async)
    fn is_symlink<'a>(&'a self, path: &'a std::path::Path) -> Self::IsSymlinkFuture<'a>;

    /// Synchronous variant for parallel processing
    fn is_symlink_sync(&self, path: &std::path::Path) -> std::result::Result<bool, Self::Error>;
}
