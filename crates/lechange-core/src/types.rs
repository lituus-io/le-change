//! Core type definitions with zero-copy design

use std::borrow::Cow;

/// Interned string handle - just an index into the interner
///
/// This is Copy-able and has zero overhead
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InternedString(pub(crate) u32);

/// Change type for a file (matches git diff output)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ChangeType {
    /// Added file
    Added = b'A',
    /// Copied file
    Copied = b'C',
    /// Deleted file
    Deleted = b'D',
    /// Modified file
    Modified = b'M',
    /// Renamed file
    Renamed = b'R',
    /// Type changed (permissions/mode)
    TypeChanged = b'T',
    /// Unmerged (conflict)
    Unmerged = b'U',
    /// Unknown change type
    Unknown = b'X',
}

impl ChangeType {
    /// Parse from git diff-filter character - zero allocation
    #[inline]
    pub const fn from_byte(b: u8) -> Option<Self> {
        match b {
            b'A' => Some(Self::Added),
            b'C' => Some(Self::Copied),
            b'D' => Some(Self::Deleted),
            b'M' => Some(Self::Modified),
            b'R' => Some(Self::Renamed),
            b'T' => Some(Self::TypeChanged),
            b'U' => Some(Self::Unmerged),
            b'X' => Some(Self::Unknown),
            _ => None,
        }
    }

    /// Convert to git diff-filter byte character
    #[inline]
    pub const fn as_byte(&self) -> u8 {
        match self {
            Self::Added => b'A',
            Self::Copied => b'C',
            Self::Deleted => b'D',
            Self::Modified => b'M',
            Self::Renamed => b'R',
            Self::TypeChanged => b'T',
            Self::Unmerged => b'U',
            Self::Unknown => b'X',
        }
    }

    /// Get string representation
    #[inline]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Copied => "copied",
            Self::Deleted => "deleted",
            Self::Modified => "modified",
            Self::Renamed => "renamed",
            Self::TypeChanged => "type_changed",
            Self::Unmerged => "unmerged",
            Self::Unknown => "unknown",
        }
    }
}

/// A single changed file entry with minimal allocations
#[derive(Debug, Clone)]
pub struct ChangedFile {
    /// Path - interned for deduplication
    pub path: InternedString,
    /// Change type
    pub change_type: ChangeType,
    /// Previous path for renames/copies (also interned)
    pub previous_path: Option<InternedString>,
    /// Is this a symlink?
    pub is_symlink: bool,
    /// Submodule depth (0 = root)
    pub submodule_depth: u8,
    /// File origin tracking (current changes vs previous failures)
    pub origin: FileOrigin,
}

/// Result of a diff operation - owns minimal data
#[derive(Debug, Default)]
pub struct DiffResult {
    /// All changed files
    pub files: Vec<ChangedFile>,
    /// Total additions (lines)
    pub additions: u32,
    /// Total deletions (lines)
    pub deletions: u32,
}

/// Workflow run status from GitHub Actions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum WorkflowStatus {
    /// Workflow is queued
    Queued,
    /// Workflow is in progress
    InProgress,
    /// Workflow completed (check conclusion for pass/fail)
    Completed,
}

/// Workflow run conclusion (only valid when status = Completed)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum WorkflowConclusion {
    /// Workflow succeeded
    Success,
    /// Workflow failed
    Failure,
    /// Workflow was cancelled
    Cancelled,
    /// Workflow was skipped
    Skipped,
    /// Workflow timed out
    TimedOut,
    /// Other/unknown
    Neutral,
}

/// File origin - tracks whether a file is in current changes, failed workflows, or both
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct FileOrigin {
    /// File is in current diff
    pub in_current_changes: bool,
    /// File was in previous workflow failure
    pub in_previous_failure: bool,
}

