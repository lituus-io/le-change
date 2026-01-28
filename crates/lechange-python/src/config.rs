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
    pub sha256: Option<String>,
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
        diff_filter=None,
        include_all_old_new_renamed_files=None,
        old_new_separator=None,
        old_new_files_separator=None,
        dir_names=None,
        dir_names_max_depth=None,
        quotepath=None,
        path_separator=None,
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
        sha256=None
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
        diff_filter: Option<String>,
        include_all_old_new_renamed_files: Option<bool>,
        old_new_separator: Option<String>,
        old_new_files_separator: Option<String>,
        dir_names: Option<bool>,
        dir_names_max_depth: Option<u32>,
        quotepath: Option<bool>,
        path_separator: Option<String>,
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
        sha256: Option<String>,
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
            diff_filter: diff_filter.unwrap_or_else(|| "ACDMRTUX".to_string()),
            include_all_old_new_renamed_files: include_all_old_new_renamed_files.unwrap_or(false),
            old_new_separator: old_new_separator.unwrap_or_else(|| " ".to_string()),
            old_new_files_separator: old_new_files_separator.unwrap_or_else(|| " ".to_string()),
            dir_names: dir_names.unwrap_or(false),
            dir_names_max_depth,
            quotepath: quotepath.unwrap_or(true),
            path_separator: path_separator.unwrap_or_else(|| " ".to_string()),
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
            sha256,
        }
    }

    fn __repr__(&self) -> String {
        format!("Config(json={}, diff_filter={})", self.json, self.diff_filter)
    }
}

impl PyConfig {
    /// Convert to core InputConfig
    pub fn to_core_config(&self) -> lechange_core::InputConfig<'static> {
        lechange_core::InputConfig {
            base_sha: self.base_sha.as_ref().map(|s| Cow::Owned(s.clone())),
            sha: self.sha.as_ref().map(|s| Cow::Owned(s.clone())),
            since: self.since.as_ref().map(|s| Cow::Owned(s.clone())),
            until: self.until.as_ref().map(|s| Cow::Owned(s.clone())),
            files: self.files.as_ref().map(|v|
                v.iter().map(|s| Cow::Owned(s.clone())).collect()
            ),
            files_separator: Cow::Owned(self.files_separator.clone()),
            files_ignore: self.files_ignore.as_ref().map(|v|
                v.iter().map(|s| Cow::Owned(s.clone())).collect()
            ),
            files_ignore_separator: Cow::Owned(self.files_ignore_separator.clone()),
            diff_filter: Cow::Owned(self.diff_filter.clone()),
            include_all_old_new_renamed_files: self.include_all_old_new_renamed_files,
            old_new_separator: Cow::Owned(self.old_new_separator.clone()),
            old_new_files_separator: Cow::Owned(self.old_new_files_separator.clone()),
            dir_names: self.dir_names,
            dir_names_max_depth: self.dir_names_max_depth,
            quotepath: self.quotepath,
            path_separator: Cow::Owned(self.path_separator.clone()),
            include_submodules: self.include_submodules,
            submodule_filter: self.submodule_filter.as_ref().map(|s| Cow::Owned(s.clone())),
            fetch_depth: self.fetch_depth,
            fetch_additional_submodule_history: self.fetch_additional_submodule_history,
            json: self.json,
            escape_json: self.escape_json,
            safe_output: self.safe_output,
            output_dir: self.output_dir.as_ref().map(|s| Cow::Owned(s.clone())),
            skip_initial_fetch: self.skip_initial_fetch,
            use_rest_api: self.use_rest_api,
            api_url: self.api_url.as_ref().map(|s| Cow::Owned(s.clone())),
            token: self.token.as_ref().map(|s| Cow::Owned(s.clone())),
            write_output_files: self.write_output_files,
            negation_patterns_first: self.negation_patterns_first,
            match_gitignore_files: self.match_gitignore_files,
            recover_deleted_files: self.recover_deleted_files,
            exclude_symlinks: self.exclude_symlinks,
            sha256: self.sha256.as_ref().map(|s| Cow::Owned(s.clone())),
        }
    }
}
