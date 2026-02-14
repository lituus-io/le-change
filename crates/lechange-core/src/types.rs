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
    /// File origin tracking (current changes vs previous failures vs previous successes)
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

/// Processed result of the full detection pipeline (index-based partitioning)
#[derive(Debug, Default)]
pub struct ProcessedResult {
    /// All files from the diff (unfiltered superset)
    pub all_files: Vec<ChangedFile>,
    /// Indices into all_files matching pattern filter
    pub filtered_indices: Vec<u32>,
    /// Indices into all_files NOT matching pattern filter
    pub unmatched_indices: Vec<u32>,
    /// Whether a pattern filter was applied
    pub pattern_applied: bool,
    /// Per-YAML-group index results
    pub group_results: Vec<GroupResult>,
    /// Total additions (lines)
    pub additions: u32,
    /// Total deletions (lines)
    pub deletions: u32,
    /// Pipeline diagnostics (warnings, soft errors)
    pub diagnostics: Vec<Diagnostic>,
    /// Enhanced workflow check result
    pub workflow_result: Option<WorkflowCheckResult>,
    /// CI rebuild/skip decision
    pub ci_decision: Option<CiDecision>,
}

impl ProcessedResult {
    /// Get files matching the pattern filter
    pub fn matched_files(&self) -> Vec<&ChangedFile> {
        self.filtered_indices
            .iter()
            .map(|&i| &self.all_files[i as usize])
            .collect()
    }

    /// Get files NOT matching the pattern filter ("other" files)
    pub fn other_files(&self) -> Vec<&ChangedFile> {
        self.unmatched_indices
            .iter()
            .map(|&i| &self.all_files[i as usize])
            .collect()
    }

    /// Create from an unfiltered DiffResult (no pattern applied)
    pub fn from_unfiltered(diff: DiffResult) -> Self {
        let n = diff.files.len() as u32;
        Self {
            filtered_indices: (0..n).collect(),
            unmatched_indices: Vec::new(),
            pattern_applied: false,
            all_files: diff.files,
            group_results: Vec::new(),
            additions: diff.additions,
            deletions: diff.deletions,
            diagnostics: Vec::new(),
            workflow_result: None,
            ci_decision: None,
        }
    }
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

/// File origin - tracks whether a file is in current changes, failed workflows, successful workflows, or combinations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct FileOrigin {
    /// File is in current diff
    pub in_current_changes: bool,
    /// File was in previous workflow failure
    pub in_previous_failure: bool,
    /// File was in a successful prior workflow
    pub in_previous_success: bool,
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

/// Individual job within a workflow run
#[derive(Debug, Clone)]
pub struct WorkflowJob {
    /// Job ID
    pub id: u64,
    /// Job name (interned)
    pub name: InternedString,
    /// Job status
    pub status: WorkflowStatus,
    /// Job conclusion (if completed)
    pub conclusion: Option<WorkflowConclusion>,
    /// Parent workflow run ID
    pub run_id: u64,
    /// Started at (Unix epoch seconds)
    pub started_at: i64,
    /// Completed at (Unix epoch seconds)
    pub completed_at: i64,
}

/// Workflow failure context with affected files
#[derive(Debug)]
pub struct WorkflowFailure {
    /// The failed workflow run
    pub run: WorkflowRun,
    /// Files that were changed in the commit that failed
    pub files: Vec<InternedString>,
    /// Individual failed jobs (populated when track_job_level is true)
    pub failed_jobs: Vec<WorkflowJob>,
}

/// Successful workflow with its verified files
#[derive(Debug)]
pub struct WorkflowSuccess {
    /// The successful workflow run
    pub run: WorkflowRun,
    /// Individual job results (populated when track_job_level is true)
    pub jobs: Vec<WorkflowJob>,
    /// Files verified by this success
    pub files: Vec<InternedString>,
}

/// Result of workflow checking process
#[derive(Debug, Default)]
pub struct WorkflowCheckResult {
    /// Workflows currently running that overlap with our files
    pub blocking_runs: Vec<WorkflowRun>,
    /// Recent failures on this branch
    pub failures: Vec<WorkflowFailure>,
    /// Recent successes on this branch
    pub successes: Vec<WorkflowSuccess>,
    /// Did we wait for blocking workflows?
    pub waited: bool,
    /// Wait time in milliseconds
    pub wait_time_ms: u64,
}

/// CI rebuild/skip decision computed from workflow analysis
#[derive(Debug, Default)]
pub struct CiDecision {
    /// Files that need CI attention (current changes + previous failures - verified successes)
    pub files_to_rebuild: Vec<InternedString>,
    /// Files from prior commits verified as successful (skip these)
    pub files_to_skip: Vec<InternedString>,
    /// Job names that failed in recent workflows
    pub failed_jobs: Vec<InternedString>,
    /// Job names that succeeded in recent workflows
    pub successful_jobs: Vec<InternedString>,
    /// Per-file rebuild reason for debugging
    pub rebuild_reasons: Vec<RebuildReason>,
}

/// Reason why a file needs to be rebuilt
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RebuildReasonKind {
    /// File is in current diff
    NewChange,
    /// File was in a failed workflow
    PreviousFailure,
    /// Both new change and previous failure
    BothNewAndFailed,
}

/// Detailed rebuild reason for a single file
#[derive(Debug, Clone)]
pub struct RebuildReason {
    /// File path
    pub file: InternedString,
    /// Why this file needs rebuild
    pub kind: RebuildReasonKind,
    /// Which workflow run failed (if applicable)
    pub failed_run_id: Option<u64>,
    /// Which specific job failed (if applicable)
    pub failed_job_name: Option<InternedString>,
}

/// Pipeline diagnostic message
#[derive(Debug, Clone)]
pub struct Diagnostic {
    /// Severity level
    pub severity: DiagnosticSeverity,
    /// Category of the diagnostic
    pub category: DiagnosticCategory,
    /// Human-readable message
    pub message: String,
}

/// Diagnostic severity level
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DiagnosticSeverity {
    /// Non-fatal warning
    Warning,
    /// Soft error (recoverable)
    SoftError,
}

/// Diagnostic category for filtering
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DiagnosticCategory {
    /// Error during initial diff computation
    InitialDiff,
    /// Error during submodule diff
    SubmoduleDiff,
    /// Skipped because base and head SHA are the same
    SkippedSameSha,
    /// Shallow clone depth insufficient
    ShallowClone,
    /// Error loading pattern file
    PatternLoad,
    /// Error during symlink detection
    SymlinkDetection,
    /// Workflow API error (non-fatal)
    WorkflowApi,
}

/// Result of YAML group pattern matching
#[derive(Debug, Clone)]
pub struct GroupResult {
    /// Group key (from YAML)
    pub key: String,
    /// Indices into all_files that matched this group's patterns
    pub matched_indices: Vec<u32>,
}

/// Configuration input - parameters organized by category
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