/// Workflow run metadata (minimal, following zero-copy design)
#[derive(Debug, Clone)]
pub struct WorkflowRun {
    /// Workflow run ID (GitHub API)
    pub id: u64,
    /// Workflow name (interned string)
    pub name: InternedString,
    /// Status
    pub status: WorkflowStatus,
    /// Conclusion (if completed)
    pub conclusion: Option<WorkflowConclusion>,
    /// Branch (interned string)
    pub branch: InternedString,
    /// Head SHA (interned string)
    pub head_sha: InternedString,
    /// Commit timestamp (Unix epoch seconds)
    pub created_at: i64,
}

/// Workflow failure context with affected files
#[derive(Debug)]
pub struct WorkflowFailure {
    /// The failed workflow run
    pub run: WorkflowRun,
    /// Files that were changed in the commit that failed
    pub files: Vec<InternedString>,
}

/// Result of workflow checking process
#[derive(Debug, Default)]
pub struct WorkflowCheckResult {
    /// Workflows currently running that overlap with our files
    pub blocking_runs: Vec<WorkflowRun>,
    /// Recent failures on this branch
    pub failures: Vec<WorkflowFailure>,
    /// Did we wait for blocking workflows?
    pub waited: bool,
    /// Wait time in milliseconds
    pub wait_time_ms: u64,
}

/// Configuration input - 64 parameters organized by category
#[derive(Debug, Clone)]
pub struct InputConfig<'a> {
    // Git references
    /// Base commit SHA for comparison
    pub base_sha: Option<Cow<'a, str>>,
    /// Current commit SHA
    pub sha: Option<Cow<'a, str>>,
    /// Include changes since this date
    pub since: Option<Cow<'a, str>>,
    /// Include changes until this date
    pub until: Option<Cow<'a, str>>,

    // Pattern filtering
    /// Glob patterns to include files
    pub files: Option<Vec<Cow<'a, str>>>,
    /// Separator for files output
    pub files_separator: Cow<'a, str>,
    /// Glob patterns to exclude files
    pub files_ignore: Option<Vec<Cow<'a, str>>>,
    /// Separator for ignored files output
    pub files_ignore_separator: Cow<'a, str>,

    // Diff configuration
    /// Git diff filter (ACDMRTUX)
    pub diff_filter: Cow<'a, str>,
    /// Include both old and new paths for renamed files
    pub include_all_old_new_renamed_files: bool,
    /// Separator between old and new paths
    pub old_new_separator: Cow<'a, str>,
    /// Separator for old/new files list
    pub old_new_files_separator: Cow<'a, str>,

    // Path handling
    /// Output directory names instead of files
    pub dir_names: bool,
    /// Maximum depth for directory names
    pub dir_names_max_depth: Option<u32>,
    /// Enable git quotepath
    pub quotepath: bool,
    /// Path separator for output
    pub path_separator: Cow<'a, str>,

    // Submodules
    /// Include submodule changes
    pub include_submodules: bool,
    /// Filter for submodule paths
    pub submodule_filter: Option<Cow<'a, str>>,

    // Fetch configuration
    /// Git fetch depth
    pub fetch_depth: u32,
    /// Fetch additional submodule history
    pub fetch_additional_submodule_history: bool,

    // Output options
    /// Output as JSON
    pub json: bool,
    /// Escape JSON special characters
    pub escape_json: bool,
    /// Enable safe output mode
    pub safe_output: bool,
    /// Output directory for file dumps
    pub output_dir: Option<Cow<'a, str>>,

    // Performance tuning
    /// Skip initial fetch operation
    pub skip_initial_fetch: bool,
    /// Use GitHub REST API instead of git
    pub use_rest_api: bool,
    /// API URL override
    pub api_url: Option<Cow<'a, str>>,
    /// GitHub API token
    pub token: Option<Cow<'a, str>>,

    // Advanced options
    /// Write output to files
    pub write_output_files: bool,
    /// Process negation patterns first
    pub negation_patterns_first: bool,
    /// Match .gitignore files
    pub match_gitignore_files: bool,
    /// Recover deleted file contents
    pub recover_deleted_files: bool,
    /// Exclude symbolic links
    pub exclude_symlinks: bool,
    /// SHA256 hash for verification
    pub sha256: Option<Cow<'a, str>>,

    // Workflow failure tracking
    /// Enable workflow failure tracking
    pub track_workflow_failures: bool,
    /// Number of commits to look back for failed workflows (default: 5)
    pub workflow_lookback_commits: u32,
    /// Check for active workflows on same files and wait (default: true)
    pub wait_for_active_workflows: bool,
    /// Maximum wait time for active workflows in seconds (default: 300 = 5 min)
    pub workflow_max_wait_seconds: u32,
    /// Include failed files in incremental CI output (default: true)
    pub include_failed_files: bool,
}

