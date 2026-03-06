//! Directory name extraction from file paths

use crate::interner::StringInterner;
use crate::types::{ChangeType, ChangedFile, InternedString};
use std::collections::HashSet;

/// Extract unique directory names from file paths
pub struct DirNameExtractor<'a> {
    interner: &'a StringInterner,
}

impl<'a> DirNameExtractor<'a> {
    /// Create a new directory name extractor
    pub fn new(interner: &'a StringInterner) -> Self {
        Self { interner }
    }

    /// Extract directory names from files with depth limiting
    ///
    /// Uses zero-copy `rfind('/')` for directory extraction.
    pub fn extract(
        &self,
        files: &[ChangedFile],
        indices: &[u32],
        max_depth: Option<u32>,
        exclude_current_dir: bool,
        include_files: Option<&[&str]>,
        deleted_only_dirs: bool,
    ) -> Vec<InternedString> {
        let mut dirs: HashSet<InternedString> = HashSet::new();

        // If deleted_only_dirs, collect all directories and check if all files in each are deleted
        if deleted_only_dirs {
            return self.extract_deleted_only_dirs(files, indices, max_depth);
        }

        for &idx in indices {
            let file = &files[idx as usize];

            if let Some(path) = self.interner.resolve(file.path) {
                // Skip if include_files is set and path doesn't match
                if let Some(patterns) = include_files {
                    let matches = patterns.iter().any(|p| path.contains(p));
                    if !matches {
                        continue;
                    }
                }

                if let Some(dir) = self.extract_dir(path, max_depth, exclude_current_dir) {
                    dirs.insert(self.interner.intern(dir));
                }
            }
        }

        dirs.into_iter().collect()
    }

