//! Configuration type conversions

use pyo3::prelude::*;
use std::borrow::Cow;

/// Python configuration wrapper
#[pyclass(name = "Config")]
#[derive(Clone)]
pub struct PyConfig {
    // SHA configuration
    pub base_sha: Option<String>,
    pub sha: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,

    // Pattern configuration
    pub files: Option<Vec<String>>,
    pub files_separator: String,
    pub files_ignore: Option<Vec<String>>,
    pub files_ignore_separator: String,

    // YAML patterns
    pub files_yaml: Option<String>,
    pub files_yaml_from_source_file: Option<String>,
    pub files_from_source_file: Option<String>,
    pub files_from_source_file_separator: String,

    // Diff configuration
    pub diff_filter: String,
    pub include_all_old_new_renamed_files: bool,
    pub old_new_separator: String,
    pub old_new_files_separator: String,

    // Directory configuration
    pub dir_names: bool,
    pub dir_names_max_depth: Option<u32>,
    pub quotepath: bool,
    pub path_separator: String,

    // Directory extras
    pub dir_names_exclude_current_dir: bool,
    pub dir_names_include_files: Option<Vec<String>>,
    pub dir_names_deleted_files_include_only_deleted_dirs: bool,

    // Submodule configuration
    pub include_submodules: bool,
    pub submodule_filter: Option<String>,

    // Fetch configuration
    pub fetch_depth: u32,
    pub fetch_additional_submodule_history: bool,

    // Output configuration
    pub json: bool,
    pub escape_json: bool,
    pub safe_output: bool,
    pub output_dir: Option<String>,

    // API configuration
    pub skip_initial_fetch: bool,
    pub use_rest_api: bool,
    pub api_url: Option<String>,
    pub token: Option<String>,

    // Other configuration
    pub write_output_files: bool,
    pub negation_patterns_first: bool,
    pub match_gitignore_files: bool,
    pub recover_deleted_files: bool,
    pub exclude_symlinks: bool,

    // Tag comparison
    pub tags_pattern: Option<String>,
    pub tags_ignore_pattern: Option<String>,

    // Soft-fail
    pub fail_on_initial_diff_error: bool,
    pub fail_on_submodule_diff_error: bool,
    pub skip_same_sha: bool,

    // Rename splitting / POSIX
    pub output_renamed_as_deleted_added: bool,
    pub use_posix_path_separator: bool,

    // Workflow failure tracking configuration
    pub track_workflow_failures: bool,
    pub workflow_lookback_commits: u32,
    pub wait_for_active_workflows: bool,
    pub workflow_max_wait_seconds: u32,
    pub include_failed_files: bool,

    // Workflow intelligence (enhanced)
    pub failure_tracking_level: Option<String>,
    pub workflow_success_lookback: u32,
    pub skip_successful_files: bool,
    pub workflow_name_filter: Option<String>,

    // Group-by discovery
    pub files_group_by: Option<String>,
    pub files_group_by_key: Option<String>,

    // Ancestor directory file association
    pub files_ancestor_lookup_depth: u32,

    // Deploy matrix enrichment
    pub deploy_matrix_include_reason: bool,
    pub deploy_matrix_include_concurrency: bool,
}

