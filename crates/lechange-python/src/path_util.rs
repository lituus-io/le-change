//! Python bindings for PathUtil

use lechange_core::platform::PathUtil;
use pyo3::prelude::*;

/// Python path utility wrapper (all static methods)
#[pyclass(name = "PathUtil")]
pub struct PyPathUtil;

#[pymethods]
impl PyPathUtil {
    #[new]
    fn new() -> Self {
        Self
    }

    /// Convert path to POSIX format (forward slashes)
    #[staticmethod]
    fn to_posix(path: &str) -> String {
        PathUtil::to_posix(path).into_owned()
    }

    /// Normalize path separator for current platform
    #[staticmethod]
    fn normalize_separator(path: &str) -> String {
        PathUtil::normalize_separator(path)
    }

    /// Check if path contains any separator
    #[staticmethod]
    fn has_separator(path: &str) -> bool {
        PathUtil::has_separator(path)
    }

    /// Split path into components
    #[staticmethod]
    fn components(path: &str) -> Vec<String> {
        PathUtil::components(path).map(|s| s.to_string()).collect()
    }

    /// Get platform-specific separator
    #[staticmethod]
    fn separator() -> String {
        PathUtil::separator().to_string()
    }

    fn __repr__(&self) -> String {
        "PathUtil()".to_string()
    }
}
