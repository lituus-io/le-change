//! Result type conversions — accepts ProcessedResult + ComputedOutputs

use lechange_core::interner::StringInterner;
use lechange_core::output::computed::ComputedOutputs;
use lechange_core::output::json_format::format_deploy_matrix;
use lechange_core::types::{GroupDeployAction, ProcessedResult, RebuildReasonKind};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

/// Python result wrapper
#[pyclass(name = "ChangedFiles")]
pub struct PyChangedFiles {
    // Per-type filtered lists (resolved strings)
    added_files: Vec<String>,
    copied_files: Vec<String>,
    deleted_files: Vec<String>,
    modified_files: Vec<String>,
    renamed_files: Vec<String>,
    type_changed_files: Vec<String>,
    unmerged_files: Vec<String>,
    unknown_files: Vec<String>,

    // All files (filtered + unfiltered)
    all_changed_files: Vec<String>,
    all_changed_and_modified_files: Vec<String>,

    // "Other" (unmatched) categories
    other_changed_files: Vec<String>,
    other_modified_files: Vec<String>,
    other_deleted_files: Vec<String>,

    // Rename mapping: old_path -> new_path
    renamed_mapping: Vec<(String, String)>,

    // Old+new renamed files
    all_old_new_renamed_files: Vec<String>,

    // YAML group keys
    modified_keys: Vec<String>,
    changed_keys: Vec<String>,

    // CI decision
    files_to_rebuild: Vec<String>,
    files_to_skip: Vec<String>,
    failed_jobs: Vec<String>,
    successful_jobs: Vec<String>,
    rebuild_reasons: Vec<PyRebuildReason>,

    // Diagnostics
    diagnostics: Vec<PyDiagnostic>,

    // Deploy decisions
    group_deploy_decisions: Vec<PyGroupDeployDecision>,
    deploy_matrix_json: String,
    _has_deployable_groups: bool,

    // Metadata
    additions: u32,
    deletions: u32,
    pattern_applied: bool,
    _json: bool,
}

/// Internal rebuild reason for Python conversion
struct PyRebuildReason {
    file: String,
    kind: String,
    failed_run_id: Option<u64>,
    failed_job_name: Option<String>,
}

/// Internal diagnostic for Python conversion
struct PyDiagnostic {
    severity: String,
    category: String,
    message: String,
}

/// Internal group deploy decision for Python conversion
struct PyGroupDeployDecision {
    key: String,
    action: String,
    reason: Option<String>,
    files: Vec<String>,
    count: usize,
    concurrency_blocked: bool,
    concurrency_blocked_by: u32,
}

#[pymethods]
impl PyChangedFiles {
    // === File lists ===