#[pymethods]
impl PyConfig {
    #[new]
    #[pyo3(signature = (
        base_sha=None,
        sha=None,
        since=None,
        until=None,
        files=None,
        files_separator=None,
        files_ignore=None,
        files_ignore_separator=None,
        files_yaml=None,
        files_yaml_from_source_file=None,
        files_from_source_file=None,
        files_from_source_file_separator=None,
        diff_filter=None,
        include_all_old_new_renamed_files=None,
        old_new_separator=None,
        old_new_files_separator=None,
        dir_names=None,
        dir_names_max_depth=None,
        quotepath=None,
        path_separator=None,
        dir_names_exclude_current_dir=None,
        dir_names_include_files=None,
        dir_names_deleted_files_include_only_deleted_dirs=None,
        include_submodules=None,
        submodule_filter=None,
        fetch_depth=None,
        fetch_additional_submodule_history=None,
        json=None,
        escape_json=None,
        safe_output=None,
        output_dir=None,
        skip_initial_fetch=None,
        use_rest_api=None,
        api_url=None,
        token=None,
        write_output_files=None,
        negation_patterns_first=None,
        match_gitignore_files=None,
        recover_deleted_files=None,
        exclude_symlinks=None,
        tags_pattern=None,
        tags_ignore_pattern=None,
        fail_on_initial_diff_error=None,
        fail_on_submodule_diff_error=None,
        skip_same_sha=None,
        output_renamed_as_deleted_added=None,
        use_posix_path_separator=None,
        track_workflow_failures=None,
        workflow_lookback_commits=None,
        wait_for_active_workflows=None,
        workflow_max_wait_seconds=None,
        include_failed_files=None,
        failure_tracking_level=None,
        workflow_success_lookback=None,
        skip_successful_files=None,
        workflow_name_filter=None,
        files_group_by=None,
        files_group_by_key=None,
        files_ancestor_lookup_depth=None,
        deploy_matrix_include_reason=None,
        deploy_matrix_include_concurrency=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        base_sha: Option<String>,
        sha: Option<String>,
        since: Option<String>,
        until: Option<String>,
        files: Option<Vec<String>>,
        files_separator: Option<String>,
        files_ignore: Option<Vec<String>>,
        files_ignore_separator: Option<String>,
        files_yaml: Option<String>,
        files_yaml_from_source_file: Option<String>,
        files_from_source_file: Option<String>,
        files_from_source_file_separator: Option<String>,
        diff_filter: Option<String>,
        include_all_old_new_renamed_files: Option<bool>,
        old_new_separator: Option<String>,
        old_new_files_separator: Option<String>,
        dir_names: Option<bool>,
        dir_names_max_depth: Option<u32>,
        quotepath: Option<bool>,
        path_separator: Option<String>,
        dir_names_exclude_current_dir: Option<bool>,
        dir_names_include_files: Option<Vec<String>>,
        dir_names_deleted_files_include_only_deleted_dirs: Option<bool>,
        include_submodules: Option<bool>,
        submodule_filter: Option<String>,
        fetch_depth: Option<u32>,
        fetch_additional_submodule_history: Option<bool>,
        json: Option<bool>,
        escape_json: Option<bool>,
        safe_output: Option<bool>,
        output_dir: Option<String>,
        skip_initial_fetch: Option<bool>,
        use_rest_api: Option<bool>,
        api_url: Option<String>,
        token: Option<String>,
        write_output_files: Option<bool>,
        negation_patterns_first: Option<bool>,
        match_gitignore_files: Option<bool>,
        recover_deleted_files: Option<bool>,
        exclude_symlinks: Option<bool>,
        tags_pattern: Option<String>,
        tags_ignore_pattern: Option<String>,
        fail_on_initial_diff_error: Option<bool>,
        fail_on_submodule_diff_error: Option<bool>,
        skip_same_sha: Option<bool>,
        output_renamed_as_deleted_added: Option<bool>,
        use_posix_path_separator: Option<bool>,
        track_workflow_failures: Option<bool>,
        workflow_lookback_commits: Option<u32>,
        wait_for_active_workflows: Option<bool>,
        workflow_max_wait_seconds: Option<u32>,
        include_failed_files: Option<bool>,
        failure_tracking_level: Option<String>,
        workflow_success_lookback: Option<u32>,
        skip_successful_files: Option<bool>,
        workflow_name_filter: Option<String>,
        files_group_by: Option<String>,
        files_group_by_key: Option<String>,
        files_ancestor_lookup_depth: Option<u32>,
        deploy_matrix_include_reason: Option<bool>,
        deploy_matrix_include_concurrency: Option<bool>,
    ) -> Self {
        Self {
            base_sha,
            sha,
            since,
            until,
            files,
            files_separator: files_separator.unwrap_or_else(|| " ".to_string()),
            files_ignore,
            files_ignore_separator: files_ignore_separator.unwrap_or_else(|| " ".to_string()),
            files_yaml,
            files_yaml_from_source_file,
            files_from_source_file,
            files_from_source_file_separator: files_from_source_file_separator
                .unwrap_or_else(|| "\n".to_string()),
            diff_filter: diff_filter.unwrap_or_else(|| "ACDMRTUX".to_string()),
            include_all_old_new_renamed_files: include_all_old_new_renamed_files.unwrap_or(false),
            old_new_separator: old_new_separator.unwrap_or_else(|| " ".to_string()),
            old_new_files_separator: old_new_files_separator.unwrap_or_else(|| " ".to_string()),
            dir_names: dir_names.unwrap_or(false),
            dir_names_max_depth,
            quotepath: quotepath.unwrap_or(true),
            path_separator: path_separator.unwrap_or_else(|| " ".to_string()),
            dir_names_exclude_current_dir: dir_names_exclude_current_dir.unwrap_or(false),
            dir_names_include_files,
            dir_names_deleted_files_include_only_deleted_dirs:
                dir_names_deleted_files_include_only_deleted_dirs.unwrap_or(false),
            include_submodules: include_submodules.unwrap_or(false),
            submodule_filter,
            fetch_depth: fetch_depth.unwrap_or(0),
            fetch_additional_submodule_history: fetch_additional_submodule_history.unwrap_or(false),
            json: json.unwrap_or(true),
            escape_json: escape_json.unwrap_or(true),
            safe_output: safe_output.unwrap_or(true),
            output_dir,
            skip_initial_fetch: skip_initial_fetch.unwrap_or(false),
            use_rest_api: use_rest_api.unwrap_or(false),
            api_url,
            token,
            write_output_files: write_output_files.unwrap_or(false),
            negation_patterns_first: negation_patterns_first.unwrap_or(false),
            match_gitignore_files: match_gitignore_files.unwrap_or(false),
            recover_deleted_files: recover_deleted_files.unwrap_or(false),
            exclude_symlinks: exclude_symlinks.unwrap_or(false),
            tags_pattern,
            tags_ignore_pattern,
            fail_on_initial_diff_error: fail_on_initial_diff_error.unwrap_or(true),
            fail_on_submodule_diff_error: fail_on_submodule_diff_error.unwrap_or(false),
            skip_same_sha: skip_same_sha.unwrap_or(false),
            output_renamed_as_deleted_added: output_renamed_as_deleted_added.unwrap_or(false),
            use_posix_path_separator: use_posix_path_separator.unwrap_or(false),
            track_workflow_failures: track_workflow_failures.unwrap_or(false),
            workflow_lookback_commits: workflow_lookback_commits.unwrap_or(5),
            wait_for_active_workflows: wait_for_active_workflows.unwrap_or(true),
            workflow_max_wait_seconds: workflow_max_wait_seconds.unwrap_or(300),
            include_failed_files: include_failed_files.unwrap_or(true),
            failure_tracking_level,
            workflow_success_lookback: workflow_success_lookback.unwrap_or(5),
            skip_successful_files: skip_successful_files.unwrap_or(true),
            workflow_name_filter,
            files_group_by,
            files_group_by_key,
            files_ancestor_lookup_depth: files_ancestor_lookup_depth.unwrap_or(0),
            deploy_matrix_include_reason: deploy_matrix_include_reason.unwrap_or(false),
            deploy_matrix_include_concurrency: deploy_matrix_include_concurrency.unwrap_or(false),
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "Config(json={}, diff_filter={})",
            self.json, self.diff_filter
        )
    }
}