    // Feature 1: YAML patterns
    /// YAML content with pattern groups
    pub files_yaml: Option<Cow<'a, str>>,
    /// Path to YAML file with pattern groups
    pub files_yaml_from_source_file: Option<Cow<'a, str>>,
    // Feature 2: Pattern source files
    /// Path to file containing patterns (one per line)
    pub files_from_source_file: Option<Cow<'a, str>>,
    /// Separator for source file patterns
    pub files_from_source_file_separator: Cow<'a, str>,

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

    // Feature 6: Directory extras
    /// Exclude current directory from dir_names output
    pub dir_names_exclude_current_dir: bool,
    /// Only include directories containing these files
    pub dir_names_include_files: Option<Vec<Cow<'a, str>>>,
    /// For deleted files, only include directories where all files are deleted
    pub dir_names_deleted_files_include_only_deleted_dirs: bool,

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

    // Feature 9: Tag comparison
    /// Pattern to match tags for comparison
    pub tags_pattern: Option<Cow<'a, str>>,
    /// Pattern to ignore tags
    pub tags_ignore_pattern: Option<Cow<'a, str>>,

    // Feature 11: Soft-fail
    /// Fail on initial diff error (default: true)
    pub fail_on_initial_diff_error: bool,
    /// Fail on submodule diff error (default: false)
    pub fail_on_submodule_diff_error: bool,
    /// Skip if base and head SHA are the same (default: false)
    pub skip_same_sha: bool,

    // Feature 15: Rename splitting
    /// Output renamed files as separate deleted + added entries
    pub output_renamed_as_deleted_added: bool,

    // Feature 16: POSIX path separator
    /// Force POSIX (forward slash) path separators in output
    pub use_posix_path_separator: bool,

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

