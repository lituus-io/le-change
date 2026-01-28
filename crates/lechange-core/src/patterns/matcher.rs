//! Pattern matching with parallel filtering

use globset::{Glob, GlobSet, GlobSetBuilder};
use rayon::prelude::*;
use crate::types::ChangedFile;
use crate::interner::StringInterner;
use crate::error::Result;

/// Pattern matcher with precompiled glob patterns
pub struct PatternMatcher {
    include_set: GlobSet,
    exclude_set: GlobSet,
    negation_first: bool,
}

impl PatternMatcher {
    /// Create a new pattern matcher
    pub fn new(
        includes: &[&str],
        excludes: &[&str],
        negation_first: bool,
    ) -> Result<Self> {
        let mut include_builder = GlobSetBuilder::new();
        for pattern in includes {
            include_builder.add(Glob::new(pattern)?);
        }

        let mut exclude_builder = GlobSetBuilder::new();
        for pattern in excludes {
            exclude_builder.add(Glob::new(pattern)?);
        }

        Ok(Self {
            include_set: include_builder.build()?,
            exclude_set: exclude_builder.build()?,
            negation_first,
        })
    }

    /// Synchronous match for use with rayon - zero allocation
    #[inline]
    pub fn matches_sync(&self, path: &str) -> bool {
        if self.negation_first {
            if self.exclude_set.is_match(path) {
                return false;
            }
            self.include_set.is_empty() || self.include_set.is_match(path)
        } else {
            if !self.include_set.is_empty() && !self.include_set.is_match(path) {
                return false;
            }
            !self.exclude_set.is_match(path)
        }
    }

    /// Parallel filter using rayon - processes files in parallel
    pub fn filter_files_parallel(
        &self,
        files: &[ChangedFile],
        interner: &StringInterner,
    ) -> Vec<ChangedFile> {
        files
            .par_iter()
            .filter(|file| {
                if let Some(path) = interner.resolve(file.path) {
                    self.matches_sync(path)
                } else {
                    false
                }
            })
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_matching() {
        let matcher = PatternMatcher::new(
            &["**/*.rs"],
            &[],
            false,
        ).unwrap();

        assert!(matcher.matches_sync("src/main.rs"));
        assert!(matcher.matches_sync("lib/mod.rs"));
        assert!(!matcher.matches_sync("README.md"));
    }

    #[test]
    fn test_exclusion() {
        let matcher = PatternMatcher::new(
            &["**/*.rs"],
            &["**/test_*.rs"],
            false,
        ).unwrap();

        assert!(matcher.matches_sync("src/main.rs"));
        assert!(!matcher.matches_sync("src/test_utils.rs"));
    }
}