    /// Extract directory from path with depth limiting
    fn extract_dir<'b>(
        &self,
        path: &'b str,
        max_depth: Option<u32>,
        exclude_current_dir: bool,
    ) -> Option<&'b str> {
        // Find directory component via rfind
        let dir = match path.rfind('/') {
            Some(pos) => &path[..pos],
            None => {
                // File is in root directory
                if exclude_current_dir {
                    return None;
                }
                return Some(".");
            }
        };

        if exclude_current_dir && dir == "." {
            return None;
        }

        // Apply depth limiting
        if let Some(max_depth) = max_depth {
            let depth = dir.matches('/').count() as u32 + 1;
            if depth > max_depth {
                // Truncate to max_depth
                let mut slash_count = 0;
                for (i, ch) in dir.char_indices() {
                    if ch == '/' {
                        slash_count += 1;
                        if slash_count >= max_depth {
                            return Some(&dir[..i]);
                        }
                    }
                }
            }
        }

        Some(dir)
    }

    /// Extract directories where ALL files are deleted
    fn extract_deleted_only_dirs(
        &self,
        files: &[ChangedFile],
        indices: &[u32],
        max_depth: Option<u32>,
    ) -> Vec<InternedString> {
        use std::collections::HashMap;

        // Group files by directory
        let mut dir_files: HashMap<String, (usize, usize)> = HashMap::new(); // (total, deleted)

        for &idx in indices {
            let file = &files[idx as usize];
            if let Some(path) = self.interner.resolve(file.path) {
                let dir = match path.rfind('/') {
                    Some(pos) => path[..pos].to_string(),
                    None => ".".to_string(),
                };

                let entry = dir_files.entry(dir).or_insert((0, 0));
                entry.0 += 1;
                if file.change_type == ChangeType::Deleted {
                    entry.1 += 1;
                }
            }
        }

        // Only include directories where all files are deleted
        let mut result = Vec::new();
        for (dir, (total, deleted)) in dir_files {
            if total == deleted && total > 0 {
                // Apply depth limiting
                let truncated = if let Some(max) = max_depth {
                    let parts: Vec<&str> = dir.split('/').collect();
                    if parts.len() > max as usize {
                        parts[..max as usize].join("/")
                    } else {
                        dir
                    }
                } else {
                    dir
                };
                result.push(self.interner.intern(&truncated));
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FileOrigin;

    fn make_file(interner: &StringInterner, path: &str, change_type: ChangeType) -> ChangedFile {
        ChangedFile {
            path: interner.intern(path),
            change_type,
            previous_path: None,
            is_symlink: false,
            submodule_depth: 0,
            origin: FileOrigin::default(),
        }
    }

    #[test]
    fn test_extract_basic() {
        let interner = StringInterner::new();
        let files = vec![
            make_file(&interner, "src/main.rs", ChangeType::Modified),
            make_file(&interner, "src/lib.rs", ChangeType::Added),
            make_file(&interner, "tests/test.rs", ChangeType::Modified),
        ];
        let indices: Vec<u32> = (0..files.len() as u32).collect();

        let extractor = DirNameExtractor::new(&interner);
        let dirs = extractor.extract(&files, &indices, None, false, None, false);

        let dir_names: HashSet<String> = dirs
            .iter()
            .filter_map(|d| interner.resolve(*d).map(String::from))
            .collect();

        assert!(dir_names.contains("src"));
        assert!(dir_names.contains("tests"));
    }

    #[test]
    fn test_extract_with_depth() {
        let interner = StringInterner::new();
        let files = vec![make_file(&interner, "a/b/c/file.rs", ChangeType::Modified)];
        let indices = vec![0u32];

        let extractor = DirNameExtractor::new(&interner);
        let dirs = extractor.extract(&files, &indices, Some(2), false, None, false);

        let dir_names: Vec<String> = dirs
            .iter()
            .filter_map(|d| interner.resolve(*d).map(String::from))
            .collect();

        assert_eq!(dir_names.len(), 1);
        assert_eq!(dir_names[0], "a/b");
    }

    #[test]
    fn test_extract_deleted_only_dirs() {
        let interner = StringInterner::new();
        let files = vec![
            // dir "old" has all files deleted
            make_file(&interner, "old/a.rs", ChangeType::Deleted),
            make_file(&interner, "old/b.rs", ChangeType::Deleted),
            // dir "mixed" has some deleted, some modified
            make_file(&interner, "mixed/a.rs", ChangeType::Deleted),
            make_file(&interner, "mixed/b.rs", ChangeType::Modified),
        ];
        let indices: Vec<u32> = (0..files.len() as u32).collect();

        let extractor = DirNameExtractor::new(&interner);
        let dirs = extractor.extract(&files, &indices, None, false, None, true);

        let dir_names: HashSet<String> = dirs
            .iter()
            .filter_map(|d| interner.resolve(*d).map(String::from))
            .collect();

        assert!(dir_names.contains("old"));
        assert!(!dir_names.contains("mixed"));
    }

    #[test]
    fn test_extract_deleted_only_dirs_with_depth() {
        let interner = StringInterner::new();
        let files = vec![make_file(&interner, "a/b/c/file.rs", ChangeType::Deleted)];
        let indices = vec![0u32];

        let extractor = DirNameExtractor::new(&interner);
        let dirs = extractor.extract(&files, &indices, Some(2), false, None, true);

        let dir_names: Vec<String> = dirs
            .iter()
            .filter_map(|d| interner.resolve(*d).map(String::from))
            .collect();

        assert_eq!(dir_names.len(), 1);
        assert_eq!(dir_names[0], "a/b");
    }

    #[test]
    fn test_extract_exclude_current_dir() {
        let interner = StringInterner::new();
        let files = vec![
            make_file(&interner, "root_file.rs", ChangeType::Modified),
            make_file(&interner, "src/main.rs", ChangeType::Modified),
        ];
        let indices: Vec<u32> = (0..files.len() as u32).collect();

        let extractor = DirNameExtractor::new(&interner);
        let dirs = extractor.extract(&files, &indices, None, true, None, false);

        let dir_names: HashSet<String> = dirs
            .iter()
            .filter_map(|d| interner.resolve(*d).map(String::from))
            .collect();

        // Root file excluded with exclude_current_dir=true
        assert!(!dir_names.contains("."));
        assert!(dir_names.contains("src"));
    }

    #[test]
    fn test_extract_include_files_filter() {
        let interner = StringInterner::new();
        let files = vec![
            make_file(&interner, "src/main.rs", ChangeType::Modified),
            make_file(&interner, "src/lib.rs", ChangeType::Modified),
            make_file(&interner, "tests/test.py", ChangeType::Modified),
        ];
        let indices: Vec<u32> = (0..files.len() as u32).collect();

        let extractor = DirNameExtractor::new(&interner);
        let include = vec![".rs"];
        let dirs = extractor.extract(&files, &indices, None, false, Some(&include), false);

        let dir_names: HashSet<String> = dirs
            .iter()
            .filter_map(|d| interner.resolve(*d).map(String::from))
            .collect();

        assert!(dir_names.contains("src"));
        // tests dir should not appear because .py doesn't match .rs filter
        assert!(!dir_names.contains("tests"));
    }
}
