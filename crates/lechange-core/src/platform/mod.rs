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
        path.split(['/', '\\']).filter(|s| !s.is_empty())
    }

    /// Convert path to POSIX format (forward slashes)
    ///
    /// No-op on Unix (returns borrowed), replaces `\` on Windows.
    #[inline]
    pub fn to_posix(path: &str) -> std::borrow::Cow<'_, str> {
        if cfg!(windows) || path.contains('\\') {
            std::borrow::Cow::Owned(path.replace('\\', "/"))
        } else {
            std::borrow::Cow::Borrowed(path)
        }
    }

    /// Apply separator to path based on config
    #[inline]
    pub fn with_separator(path: &str, use_posix: bool) -> std::borrow::Cow<'_, str> {
        if use_posix {
            Self::to_posix(path)
        } else {
            std::borrow::Cow::Borrowed(path)
        }
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

    #[test]
    fn test_normalize_separator() {
        // On non-windows, backslashes become forward slashes
        if !cfg!(windows) {
            assert_eq!(PathUtil::normalize_separator("foo\\bar"), "foo/bar");
            assert_eq!(PathUtil::normalize_separator("foo/bar"), "foo/bar");
        }
    }

    #[test]
    fn test_to_posix() {
        let result = PathUtil::to_posix("foo\\bar\\baz");
        assert_eq!(result.as_ref(), "foo/bar/baz");

        // Already POSIX on non-windows: should be Cow::Borrowed (no allocation)
        if !cfg!(windows) {
            let result = PathUtil::to_posix("foo/bar/baz");
            assert!(matches!(result, std::borrow::Cow::Borrowed(_)));
            assert_eq!(result.as_ref(), "foo/bar/baz");
        }
    }

    #[test]
    fn test_to_posix_no_backslash() {
        let input = "already/posix/path";
        let result = PathUtil::to_posix(input);
        if !cfg!(windows) {
            assert!(matches!(result, std::borrow::Cow::Borrowed(_)));
        }
        assert_eq!(result.as_ref(), "already/posix/path");
    }

    #[test]
    fn test_with_separator_posix_true() {
        let result = PathUtil::with_separator("foo\\bar", true);
        assert_eq!(result.as_ref(), "foo/bar");
    }

    #[test]
    fn test_with_separator_posix_false() {
        let result = PathUtil::with_separator("foo\\bar", false);
        // When use_posix is false, returns Cow::Borrowed (original string)
        assert!(matches!(result, std::borrow::Cow::Borrowed(_)));
        assert_eq!(result.as_ref(), "foo\\bar");
    }
}
