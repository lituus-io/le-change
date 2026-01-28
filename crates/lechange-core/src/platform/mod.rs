//! Platform-specific utilities

use std::path::MAIN_SEPARATOR;

/// Platform-aware path utilities with zero allocation where possible
pub struct PathUtil;

impl PathUtil {
    /// Normalize path separator for current platform
    #[inline]
    pub fn normalize_separator(path: &str) -> String {
        if cfg!(windows) {
            path.replace('/', "\\")
        } else {
            path.replace('\\', "/")
        }
    }

    /// Get platform-specific separator
    #[inline]
    pub const fn separator() -> char {
        MAIN_SEPARATOR
    }

    /// Check if path contains any separator
    #[inline]
    pub fn has_separator(path: &str) -> bool {
        path.contains('/') || path.contains('\\')
    }

    /// Split path by any separator (zero-copy iterator)
    #[inline]
    pub fn components(path: &str) -> impl Iterator<Item = &str> {
        path.split(|c| c == '/' || c == '\\')
            .filter(|s| !s.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_separator() {
        assert!(PathUtil::has_separator("foo/bar"));
        assert!(PathUtil::has_separator("foo\\bar"));
        assert!(!PathUtil::has_separator("foo"));
    }

    #[test]
    fn test_components() {
        let parts: Vec<&str> = PathUtil::components("foo/bar/baz").collect();
        assert_eq!(parts, vec!["foo", "bar", "baz"]);

        let parts: Vec<&str> = PathUtil::components("foo\\bar\\baz").collect();
        assert_eq!(parts, vec!["foo", "bar", "baz"]);
    }
}
