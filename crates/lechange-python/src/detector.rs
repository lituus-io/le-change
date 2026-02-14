//! Main detector wrapper

use crate::config::PyConfig;
use crate::result::PyChangedFiles;
use crate::runtime::block_on_runtime;
use lechange_core::output::computed::ComputedOutputs;
use lechange_core::StringInterner;
use pyo3::prelude::*;
use std::path::PathBuf;

/// Python change detector wrapper
#[pyclass(name = "ChangeDetector")]
pub struct PyChangeDetector {
    repo_path: PathBuf,
}

#[pymethods]
impl PyChangeDetector {
    #[new]
    #[pyo3(signature = (repo_path=None))]
    fn new(repo_path: Option<&str>) -> PyResult<Self> {
        let path = repo_path
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        if !path.exists() {
            return Err(PyErr::new::<crate::error::PathError, _>(format!(
                "Path does not exist: {}",
                path.display()
            )));
        }

        Ok(Self { repo_path: path })
    }

    fn get_changed_files(&self, config: PyConfig) -> PyResult<PyChangedFiles> {
        let repo_path = self.repo_path.clone();
        let core_config = config.to_core_config();
        let json = config.json;
        let use_posix = config.use_posix_path_separator;

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

            let processed = processor.process().await?;

            // Compute derived outputs (with rename splitting support)
            let outputs = ComputedOutputs::compute(
                &processed,
                core_config.output_renamed_as_deleted_added,
            );

            Ok((processed, outputs, interner))
        })?;

        let (processed, outputs, interner) = result;
        Ok(PyChangedFiles::from_core(processed, &outputs, &interner, json, use_posix))
    }

    fn __repr__(&self) -> String {
        format!("ChangeDetector(repo_path={})", self.repo_path.display())
    }
}