    #[getter]
    fn added_files<'py>(&self, py: Python<'py>) -> Bound<'py, PyList> {
        PyList::new(py, &self.added_files).unwrap()
    }

    #[getter]
    fn copied_files<'py>(&self, py: Python<'py>) -> Bound<'py, PyList> {
        PyList::new(py, &self.copied_files).unwrap()
    }

    #[getter]
    fn deleted_files<'py>(&self, py: Python<'py>) -> Bound<'py, PyList> {
        PyList::new(py, &self.deleted_files).unwrap()
    }

    #[getter]
    fn modified_files<'py>(&self, py: Python<'py>) -> Bound<'py, PyList> {
        PyList::new(py, &self.modified_files).unwrap()
    }

    #[getter]
    fn renamed_files<'py>(&self, py: Python<'py>) -> Bound<'py, PyList> {
        PyList::new(py, &self.renamed_files).unwrap()
    }

    #[getter]
    fn type_changed_files<'py>(&self, py: Python<'py>) -> Bound<'py, PyList> {
        PyList::new(py, &self.type_changed_files).unwrap()
    }

    #[getter]
    fn unmerged_files<'py>(&self, py: Python<'py>) -> Bound<'py, PyList> {
        PyList::new(py, &self.unmerged_files).unwrap()
    }

    #[getter]
    fn unknown_files<'py>(&self, py: Python<'py>) -> Bound<'py, PyList> {
        PyList::new(py, &self.unknown_files).unwrap()
    }

    #[getter]
    fn all_changed_files<'py>(&self, py: Python<'py>) -> Bound<'py, PyList> {
        PyList::new(py, &self.all_changed_files).unwrap()
    }

    #[getter]
    fn all_changed_and_modified_files<'py>(&self, py: Python<'py>) -> Bound<'py, PyList> {
        PyList::new(py, &self.all_changed_and_modified_files).unwrap()
    }

    #[getter]
    fn other_changed_files<'py>(&self, py: Python<'py>) -> Bound<'py, PyList> {
        PyList::new(py, &self.other_changed_files).unwrap()
    }

    #[getter]
    fn other_modified_files<'py>(&self, py: Python<'py>) -> Bound<'py, PyList> {
        PyList::new(py, &self.other_modified_files).unwrap()
    }

    #[getter]
    fn other_deleted_files<'py>(&self, py: Python<'py>) -> Bound<'py, PyList> {
        PyList::new(py, &self.other_deleted_files).unwrap()
    }

    #[getter]
    fn all_old_new_renamed_files<'py>(&self, py: Python<'py>) -> Bound<'py, PyList> {
        PyList::new(py, &self.all_old_new_renamed_files).unwrap()
    }

    // === YAML group keys ===

    #[getter]
    fn modified_keys<'py>(&self, py: Python<'py>) -> Bound<'py, PyList> {
        PyList::new(py, &self.modified_keys).unwrap()
    }

    #[getter]
    fn changed_keys<'py>(&self, py: Python<'py>) -> Bound<'py, PyList> {
        PyList::new(py, &self.changed_keys).unwrap()
    }

    // === CI decision ===

    #[getter]
    fn files_to_rebuild<'py>(&self, py: Python<'py>) -> Bound<'py, PyList> {
        PyList::new(py, &self.files_to_rebuild).unwrap()
    }

    #[getter]
    fn files_to_skip<'py>(&self, py: Python<'py>) -> Bound<'py, PyList> {
        PyList::new(py, &self.files_to_skip).unwrap()
    }

    #[getter]
    fn failed_jobs<'py>(&self, py: Python<'py>) -> Bound<'py, PyList> {
        PyList::new(py, &self.failed_jobs).unwrap()
    }

    #[getter]
    fn successful_jobs<'py>(&self, py: Python<'py>) -> Bound<'py, PyList> {
        PyList::new(py, &self.successful_jobs).unwrap()
    }

    #[getter]
    fn rebuild_reasons<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let list = PyList::empty(py);
        for reason in &self.rebuild_reasons {
            let dict = PyDict::new(py);
            dict.set_item("file", &reason.file)?;
            dict.set_item("kind", &reason.kind)?;
            dict.set_item("failed_run_id", reason.failed_run_id)?;
            dict.set_item("failed_job_name", &reason.failed_job_name)?;
            list.append(dict)?;
        }
        Ok(list)
    }

    // === Diagnostics ===

    #[getter]
    fn diagnostics<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let list = PyList::empty(py);
        for diag in &self.diagnostics {
            let dict = PyDict::new(py);
            dict.set_item("severity", &diag.severity)?;
            dict.set_item("category", &diag.category)?;
            dict.set_item("message", &diag.message)?;
            list.append(dict)?;
        }
        Ok(list)
    }

    // === Deploy decisions ===

    #[getter]
    fn deploy_decisions<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let list = PyList::empty(py);
        for d in &self.group_deploy_decisions {
            let dict = PyDict::new(py);
            dict.set_item("key", &d.key)?;
            dict.set_item("action", &d.action)?;
            dict.set_item("reason", &d.reason)?;
            dict.set_item("files", PyList::new(py, &d.files).unwrap())?;
            dict.set_item("count", d.count)?;
            dict.set_item("concurrency_blocked", d.concurrency_blocked)?;
            dict.set_item("concurrency_blocked_by", d.concurrency_blocked_by)?;
            list.append(dict)?;
        }
        Ok(list)
    }

    #[getter]
    fn deploy_matrix(&self) -> &str {
        &self.deploy_matrix_json
    }

    #[getter]
    fn has_deployable_groups(&self) -> bool {
        self._has_deployable_groups
    }

    // === Rename mapping ===

    #[getter]
    fn renamed_files_mapping<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        for (old, new) in &self.renamed_mapping {
            dict.set_item(old, new)?;
        }
        Ok(dict)
    }

    // === Counts ===

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

    #[getter]
    fn all_changed_and_modified_files_count(&self) -> usize {
        self.all_changed_and_modified_files.len()
    }

    #[getter]
    fn other_changed_files_count(&self) -> usize {
        self.other_changed_files.len()
    }

    #[getter]
    fn other_modified_files_count(&self) -> usize {
        self.other_modified_files.len()
    }

    #[getter]
    fn other_deleted_files_count(&self) -> usize {
        self.other_deleted_files.len()
    }

    #[getter]
    fn files_to_rebuild_count(&self) -> usize {
        self.files_to_rebuild.len()
    }

    #[getter]
    fn files_to_skip_count(&self) -> usize {
        self.files_to_skip.len()
    }

    // === Metadata ===

    #[getter]
    fn additions(&self) -> u32 {
        self.additions
    }

    #[getter]
    fn deletions(&self) -> u32 {
        self.deletions
    }

    #[getter]
    fn pattern_applied(&self) -> bool {
        self.pattern_applied
    }

    // === Boolean checks ===

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
        self.added_files.len() == 1
            && self.modified_files.is_empty()
            && self.deleted_files.is_empty()
            && self.renamed_files.is_empty()
            && self.copied_files.is_empty()
    }

    #[getter]
    fn only_deleted(&self) -> bool {
        self.deleted_files.len() == 1
            && self.added_files.is_empty()
            && self.modified_files.is_empty()
            && self.renamed_files.is_empty()
            && self.copied_files.is_empty()
    }

    #[getter]
    fn only_modified(&self) -> bool {
        self.modified_files.len() == 1
            && self.added_files.is_empty()
            && self.deleted_files.is_empty()
            && self.renamed_files.is_empty()
            && self.copied_files.is_empty()
    }

    fn __repr__(&self) -> String {
        format!(
            "ChangedFiles(total={}, added={}, modified={}, deleted={}, rebuild={}, skip={}, deploy_groups={})",
            self.all_changed_files.len(),
            self.added_files.len(),
            self.modified_files.len(),
            self.deleted_files.len(),
            self.files_to_rebuild.len(),
            self.files_to_skip.len(),
            self.group_deploy_decisions.iter().filter(|d| d.action == "deploy").count(),
        )
    }
}

