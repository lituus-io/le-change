//! Python bindings for PatternLoader (YAML pattern groups)

use crate::pattern_matcher::PyPatternMatcher;
use lechange_core::patterns::loader::PatternLoader;
use pyo3::prelude::*;
use pyo3::types::PyDict;

/// Load YAML pattern groups and return a list of dicts with 'name' and 'matcher' keys
#[pyfunction]
#[pyo3(signature = (yaml, negation_first=false))]
pub fn load_yaml_patterns(
    py: Python<'_>,
    yaml: &str,
    negation_first: bool,
) -> PyResult<Vec<Py<PyAny>>> {
    let groups = PatternLoader::load_yaml_groups(yaml, negation_first)
        .map_err(|e| PyErr::new::<crate::error::YamlError, _>(format!("{}", e)))?;

    let mut result = Vec::with_capacity(groups.len());
    for group in groups {
        let dict = PyDict::new(py);
        dict.set_item("name", &group.name)?;
        let matcher = PyPatternMatcher::from_inner(group.matcher);
        dict.set_item("matcher", matcher.into_pyobject(py)?)?;
        result.push(dict.into_any().unbind());
    }

    Ok(result)
}
