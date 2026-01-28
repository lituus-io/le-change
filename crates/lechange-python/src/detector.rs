//! Main detector wrapper

use pyo3::prelude::*;
use std::path::PathBuf;
use crate::config::PyConfig;
use crate::result::PyChangedFiles;
use crate::runtime::block_on_runtime;
use lechange_core::StringInterner;

/// Python change detector wrapper
#[pyclass(name = "ChangeDetector")]
pub struct PyChangeDetector {
    repo_path: PathBuf,
}

#[pymethods]
impl PyChangeDetector {
    #[new]
    fn new(repo_path: Option<&str>) -> PyResult<Self> {
        let path = repo_path
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from(".")));

        if !path.exists() {
            return Err(PyErr::new::<crate::error::PathError, _>(
                format!("Path does not exist: {}", path.display())
            ));
        }

        Ok(Self {
            repo_path: path,
        })
    }

    fn get_changed_files(&self, config: PyConfig) -> PyResult<PyChangedFiles> {
        let repo_path = self.repo_path.clone();
        let core_config = config.to_core_config();
        let json = config.json;

        // Execute the detection
        let result = block_on_runtime(async move {
            // Initialize interner
            let interner = StringInterner::with_capacity(2048);

            // Open repository
            let repo = lechange_core::git::GitRepository::discover(&repo_path)?;

            // Ensure depth if needed
            if core_config.fetch_depth > 0 {
                repo.ensure_depth(core_config.fetch_depth).await?;
            }

            // Create processor and run
            let processor = lechange_core::coordination::processor::FileProcessor::new(
                &repo,
                &interner,
                &core_config,
            );

            let diff = processor.process().await?;

            Ok((diff, interner))
        })?;

        let (diff, interner) = result;
        Ok(PyChangedFiles::from_core(diff, &interner, json))
    }

    fn __repr__(&self) -> String {
        format!("ChangeDetector(repo_path={})", self.repo_path.display())
    }
}
