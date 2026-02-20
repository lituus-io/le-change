//! Python bindings for lechange-core

use pyo3::prelude::*;

mod config;
mod detector;
mod error;
mod format_utils;
mod output_writer;
mod path_util;
mod pattern_loader;
pub(crate) mod pattern_matcher;
mod recovery;
mod result;
mod runtime;

pub use config::PyConfig;
pub use detector::PyChangeDetector;
pub use result::PyChangedFiles;

/// LeChange Python module
#[pymodule]
fn _lechange(module: &Bound<'_, PyModule>) -> PyResult<()> {
    // Register classes
    module.add_class::<PyChangeDetector>()?;
    module.add_class::<PyConfig>()?;
    module.add_class::<PyChangedFiles>()?;
    module.add_class::<pattern_matcher::PyPatternMatcher>()?;
    module.add_class::<path_util::PyPathUtil>()?;
    module.add_class::<recovery::PyFileRecovery>()?;
    module.add_class::<output_writer::PyOutputWriter>()?;

    // Register functions
    module.add_function(wrap_pyfunction!(format_utils::escape_json, module)?)?;
    module.add_function(wrap_pyfunction!(format_utils::safe_output_escape, module)?)?;
    module.add_function(wrap_pyfunction!(format_utils::format_json_array, module)?)?;
    module.add_function(wrap_pyfunction!(format_utils::format_matrix, module)?)?;
    module.add_function(wrap_pyfunction!(
        pattern_loader::load_yaml_patterns,
        module
    )?)?;

    // Register exceptions
    error::register_exceptions(module)?;

    // Module metadata
    module.add("__version__", env!("CARGO_PKG_VERSION"))?;

    Ok(())
}
