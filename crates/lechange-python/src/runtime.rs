//! Tokio runtime management for Python bindings

use once_cell::sync::OnceCell;
use pyo3::prelude::*;
use std::sync::Arc;
use tokio::runtime::{Builder, Runtime};

// Global runtime - initialized once, lives for process lifetime
static RUNTIME: OnceCell<Arc<Runtime>> = OnceCell::new();

/// Get or initialize the global Tokio runtime
pub fn get_runtime() -> PyResult<Arc<Runtime>> {
    RUNTIME
        .get_or_try_init(|| {
            Builder::new_multi_thread()
                .worker_threads(4)
                .thread_name("lechange-worker")
                .enable_all()
                .build()
                .map(Arc::new)
                .map_err(|e| {
                    PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                        format!("Failed to initialize Tokio runtime: {}", e)
                    )
                })
        })
        .cloned()
}

/// Execute an async function on the runtime, blocking until completion
pub fn block_on_runtime<F, T>(future: F) -> PyResult<T>
where
    F: std::future::Future<Output = lechange_core::Result<T>> + Send + 'static,
    T: Send + 'static,
{
    let runtime = get_runtime()?;

    // Release GIL during blocking operation to allow other Python threads
    Python::with_gil(|py| {
        py.allow_threads(|| {
            runtime.block_on(future)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                    format!("Async operation failed: {}", e)
                ))
        })
    })
}
