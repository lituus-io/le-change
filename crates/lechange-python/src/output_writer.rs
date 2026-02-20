//! Python bindings for OutputWriter

use lechange_core::output::writer::OutputWriter;
use pyo3::prelude::*;
use std::path::Path;

/// Python output writer wrapper (all static methods)
#[pyclass(name = "OutputWriter")]
pub struct PyOutputWriter;

#[pymethods]
impl PyOutputWriter {
    #[new]
    fn new() -> Self {
        Self
    }

    /// Write a list of values to a text file
    #[staticmethod]
    fn write_text(
        output_dir: &str,
        name: &str,
        values: Vec<String>,
        separator: &str,
    ) -> PyResult<()> {
        let refs: Vec<&str> = values.iter().map(|s| s.as_str()).collect();
        OutputWriter::write_text(Path::new(output_dir), name, &refs, separator)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyOSError, _>(format!("{}", e)))
    }

    /// Write a JSON array to a file
    #[staticmethod]
    fn write_json(output_dir: &str, name: &str, values: Vec<String>) -> PyResult<()> {
        let refs: Vec<&str> = values.iter().map(|s| s.as_str()).collect();
        OutputWriter::write_json(Path::new(output_dir), name, &refs)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyOSError, _>(format!("{}", e)))
    }

    fn __repr__(&self) -> String {
        "OutputWriter()".to_string()
    }
}
