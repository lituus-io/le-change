//! Python bindings for lechange-core

use pyo3::prelude::*;

mod config;
mod detector;
mod error;
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

    // Register exceptions
    error::register_exceptions(module)?;

    // Module metadata
    module.add("__version__", env!("CARGO_PKG_VERSION"))?;

    Ok(())
}
