//! Python bindings for PatternMatcher

use lechange_core::patterns::matcher::PatternMatcher;
use pyo3::prelude::*;

/// Python pattern matcher wrapper
#[pyclass(name = "PatternMatcher")]
pub struct PyPatternMatcher {
    inner: PatternMatcher,
}

impl PyPatternMatcher {
    /// Create from an existing core PatternMatcher (used by pattern_loader)
    pub(crate) fn from_inner(inner: PatternMatcher) -> Self {
        Self { inner }
    }
}

#[pymethods]
impl PyPatternMatcher {
    #[new]
    #[pyo3(signature = (includes=None, excludes=None, negation_first=false))]
    fn new(
        includes: Option<Vec<String>>,
        excludes: Option<Vec<String>>,
        negation_first: bool,
    ) -> PyResult<Self> {
        let inc = includes.unwrap_or_default();
        let exc = excludes.unwrap_or_default();

        let inc_refs: Vec<&str> = inc.iter().map(|s| s.as_str()).collect();
        let exc_refs: Vec<&str> = exc.iter().map(|s| s.as_str()).collect();

        let inner = PatternMatcher::new(&inc_refs, &exc_refs, negation_first).map_err(|e| {
            PyErr::new::<crate::error::ConfigError, _>(format!("Invalid pattern: {}", e))
        })?;

        Ok(Self { inner })
    }

    /// Check if a path matches the patterns
    fn matches(&self, path: &str) -> bool {
        self.inner.matches_sync(path)
    }

    /// Filter a list of paths, returning only those that match
    fn filter(&self, paths: Vec<String>) -> Vec<String> {
        paths
            .into_iter()
            .filter(|p| self.inner.matches_sync(p))
            .collect()
    }

    /// Partition paths into (matched, unmatched)
    fn partition(&self, paths: Vec<String>) -> (Vec<String>, Vec<String>) {
        let mut matched = Vec::new();
        let mut unmatched = Vec::new();
        for p in paths {
            if self.inner.matches_sync(&p) {
                matched.push(p);
            } else {
                unmatched.push(p);
            }
        }
        (matched, unmatched)
    }

    fn __repr__(&self) -> String {
        "PatternMatcher(...)".to_string()
    }
}