impl PyConfig {
    /// Convert to core InputConfig (zero-copy: borrows from self)
    pub fn to_core_config(&self) -> lechange_core::InputConfig<'_> {
        lechange_core::InputConfig {
            base_sha: self.base_sha.as_deref().map(Cow::Borrowed),
            sha: self.sha.as_deref().map(Cow::Borrowed),
            since: self.since.as_deref().map(Cow::Borrowed),
            until: self.until.as_deref().map(Cow::Borrowed),
            files: self
                .files
                .as_ref()
                .map(|v| v.iter().map(|s| Cow::Borrowed(s.as_str())).collect()),
            files_separator: Cow::Borrowed(&self.files_separator),
            files_ignore: self
                .files_ignore
                .as_ref()
                .map(|v| v.iter().map(|s| Cow::Borrowed(s.as_str())).collect()),
            files_ignore_separator: Cow::Borrowed(&self.files_ignore_separator),
            files_yaml: self.files_yaml.as_deref().map(Cow::Borrowed),
            files_yaml_from_source_file: self
                .files_yaml_from_source_file
                .as_deref()
                .map(Cow::Borrowed),
            files_from_source_file: self.files_from_source_file.as_deref().map(Cow::Borrowed),
            files_from_source_file_separator: Cow::Borrowed(&self.files_from_source_file_separator),
            diff_filter: Cow::Borrowed(&self.diff_filter),
            include_all_old_new_renamed_files: self.include_all_old_new_renamed_files,
            old_new_separator: Cow::Borrowed(&self.old_new_separator),
            old_new_files_separator: Cow::Borrowed(&self.old_new_files_separator),
            dir_names: self.dir_names,
            dir_names_max_depth: self.dir_names_max_depth,
            quotepath: self.quotepath,
            path_separator: Cow::Borrowed(&self.path_separator),
            dir_names_exclude_current_dir: self.dir_names_exclude_current_dir,
            dir_names_include_files: self
                .dir_names_include_files
                .as_ref()
                .map(|v| v.iter().map(|s| Cow::Borrowed(s.as_str())).collect()),
            dir_names_deleted_files_include_only_deleted_dirs: self
                .dir_names_deleted_files_include_only_deleted_dirs,
            include_submodules: self.include_submodules,
            submodule_filter: self.submodule_filter.as_deref().map(Cow::Borrowed),
            fetch_depth: self.fetch_depth,
            fetch_additional_submodule_history: self.fetch_additional_submodule_history,
            json: self.json,
            escape_json: self.escape_json,
            safe_output: self.safe_output,
            output_dir: self.output_dir.as_deref().map(Cow::Borrowed),
            skip_initial_fetch: self.skip_initial_fetch,
            use_rest_api: self.use_rest_api,
            api_url: self.api_url.as_deref().map(Cow::Borrowed),
            token: self.token.as_deref().map(Cow::Borrowed),
            write_output_files: self.write_output_files,
            negation_patterns_first: self.negation_patterns_first,
            match_gitignore_files: self.match_gitignore_files,
            recover_deleted_files: self.recover_deleted_files,
            exclude_symlinks: self.exclude_symlinks,
            tags_pattern: self.tags_pattern.as_deref().map(Cow::Borrowed),
            tags_ignore_pattern: self.tags_ignore_pattern.as_deref().map(Cow::Borrowed),
            fail_on_initial_diff_error: self.fail_on_initial_diff_error,
            fail_on_submodule_diff_error: self.fail_on_submodule_diff_error,
            skip_same_sha: self.skip_same_sha,
            output_renamed_as_deleted_added: self.output_renamed_as_deleted_added,
            use_posix_path_separator: self.use_posix_path_separator,
            track_workflow_failures: self.track_workflow_failures,
            workflow_lookback_commits: self.workflow_lookback_commits,
            wait_for_active_workflows: self.wait_for_active_workflows,
            workflow_max_wait_seconds: self.workflow_max_wait_seconds,
            include_failed_files: self.include_failed_files,
            failure_tracking_level: match self.failure_tracking_level.as_deref() {
                Some("job") | Some("Job") => lechange_core::FailureTrackingLevel::Job,
                _ => lechange_core::FailureTrackingLevel::Run,
            },
            workflow_success_lookback: self.workflow_success_lookback,
            skip_successful_files: self.skip_successful_files,
            workflow_name_filter: self.workflow_name_filter.as_deref().map(Cow::Borrowed),
            files_group_by: self.files_group_by.as_deref().map(Cow::Borrowed),
            files_group_by_key: self.files_group_by_key.as_deref().map(Cow::Borrowed),
            files_ancestor_lookup_depth: self.files_ancestor_lookup_depth,
            deploy_matrix_include_reason: self.deploy_matrix_include_reason,
            deploy_matrix_include_concurrency: self.deploy_matrix_include_concurrency,
        }
    }
}