impl PyChangedFiles {
    /// Convert from ProcessedResult + ComputedOutputs
    pub fn from_core(
        mut result: ProcessedResult,
        outputs: &ComputedOutputs,
        interner: &StringInterner,
        json: bool,
        use_posix_path_separator: bool,
        deploy_matrix_include_reason: bool,
        deploy_matrix_include_concurrency: bool,
    ) -> Self {
        // Helper to apply POSIX path conversion if configured
        let resolve_path = |s: &str| -> String {
            if use_posix_path_separator {
                lechange_core::platform::PathUtil::to_posix(s).into_owned()
            } else {
                s.to_string()
            }
        };

        // Helper to resolve index list to strings
        let resolve_indices = |indices: &[u32]| -> Vec<String> {
            indices
                .iter()
                .filter_map(|&i| {
                    let file = &result.all_files[i as usize];
                    interner.resolve(file.path).map(&resolve_path)
                })
                .collect()
        };

        // Per-type filtered lists
        let added_files = resolve_indices(&outputs.filtered_added);
        let copied_files = resolve_indices(&outputs.filtered_copied);
        let mut deleted_files = resolve_indices(&outputs.filtered_deleted);
        let modified_files = resolve_indices(&outputs.filtered_modified);
        let renamed_files = resolve_indices(&outputs.filtered_renamed);
        let type_changed_files = resolve_indices(&outputs.filtered_type_changed);
        let unmerged_files = resolve_indices(&outputs.filtered_unmerged);
        let unknown_files = resolve_indices(&outputs.filtered_unknown);

        // Include rename-split deletions (old paths of renames treated as deleted)
        for &(_idx, prev_path) in &outputs.rename_split_deletions {
            if let Some(path_str) = interner.resolve(prev_path) {
                deleted_files.push(resolve_path(path_str));
            }
        }

        // All filtered files
        let all_changed_files = resolve_indices(&result.filtered_indices);

        // All changed and modified
        let all_changed_and_modified_files = resolve_indices(&outputs.all_changed_and_modified);

        // "Other" categories
        let other_changed_files = resolve_indices(&outputs.other_changed);
        let other_modified_files = resolve_indices(&outputs.other_modified);
        let other_deleted_files = resolve_indices(&outputs.other_deleted);

        // Rename mapping
        let renamed_mapping: Vec<(String, String)> = outputs
            .renamed_mapping
            .iter()
            .filter_map(|&(idx, prev_path)| {
                let file = &result.all_files[idx as usize];
                let new_path = interner.resolve(file.path)?;
                let old_path = interner.resolve(prev_path)?;
                Some((resolve_path(old_path), resolve_path(new_path)))
            })
            .collect();

        // Old+new renamed files: [old1, new1, old2, new2, ...]
        let all_old_new_renamed_files: Vec<String> = outputs
            .renamed_mapping
            .iter()
            .filter_map(|&(idx, prev_path)| {
                let file = &result.all_files[idx as usize];
                let new_path = interner.resolve(file.path)?;
                let old_path = interner.resolve(prev_path)?;
                Some(vec![resolve_path(old_path), resolve_path(new_path)])
            })
            .flatten()
            .collect();

        // CI decision
        let (files_to_rebuild, files_to_skip, failed_jobs, successful_jobs, rebuild_reasons) =
            if let Some(ref ci) = result.ci_decision {
                let rebuild: Vec<String> = ci
                    .files_to_rebuild
                    .iter()
                    .filter_map(|s| interner.resolve(*s).map(&resolve_path))
                    .collect();
                let skip: Vec<String> = ci
                    .files_to_skip
                    .iter()
                    .filter_map(|s| interner.resolve(*s).map(&resolve_path))
                    .collect();
                let fj: Vec<String> = ci
                    .failed_jobs
                    .iter()
                    .filter_map(|s| interner.resolve(*s).map(|p| p.to_string()))
                    .collect();
                let sj: Vec<String> = ci
                    .successful_jobs
                    .iter()
                    .filter_map(|s| interner.resolve(*s).map(|p| p.to_string()))
                    .collect();
                let reasons: Vec<PyRebuildReason> = ci
                    .rebuild_reasons
                    .iter()
                    .filter_map(|r| {
                        let file = interner.resolve(r.file)?.to_string();
                        let kind = match r.kind {
                            RebuildReasonKind::NewChange => "new_change",
                            RebuildReasonKind::PreviousFailure => "previous_failure",
                            RebuildReasonKind::BothNewAndFailed => "both_new_and_failed",
                        }
                        .to_string();
                        let failed_job_name = r
                            .failed_job_name
                            .and_then(|s| interner.resolve(s).map(|p| p.to_string()));
                        Some(PyRebuildReason {
                            file,
                            kind,
                            failed_run_id: r.failed_run_id,
                            failed_job_name,
                        })
                    })
                    .collect();
                (rebuild, skip, fj, sj, reasons)
            } else {
                (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new())
            };

        // Deploy decisions
        let group_deploy_decisions: Vec<PyGroupDeployDecision> = outputs
            .group_deploy_decisions
            .iter()
            .map(|d| {
                let action = match d.action {
                    GroupDeployAction::Deploy => "deploy",
                    GroupDeployAction::Skip => "skip",
                }
                .to_string();
                let reason = d.reason.map(|r| {
                    match r {
                        lechange_core::types::GroupDeployReason::NewChange => "new_change",
                        lechange_core::types::GroupDeployReason::PreviousFailure => {
                            "previous_failure"
                        }
                        lechange_core::types::GroupDeployReason::BothNewAndFailed => {
                            "both_new_and_failed"
                        }
                    }
                    .to_string()
                });
                let files: Vec<String> = d
                    .files_to_rebuild
                    .iter()
                    .filter_map(|&s| interner.resolve(s).map(&resolve_path))
                    .collect();
                let count = files.len();
                PyGroupDeployDecision {
                    key: interner.resolve(d.key).unwrap_or("").to_string(),
                    action,
                    reason,
                    files,
                    count,
                    concurrency_blocked: d.concurrency_blocked,
                    concurrency_blocked_by: d.concurrency_blocked_by,
                }
            })
            .collect();

        let deploy_matrix_json = format_deploy_matrix(
            &outputs.group_deploy_decisions,
            |s| interner.resolve(s),
            " ",
            deploy_matrix_include_reason,
            deploy_matrix_include_concurrency,
        );

        let _has_deployable_groups = outputs.has_deployable_groups();

        // Diagnostics — drain to move Strings instead of cloning
        let diagnostics: Vec<PyDiagnostic> = result
            .diagnostics
            .drain(..)
            .map(|d| {
                let severity = match d.severity {
                    lechange_core::types::DiagnosticSeverity::Warning => "warning",
                    lechange_core::types::DiagnosticSeverity::SoftError => "soft_error",
                }
                .to_string();
                let category = match d.category {
                    lechange_core::types::DiagnosticCategory::InitialDiff => "initial_diff",
                    lechange_core::types::DiagnosticCategory::SubmoduleDiff => "submodule_diff",
                    lechange_core::types::DiagnosticCategory::SkippedSameSha => "skipped_same_sha",
                    lechange_core::types::DiagnosticCategory::ShallowClone => "shallow_clone",
                    lechange_core::types::DiagnosticCategory::PatternLoad => "pattern_load",
                    lechange_core::types::DiagnosticCategory::SymlinkDetection => {
                        "symlink_detection"
                    }
                    lechange_core::types::DiagnosticCategory::WorkflowApi => "workflow_api",
                    lechange_core::types::DiagnosticCategory::AncestorRecovery => {
                        "ancestor_recovery"
                    }
                }
                .to_string();
                PyDiagnostic {
                    severity,
                    category,
                    message: d.message,
                }
            })
            .collect();

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
            all_changed_and_modified_files,
            other_changed_files,
            other_modified_files,
            other_deleted_files,
            renamed_mapping,
            all_old_new_renamed_files,
            modified_keys: outputs
                .modified_keys
                .iter()
                .filter_map(|&s| interner.resolve(s).map(|p| p.to_string()))
                .collect(),
            changed_keys: outputs
                .changed_keys
                .iter()
                .filter_map(|&s| interner.resolve(s).map(|p| p.to_string()))
                .collect(),
            files_to_rebuild,
            files_to_skip,
            failed_jobs,
            successful_jobs,
            rebuild_reasons,
            diagnostics,
            group_deploy_decisions,
            deploy_matrix_json,
            _has_deployable_groups,
            additions: result.additions,
            deletions: result.deletions,
            pattern_applied: result.pattern_applied,
            _json: json,
        }
    }
}