    // Workflow intelligence (enhanced)
    /// Track at job level (individual job results)
    pub track_job_level: bool,
    /// Number of commits to look back for successful workflows
    pub workflow_success_lookback: u32,
    /// Skip files from successful prior workflows (default: true when track_workflow_failures)
    pub skip_successful_files: bool,
    /// Glob pattern to match specific workflow names
    pub workflow_name_filter: Option<Cow<'a, str>>,
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
            files_yaml: None,
            files_yaml_from_source_file: None,
            files_from_source_file: None,
            files_from_source_file_separator: Cow::Borrowed("\n"),
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
            dir_names_exclude_current_dir: false,
            dir_names_include_files: None,
            dir_names_deleted_files_include_only_deleted_dirs: false,
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
            tags_pattern: None,
            tags_ignore_pattern: None,
            fail_on_initial_diff_error: true,
            fail_on_submodule_diff_error: false,
            skip_same_sha: false,
            output_renamed_as_deleted_added: false,
            use_posix_path_separator: false,
            track_workflow_failures: false,
            workflow_lookback_commits: 5,
            wait_for_active_workflows: true,
            workflow_max_wait_seconds: 300,
            include_failed_files: true,
            track_job_level: false,
            workflow_success_lookback: 5,
            skip_successful_files: true,
            workflow_name_filter: None,
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

    #[test]
    fn test_file_origin_default() {
        let origin = FileOrigin::default();
        assert!(!origin.in_current_changes);
        assert!(!origin.in_previous_failure);
        assert!(!origin.in_previous_success);
    }

    #[test]
    fn test_processed_result_from_unfiltered() {
        let diff = DiffResult {
            files: vec![
                ChangedFile {
                    path: InternedString(0),
                    change_type: ChangeType::Added,
                    previous_path: None,
                    is_symlink: false,
                    submodule_depth: 0,
                    origin: FileOrigin::default(),
                },
                ChangedFile {
                    path: InternedString(1),
                    change_type: ChangeType::Modified,
                    previous_path: None,
                    is_symlink: false,
                    submodule_depth: 0,
                    origin: FileOrigin::default(),
                },
            ],
            additions: 10,
            deletions: 5,
        };

        let result = ProcessedResult::from_unfiltered(diff);
        assert_eq!(result.all_files.len(), 2);
        assert_eq!(result.filtered_indices, vec![0, 1]);
        assert!(result.unmatched_indices.is_empty());
        assert!(!result.pattern_applied);
        assert_eq!(result.additions, 10);
        assert_eq!(result.deletions, 5);
    }

    #[test]
    fn test_processed_result_accessors() {
        let result = ProcessedResult {
            all_files: vec![
                ChangedFile {
                    path: InternedString(0),
                    change_type: ChangeType::Added,
                    previous_path: None,
                    is_symlink: false,
                    submodule_depth: 0,
                    origin: FileOrigin::default(),
                },
                ChangedFile {
                    path: InternedString(1),
                    change_type: ChangeType::Modified,
                    previous_path: None,
                    is_symlink: false,
                    submodule_depth: 0,
                    origin: FileOrigin::default(),
                },
                ChangedFile {
                    path: InternedString(2),
                    change_type: ChangeType::Deleted,
                    previous_path: None,
                    is_symlink: false,
                    submodule_depth: 0,
                    origin: FileOrigin::default(),
                },
            ],
            filtered_indices: vec![0, 2],
            unmatched_indices: vec![1],
            pattern_applied: true,
            group_results: Vec::new(),
            additions: 0,
            deletions: 0,
            diagnostics: Vec::new(),
            workflow_result: None,
            ci_decision: None,
        };

        assert_eq!(result.matched_files().len(), 2);
        assert_eq!(result.other_files().len(), 1);
        assert_eq!(result.matched_files()[0].change_type, ChangeType::Added);
        assert_eq!(result.matched_files()[1].change_type, ChangeType::Deleted);
        assert_eq!(result.other_files()[0].change_type, ChangeType::Modified);
    }

    #[test]
    fn test_ci_decision_default() {
        let decision = CiDecision::default();
        assert!(decision.files_to_rebuild.is_empty());
        assert!(decision.files_to_skip.is_empty());
        assert!(decision.failed_jobs.is_empty());
        assert!(decision.successful_jobs.is_empty());
        assert!(decision.rebuild_reasons.is_empty());
    }
}
