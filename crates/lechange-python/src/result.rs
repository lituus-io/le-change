//! Result type conversions

use pyo3::prelude::*;
use pyo3::types::PyList;
use lechange_core::{DiffResult, ChangeType};
use lechange_core::interner::StringInterner;

/// Python result wrapper
#[pyclass(name = "ChangedFiles")]
pub struct PyChangedFiles {
    added_files: Vec<String>,
    copied_files: Vec<String>,
    deleted_files: Vec<String>,
    modified_files: Vec<String>,
    renamed_files: Vec<String>,
    type_changed_files: Vec<String>,
    unmerged_files: Vec<String>,
    unknown_files: Vec<String>,
    all_changed_files: Vec<String>,
    json: bool,
}

#[pymethods]
impl PyChangedFiles {
    // File lists
    #[getter]
    fn added_files<'py>(&self, py: Python<'py>) -> Bound<'py, PyList> {
        PyList::new_bound(py, &self.added_files)
    }

    #[getter]
    fn copied_files<'py>(&self, py: Python<'py>) -> Bound<'py, PyList> {
        PyList::new_bound(py, &self.copied_files)
    }

    #[getter]
    fn deleted_files<'py>(&self, py: Python<'py>) -> Bound<'py, PyList> {
        PyList::new_bound(py, &self.deleted_files)
    }

    #[getter]
    fn modified_files<'py>(&self, py: Python<'py>) -> Bound<'py, PyList> {
        PyList::new_bound(py, &self.modified_files)
    }

    #[getter]
    fn renamed_files<'py>(&self, py: Python<'py>) -> Bound<'py, PyList> {
        PyList::new_bound(py, &self.renamed_files)
    }

    #[getter]
    fn type_changed_files<'py>(&self, py: Python<'py>) -> Bound<'py, PyList> {
        PyList::new_bound(py, &self.type_changed_files)
    }

    #[getter]
    fn unmerged_files<'py>(&self, py: Python<'py>) -> Bound<'py, PyList> {
        PyList::new_bound(py, &self.unmerged_files)
    }

    #[getter]
    fn unknown_files<'py>(&self, py: Python<'py>) -> Bound<'py, PyList> {
        PyList::new_bound(py, &self.unknown_files)
    }

    #[getter]
    fn all_changed_files<'py>(&self, py: Python<'py>) -> Bound<'py, PyList> {
        PyList::new_bound(py, &self.all_changed_files)
    }

    // Counts
    #[getter]
    fn added_files_count(&self) -> usize {
        self.added_files.len()
    }

    #[getter]
    fn copied_files_count(&self) -> usize {
        self.copied_files.len()
    }

    #[getter]
    fn deleted_files_count(&self) -> usize {
        self.deleted_files.len()
    }

    #[getter]
    fn modified_files_count(&self) -> usize {
        self.modified_files.len()
    }

    #[getter]
    fn renamed_files_count(&self) -> usize {
        self.renamed_files.len()
    }

    #[getter]
    fn type_changed_files_count(&self) -> usize {
        self.type_changed_files.len()
    }

    #[getter]
    fn unmerged_files_count(&self) -> usize {
        self.unmerged_files.len()
    }

    #[getter]
    fn unknown_files_count(&self) -> usize {
        self.unknown_files.len()
    }

    #[getter]
    fn all_changed_files_count(&self) -> usize {
        self.all_changed_files.len()
    }

    // Boolean checks
    #[getter]
    fn any_changed(&self) -> bool {
        !self.all_changed_files.is_empty()
    }

    #[getter]
    fn any_added(&self) -> bool {
        !self.added_files.is_empty()
    }

    #[getter]
    fn any_copied(&self) -> bool {
        !self.copied_files.is_empty()
    }

    #[getter]
    fn any_deleted(&self) -> bool {
        !self.deleted_files.is_empty()
    }

    #[getter]
    fn any_modified(&self) -> bool {
        !self.modified_files.is_empty()
    }

    #[getter]
    fn any_renamed(&self) -> bool {
        !self.renamed_files.is_empty()
    }

    #[getter]
    fn only_changed(&self) -> bool {
        self.all_changed_files.len() == 1
    }

    #[getter]
    fn only_added(&self) -> bool {
        self.added_files.len() == 1 && self.all_changed_files.len() == 1
    }

    #[getter]
    fn only_deleted(&self) -> bool {
        self.deleted_files.len() == 1 && self.all_changed_files.len() == 1
    }

    #[getter]
    fn only_modified(&self) -> bool {
        self.modified_files.len() == 1 && self.all_changed_files.len() == 1
    }

    fn __repr__(&self) -> String {
        format!(
            "ChangedFiles(total={}, added={}, modified={}, deleted={})",
            self.all_changed_files.len(),
            self.added_files.len(),
            self.modified_files.len(),
            self.deleted_files.len()
        )
    }
}

impl PyChangedFiles {
    /// Convert from core DiffResult
    pub fn from_core(diff: DiffResult, interner: &StringInterner, json: bool) -> Self {
        let mut added_files = Vec::new();
        let mut copied_files = Vec::new();
        let mut deleted_files = Vec::new();
        let mut modified_files = Vec::new();
        let mut renamed_files = Vec::new();
        let mut type_changed_files = Vec::new();
        let mut unmerged_files = Vec::new();
        let mut unknown_files = Vec::new();
        let mut all_changed_files = Vec::new();

        for file in diff.files {
            if let Some(path) = interner.resolve(file.path) {
                let path_string = path.to_string();
                all_changed_files.push(path_string.clone());

                match file.change_type {
                    ChangeType::Added => added_files.push(path_string),
                    ChangeType::Copied => copied_files.push(path_string),
                    ChangeType::Deleted => deleted_files.push(path_string),
                    ChangeType::Modified => modified_files.push(path_string),
                    ChangeType::Renamed => renamed_files.push(path_string),
                    ChangeType::TypeChanged => type_changed_files.push(path_string),
                    ChangeType::Unmerged => unmerged_files.push(path_string),
                    ChangeType::Unknown => unknown_files.push(path_string),
                }
            }
        }

        Self {
            added_files,
            copied_files,
            deleted_files,
            modified_files,
            renamed_files,
            type_changed_files,
            unmerged_files,
            unknown_files,
            all_changed_files,
            json,
        }
    }
}