impl<'a> Default for InputConfig<'a> {
    fn default() -> Self {
        Self {
            base_sha: None,
            sha: None,
            since: None,
            until: None,
            files: None,
            files_separator: Cow::Borrowed("\n"),
            files_ignore: None,
            files_ignore_separator: Cow::Borrowed("\n"),
            diff_filter: Cow::Borrowed("ACDMRTUX"),
            include_all_old_new_renamed_files: false,
            old_new_separator: Cow::Borrowed(" "),
            old_new_files_separator: Cow::Borrowed("\n"),
            dir_names: false,
            dir_names_max_depth: None,
            quotepath: true,
            path_separator: if cfg!(windows) {
                Cow::Borrowed("\\")
            } else {
                Cow::Borrowed("/")
            },
            include_submodules: false,
            submodule_filter: None,
            fetch_depth: 0,
            fetch_additional_submodule_history: false,
            json: false,
            escape_json: true,
            safe_output: true,
            output_dir: None,
            skip_initial_fetch: false,
            use_rest_api: false,
            api_url: None,
            token: None,
            write_output_files: false,
            negation_patterns_first: true,
            match_gitignore_files: false,
            recover_deleted_files: false,
            exclude_symlinks: false,
            sha256: None,
            track_workflow_failures: false,
            workflow_lookback_commits: 5,
            wait_for_active_workflows: true,
            workflow_max_wait_seconds: 300,
            include_failed_files: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_change_type_from_byte() {
        assert_eq!(ChangeType::from_byte(b'A'), Some(ChangeType::Added));
        assert_eq!(ChangeType::from_byte(b'M'), Some(ChangeType::Modified));
        assert_eq!(ChangeType::from_byte(b'D'), Some(ChangeType::Deleted));
        assert_eq!(ChangeType::from_byte(b'R'), Some(ChangeType::Renamed));
        assert_eq!(ChangeType::from_byte(b'Z'), None);
    }

    #[test]
    fn test_change_type_as_str() {
        assert_eq!(ChangeType::Added.as_str(), "added");
        assert_eq!(ChangeType::Modified.as_str(), "modified");
        assert_eq!(ChangeType::Deleted.as_str(), "deleted");
    }

    #[test]
    fn test_change_type_as_byte() {
        assert_eq!(ChangeType::Added.as_byte(), b'A');
        assert_eq!(ChangeType::Modified.as_byte(), b'M');
        assert_eq!(ChangeType::Deleted.as_byte(), b'D');
        assert_eq!(ChangeType::Renamed.as_byte(), b'R');
    }

    #[test]
    fn test_change_type_roundtrip() {
        let types = [
            ChangeType::Added,
            ChangeType::Copied,
            ChangeType::Deleted,
            ChangeType::Modified,
            ChangeType::Renamed,
            ChangeType::TypeChanged,
            ChangeType::Unmerged,
            ChangeType::Unknown,
        ];

        for change_type in &types {
            let byte = change_type.as_byte();
            let parsed = ChangeType::from_byte(byte);
            assert_eq!(parsed, Some(*change_type));
        }
    }

    #[test]
    fn test_input_config_default() {
        let config = InputConfig::default();
        assert_eq!(config.diff_filter, "ACDMRTUX");
        assert_eq!(config.files_separator, "\n");
        assert!(!config.json);
        assert!(config.quotepath);
    }
}
