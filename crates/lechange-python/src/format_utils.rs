//! Python bindings for JSON/format utility functions

use lechange_core::output::json_format;
use pyo3::prelude::*;

/// Escape a string for JSON output
#[pyfunction]
pub fn escape_json(s: &str) -> String {
    json_format::escape_json_value(s)
}

/// Escape for GitHub Actions safe output (percent-encoding special chars)
#[pyfunction]
pub fn safe_output_escape(s: &str) -> String {
    json_format::safe_output_escape(s)
}

/// Format a list of values as a JSON array string
#[pyfunction]
pub fn format_json_array(values: Vec<String>) -> String {
    let refs: Vec<&str> = values.iter().map(|s| s.as_str()).collect();
    json_format::format_json_array(&refs)
}

/// Format as a GitHub Actions matrix value
#[pyfunction]
pub fn format_matrix(values: Vec<String>) -> String {
    let refs: Vec<&str> = values.iter().map(|s| s.as_str()).collect();
    json_format::format_matrix(&refs)
}
