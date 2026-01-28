//! Core type definitions with zero-copy design

use std::borrow::Cow;

/// Interned string handle - just an index into the interner
///
/// This is Copy-able and has zero overhead
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InternedString(pub(crate) u32);

/// Change type for a file (matches git diff output)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// Configuration input - 64 parameters organized by category
#[derive(Debug, Clone)]
pub struct InputConfig<'a> {
    // Git references
    pub base_sha: Option<Cow<'a, str>>,
    pub sha: Option<Cow<'a, str>>,
    pub since: Option<Cow<'a, str>>,
    pub until: Option<Cow<'a, str>>,

    // Pattern filtering
    pub files: Option<Vec<Cow<'a, str>>>,
    pub files_separator: Cow<'a, str>,
    pub files_ignore: Option<Vec<Cow<'a, str>>>,
    pub files_ignore_separator: Cow<'a, str>,

    // Diff configuration
    pub diff_filter: Cow<'a, str>,
    pub include_all_old_new_renamed_files: bool,
    pub old_new_separator: Cow<'a, str>,
    pub old_new_files_separator: Cow<'a, str>,

    // Path handling
    pub dir_names: bool,
    pub dir_names_max_depth: Option<u32>,
    pub quotepath: bool,
    pub path_separator: Cow<'a, str>,

    // Submodules
    pub include_submodules: bool,
    pub submodule_filter: Option<Cow<'a, str>>,

    // Fetch configuration
    pub fetch_depth: u32,
    pub fetch_additional_submodule_history: bool,

    // Output options
    pub json: bool,
    pub escape_json: bool,
    pub safe_output: bool,
    pub output_dir: Option<Cow<'a, str>>,

    // Performance tuning
    pub skip_initial_fetch: bool,
    pub use_rest_api: bool,
    pub api_url: Option<Cow<'a, str>>,
    pub token: Option<Cow<'a, str>>,

    // Advanced options
    pub write_output_files: bool,
    pub negation_patterns_first: bool,
    pub match_gitignore_files: bool,
    pub recover_deleted_files: bool,
    pub exclude_symlinks: bool,
    pub sha256: Option<Cow<'a, str>>,
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
    fn test_input_config_default() {
        let config = InputConfig::default();
        assert_eq!(config.diff_filter, "ACDMRTUX");
        assert_eq!(config.files_separator, "\n");
        assert!(!config.json);
        assert!(config.quotepath);
    }
}
