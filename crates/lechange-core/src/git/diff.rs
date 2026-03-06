//! Git diff parsing with zero-copy

use crate::interner::StringInterner;
use crate::types::{ChangeType, ChangedFile};

/// Zero-copy git diff parser
pub struct DiffParser<'a> {
    interner: &'a StringInterner,
}

impl<'a> DiffParser<'a> {
    /// Create a new diff parser
    pub fn new(interner: &'a StringInterner) -> Self {
        Self { interner }
    }

    /// Parse a single diff line with zero allocations
    /// Format: "M\tpath/to/file.rs" or "R100\told/path\tnew/path"
    pub fn parse_diff_line(&self, line: &[u8]) -> Option<ChangedFile> {
        use memchr::memchr;

        // Find first tab character
        let tab_pos = memchr(b'\t', line)?;

        // Parse change type from first character
        let change_type = ChangeType::from_byte(line[0])?;

        // For renames/copies, there are two paths
        match change_type {
            ChangeType::Renamed | ChangeType::Copied => {
                // Find second tab
                let second_tab_offset = memchr(b'\t', &line[tab_pos + 1..])?;
                let second_tab = tab_pos + 1 + second_tab_offset;

                // SAFETY: Git output is UTF-8
                let old_path = std::str::from_utf8(&line[tab_pos + 1..second_tab]).ok()?;
                let new_path = std::str::from_utf8(&line[second_tab + 1..]).ok()?;

                Some(ChangedFile {
                    path: self.interner.intern(new_path.trim()),
                    change_type,
                    previous_path: Some(self.interner.intern(old_path.trim())),
                    is_symlink: false, // Determined later
                    submodule_depth: 0,
                    origin: crate::types::FileOrigin {
                        in_current_changes: true,
                        in_previous_failure: false,
                        in_previous_success: false,
                    },
                })
            }
            _ => {
                // SAFETY: Git output is UTF-8
                let path = std::str::from_utf8(&line[tab_pos + 1..]).ok()?;

                Some(ChangedFile {
                    path: self.interner.intern(path.trim()),
                    change_type,
                    previous_path: None,
                    is_symlink: false,
                    submodule_depth: 0,
                    origin: crate::types::FileOrigin {
                        in_current_changes: true,
                        in_previous_failure: false,
                        in_previous_success: false,
                    },
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_modified() {
        let interner = StringInterner::new();
        let parser = DiffParser::new(&interner);

        let line = b"M\tsrc/main.rs";
        let file = parser.parse_diff_line(line).unwrap();

        assert_eq!(file.change_type, ChangeType::Modified);
        assert_eq!(interner.resolve(file.path), Some("src/main.rs"));
        assert!(file.previous_path.is_none());
    }

    #[test]
    fn test_parse_renamed() {
        let interner = StringInterner::new();
        let parser = DiffParser::new(&interner);

        let line = b"R100\told/path.rs\tnew/path.rs";
        let file = parser.parse_diff_line(line).unwrap();

        assert_eq!(file.change_type, ChangeType::Renamed);
        assert_eq!(interner.resolve(file.path), Some("new/path.rs"));
        assert_eq!(
            interner.resolve(file.previous_path.unwrap()),
            Some("old/path.rs")
        );
    }
}
