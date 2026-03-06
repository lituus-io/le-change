//! Python bindings for FileRecovery

use lechange_core::git::recovery::FileRecovery;
use pyo3::prelude::*;
use std::path::{Path, PathBuf};

/// Python file recovery wrapper
#[pyclass(name = "FileRecovery")]
pub struct PyFileRecovery {
    inner: FileRecovery,
    repo_path: PathBuf,
}

#[pymethods]
impl PyFileRecovery {
    #[new]
    fn new(repo_path: &str) -> PyResult<Self> {
        let path = Path::new(repo_path);
        if !path.exists() {
            return Err(PyErr::new::<crate::error::PathError, _>(format!(
                "Path does not exist: {}",
                repo_path
            )));
        }

        Ok(Self {
            inner: FileRecovery::new(path),
            repo_path: path.to_path_buf(),
        })
    }

    /// Recover a single file from a given commit SHA
    fn recover_file(&self, sha: &str, file_path: &str, output_dir: &str) -> PyResult<String> {
        let output = Path::new(output_dir);
        let result = self
            .inner
            .recover_file(sha, file_path, output)
            .map_err(|e| PyErr::new::<crate::error::RecoveryError, _>(format!("{}", e)))?;
        Ok(result.to_string_lossy().into_owned())
    }

    /// Recover multiple files from a given commit SHA
    fn recover_files(
        &self,
        sha: &str,
        file_paths: Vec<String>,
        output_dir: &str,
    ) -> PyResult<Vec<String>> {
        let output = Path::new(output_dir);
        let mut results = Vec::with_capacity(file_paths.len());
        for fp in &file_paths {
            let result = self
                .inner
                .recover_file(sha, fp, output)
                .map_err(|e| PyErr::new::<crate::error::RecoveryError, _>(format!("{}", e)))?;
            results.push(result.to_string_lossy().into_owned());
        }
        Ok(results)
    }

    fn __repr__(&self) -> String {
        format!("FileRecovery(repo_path={})", self.repo_path.display())
    }
}
