//! Main file processing coordinator

use crate::coordination::ci_decision::CiDecisionEngine;
use crate::coordination::{extract_owner_repo, WorkflowTracker};
use crate::error::Result;
use crate::file_ops::FileOps;
use crate::git::{GitRepository, ShaResolver, SubmoduleProcessor};
use crate::http::{GitHubApiClient, WorkflowApiClient};
use crate::interner::StringInterner;
use crate::patterns::loader::{PatternGroup, PatternLoader};
use crate::patterns::matcher::PatternMatcher;
use crate::traits::AsyncGitOps;
use crate::types::{
    Diagnostic, DiagnosticCategory, DiagnosticSeverity, GroupResult, InputConfig, ProcessedResult,
    WorkflowCheckResult,
};
use rayon::prelude::*;
use std::path::Path;

/// File processor that orchestrates the entire detection pipeline
pub struct FileProcessor<'a> {
    git_ops: &'a GitRepository,
    interner: &'a StringInterner,
    config: &'a InputConfig<'a>,
}

impl<'a> FileProcessor<'a> {
    /// Create a new file processor
    pub fn new(
        git_ops: &'a GitRepository,
        interner: &'a StringInterner,
        config: &'a InputConfig<'a>,
    ) -> Self {
        Self {
            git_ops,
            interner,
            config,
        }
    }

    /// Main processing pipeline — returns ProcessedResult with index-based partitioning
    pub async fn process(&self) -> Result<ProcessedResult> {
        let mut result = ProcessedResult::default();

        // Step 1: Check if we should use REST API
        if self.config.use_rest_api {
            return self.process_via_rest_api().await;
        }

        // Step 2: Resolve SHAs
        let repo_path = self.git_ops.path();
        let sha_resolver = ShaResolver::new(repo_path);
        let (base_sha, head_sha) = sha_resolver.resolve_event_aware(self.config)?;

        // Step 2b: Skip if same SHA
        if self.config.skip_same_sha && base_sha == head_sha {
            result.diagnostics.push(Diagnostic {
                severity: DiagnosticSeverity::Warning,
                category: DiagnosticCategory::SkippedSameSha,
                message: format!("Skipped: base and head SHA are identical ({})", base_sha),
            });
            return Ok(result);
        }

        // Step 3: Compute diff (with soft-fail support)
        let diff = match self
            .git_ops
            .diff(
                &base_sha,
                &head_sha,
                self.interner,
                &self.config.diff_filter,
            )
            .await
        {
            Ok(diff) => diff,
            Err(e) => {
                if self.config.fail_on_initial_diff_error {
                    return Err(e);
                }
                result.diagnostics.push(Diagnostic {
                    severity: DiagnosticSeverity::SoftError,
                    category: DiagnosticCategory::InitialDiff,
                    message: format!("Initial diff failed (soft): {}", e),
                });
                return Ok(result);
            }
        };

        result.all_files = diff.files;
        result.additions = diff.additions;
        result.deletions = diff.deletions;

        // Step 4: Process submodules if enabled
        if self.config.include_submodules {
            match self.process_submodules(&base_sha, &head_sha).await {
                Ok(submodule_diff) => {
                    result.all_files.extend(submodule_diff.files);
                }
                Err(e) => {
                    if self.config.fail_on_submodule_diff_error {
                        return Err(e);
                    }
                    result.diagnostics.push(Diagnostic {
                        severity: DiagnosticSeverity::SoftError,
                        category: DiagnosticCategory::SubmoduleDiff,
                        message: format!("Submodule diff failed (soft): {}", e),
                    });
                }
            }
        }

        // Step 5a: Load YAML groups (if configured) — needed by both workflow tracker and group filtering
        let yaml_groups = self.load_yaml_groups(&mut result);

        // Step 5b: Workflow intelligence (pass loaded groups to tracker for job-level partitioning)
        if self.config.track_workflow_failures {
            match self
                .check_workflows_enhanced(&mut result.all_files, &yaml_groups)
                .await
            {
                Ok(workflow_result) => {
                    // Compute CI decision
                    let ci_engine = CiDecisionEngine::new(self.interner);
                    let ci_decision = ci_engine.compute(
                        &result.all_files,
                        &workflow_result.failures,
                        &workflow_result.successes,
                    );

                    if workflow_result.waited {
                        eprintln!(
                            "Waited {}ms for {} active workflows",
                            workflow_result.wait_time_ms,
                            workflow_result.blocking_runs.len()
                        );
                    }

                    if !workflow_result.failures.is_empty() {
                        eprintln!(
                            "Found {} recent failures on branch",
                            workflow_result.failures.len()
                        );
                    }

                    result.ci_decision = Some(ci_decision);
                    result.workflow_result = Some(workflow_result);
                }
                Err(e) => {
                    result.diagnostics.push(Diagnostic {
                        severity: DiagnosticSeverity::SoftError,
                        category: DiagnosticCategory::WorkflowApi,
                        message: format!("Workflow check failed (soft): {}", e),
                    });
                }
            }
        }

        // Step 6: Load patterns and partition files
        let matcher = self.build_pattern_matcher()?;

        if let Some(matcher) = &matcher {
            let (matched, unmatched) =
                matcher.partition_files_parallel(&result.all_files, self.interner);
            result.filtered_indices = matched;
            result.unmatched_indices = unmatched;
            result.pattern_applied = true;
        } else {
            // No patterns — all files are "matched"
            let n = result.all_files.len() as u32;
            result.filtered_indices = (0..n).collect();
            result.unmatched_indices = Vec::new();
            result.pattern_applied = false;
        }

        // Step 6b: Ancestor recovery — recover unmatched files whose ancestor directories
        // contain files that DO match the pattern
        if self.config.files_ancestor_lookup_depth > 0 {
            if let Some(ref matcher) = matcher {
                self.recover_unmatched_by_ancestor(
                    &result.all_files,
                    &mut result.filtered_indices,
                    &mut result.unmatched_indices,
                    matcher,
                    &mut result.diagnostics,
                );
            }
        }

        // Step 7: YAML group filtering (reuse already-loaded groups)
        if !yaml_groups.is_empty() {
            result.group_results = yaml_groups
                .iter()
                .map(|group| {
                    let matched: Vec<u32> = result
                        .filtered_indices
                        .iter()
                        .copied()
                        .filter(|&i| {
                            let file = &result.all_files[i as usize];
                            self.interner
                                .resolve(file.path)
                                .map(|path| group.matcher.matches_sync(path))
                                .unwrap_or(false)
                        })
                        .collect();
                    GroupResult {
                        key: self.interner.intern(&group.name),
                        matched_indices: matched,
                    }
                })
                .collect();
        }

        // Step 8: Detect symlinks (parallel)
        if !self.config.exclude_symlinks {
            self.detect_symlinks_parallel(&mut result.all_files);
        }

        // Step 9: Recover deleted files if configured
        if self.config.recover_deleted_files {
            if let Some(ref output_dir) = self.config.output_dir {
                let recovery = crate::git::recovery::FileRecovery::new(repo_path);
                let deleted_paths: Vec<crate::types::InternedString> = result
                    .all_files
                    .iter()
                    .filter(|f| f.change_type == crate::types::ChangeType::Deleted)
                    .map(|f| f.path)
                    .collect();
                if !deleted_paths.is_empty() {
                    let results = recovery.recover_all_parallel(
                        &base_sha,
                        &deleted_paths,
                        self.interner,
                        Path::new(output_dir.as_ref()),
                    );
                    for res in results {
                        if let Err(e) = res {
                            result.diagnostics.push(Diagnostic {
                                severity: DiagnosticSeverity::SoftError,
                                category: DiagnosticCategory::SymlinkDetection,
                                message: format!("File recovery failed: {}", e),
                            });
                        }
                    }
                }
            }
        }

        // Step 10: Write output files if configured
        if self.config.write_output_files {
            if let Some(ref output_dir) = self.config.output_dir {
                let dir = Path::new(output_dir.as_ref());
                std::fs::create_dir_all(dir)?;

                let blocked_groups = result.workflow_result.as_ref().map(|wr| &wr.blocked_groups);
                let outputs = crate::output::ComputedOutputs::compute_with_concurrency(
                    &result,
                    self.config.output_renamed_as_deleted_added,
                    blocked_groups,
                    Some(self.interner),
                );

                // Write filtered files list
                let filtered_paths: Vec<&str> = result
                    .filtered_indices
                    .iter()
                    .filter_map(|&i| self.interner.resolve(result.all_files[i as usize].path))
                    .collect();
                if let Err(e) = crate::output::writer::OutputWriter::write_text(
                    dir,
                    "all_changed_files",
                    &filtered_paths,
                    &self.config.files_separator,
                ) {
                    result.diagnostics.push(Diagnostic {
                        severity: DiagnosticSeverity::SoftError,
                        category: DiagnosticCategory::PatternLoad,
                        message: format!("Failed to write output file: {}", e),
                    });
                }

                // Write JSON output
                if self.config.json {
                    if let Err(e) = crate::output::writer::OutputWriter::write_json(
                        dir,
                        "all_changed_files",
                        &filtered_paths,
                    ) {
                        result.diagnostics.push(Diagnostic {
                            severity: DiagnosticSeverity::SoftError,
                            category: DiagnosticCategory::PatternLoad,
                            message: format!("Failed to write JSON output: {}", e),
                        });
                    }
                }

                // Write per-change-type outputs
                let type_categories = [
                    ("added_files", &outputs.filtered_added),
                    ("modified_files", &outputs.filtered_modified),
                    ("deleted_files", &outputs.filtered_deleted),
                    ("renamed_files", &outputs.filtered_renamed),
                    ("copied_files", &outputs.filtered_copied),
                ];

                for (name, indices) in &type_categories {
                    let paths: Vec<&str> = indices
                        .iter()
                        .filter_map(|&i| self.interner.resolve(result.all_files[i as usize].path))
                        .collect();
                    let _ = crate::output::writer::OutputWriter::write_text(
                        dir,
                        name,
                        &paths,
                        &self.config.files_separator,
                    );
                }
            }
        }

        Ok(result)
    }

    /// Build a combined pattern matcher from all sources (inline, source file, YAML)
    fn build_pattern_matcher(&self) -> Result<Option<PatternMatcher>> {
        // Source file patterns (must be loaded into owned buffer)
        let mut source_buf = String::new();
        let mut source_patterns: Vec<&str> = Vec::new();
        if let Some(ref source_file) = self.config.files_from_source_file {
            match PatternLoader::load_from_file(source_file, &mut source_buf) {
                Ok(patterns) => {
                    source_patterns = patterns;
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }

        // Inline patterns — borrow directly from config, no .to_string()
        let inline_refs: Vec<&str> = self
            .config
            .files
            .as_ref()
            .map(|v| v.iter().map(|c| c.as_ref()).collect())
            .unwrap_or_default();

        if inline_refs.is_empty() && source_patterns.is_empty() {
            return Ok(None);
        }

        let patterns: Vec<&str> = inline_refs.into_iter().chain(source_patterns).collect();

        // .gitignore integration
        let mut gitignore_buf = String::new();
        let mut gitignore_patterns: Vec<&str> = Vec::new();
        if self.config.match_gitignore_files {
            let gitignore_path = std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join(".gitignore");
            if gitignore_path.exists() {
                if let Ok(loaded) = PatternLoader::load_from_file(
                    gitignore_path.to_str().unwrap_or(""),
                    &mut gitignore_buf,
                ) {
                    gitignore_patterns = loaded;
                }
            }
        }

        let mut ignore_patterns: Vec<&str> = self
            .config
            .files_ignore
            .as_ref()
            .map(|ignore| ignore.iter().map(|c| c.as_ref()).collect())
            .unwrap_or_default();

        // Add .gitignore patterns as excludes
        ignore_patterns.extend(gitignore_patterns);

        let matcher = PatternMatcher::new(
            &patterns,
            &ignore_patterns,
            self.config.negation_patterns_first,
        )?;

        Ok(Some(matcher))
    }

    /// Recover unmatched files whose ancestor directories contain pattern-matched files.
    ///
    /// Scans up to `files_ancestor_lookup_depth` ancestor directories for any file
    /// that matches the current pattern. If found, the unmatched file is moved to
    /// the filtered set so it participates in group matching.
    fn recover_unmatched_by_ancestor(
        &self,
        all_files: &[crate::types::ChangedFile],
        filtered: &mut Vec<u32>,
        unmatched: &mut Vec<u32>,
        matcher: &PatternMatcher,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let depth = self.config.files_ancestor_lookup_depth.min(3);
        if depth != self.config.files_ancestor_lookup_depth {
            diagnostics.push(Diagnostic {
                severity: DiagnosticSeverity::Warning,
                category: DiagnosticCategory::AncestorRecovery,
                message: format!(
                    "files_ancestor_lookup_depth clamped from {} to 3 (maximum)",
                    self.config.files_ancestor_lookup_depth
                ),
            });
        }

        let workdir = self.git_ops.workdir();
        let mut recovered = Vec::new();

        unmatched.retain(|&idx| {
            let path = match self.interner.resolve(all_files[idx as usize].path) {
                Some(p) => p,
                None => return true, // keep in unmatched
            };
            if self.has_matching_ancestor_file(path, workdir, matcher, depth, diagnostics) {
                recovered.push(idx);
                false // remove from unmatched
            } else {
                true // keep in unmatched
            }
        });

        if !recovered.is_empty() {
            diagnostics.push(Diagnostic {
                severity: DiagnosticSeverity::Warning,
                category: DiagnosticCategory::AncestorRecovery,
                message: format!(
                    "Recovered {} file(s) via ancestor directory lookup (depth={})",
                    recovered.len(),
                    depth
                ),
            });
            filtered.extend(recovered);
        }
    }

    /// Check if any file in the ancestor directory hierarchy matches the pattern.
    fn has_matching_ancestor_file(
        &self,
        file_path: &str,
        workdir: &Path,
        matcher: &PatternMatcher,
        max_depth: u32,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> bool {
        let rel = Path::new(file_path);
        let mut current_dir = match rel.parent() {
            Some(d) => d.to_path_buf(),
            None => return false,
        };

        for _ in 0..max_depth {
            let abs_dir = workdir.join(&current_dir);
            match std::fs::read_dir(&abs_dir) {
                Ok(entries) => {
                    for entry in entries.flatten() {
                        if entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                            let entry_rel = current_dir.join(entry.file_name());
                            if let Some(s) = entry_rel.to_str() {
                                if matcher.matches_sync(s) {
                                    return true;
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    diagnostics.push(Diagnostic {
                        severity: DiagnosticSeverity::SoftError,
                        category: DiagnosticCategory::AncestorRecovery,
                        message: format!("Failed to scan directory '{}': {}", abs_dir.display(), e),
                    });
                }
            }
            // Move to parent
            match current_dir.parent() {
                Some(p) if !p.as_os_str().is_empty() => current_dir = p.to_path_buf(),
                _ => break,
            }
        }
        false
    }

    /// Load YAML content from inline or file source
    fn load_yaml_content(&self) -> Result<Option<String>> {
        if let Some(ref yaml) = self.config.files_yaml {
            return Ok(Some(yaml.to_string()));
        }

        if let Some(ref yaml_file) = self.config.files_yaml_from_source_file {
            let content = std::fs::read_to_string(yaml_file.as_ref()).map_err(|e| {
                crate::error::Error::Pattern(format!(
                    "Failed to read YAML pattern file '{}': {}",
                    yaml_file, e
                ))
            })?;
            return Ok(Some(content));
        }

        Ok(None)
    }

    /// Load YAML groups once for reuse by both workflow tracker and group filtering.
    ///
    /// Priority: files_yaml > files_group_by. If both set, YAML wins with a diagnostic.
    fn load_yaml_groups(&self, result: &mut ProcessedResult) -> Vec<PatternGroup> {
        match self.load_yaml_content() {
            Ok(Some(yaml_content)) => {
                if self.config.files_group_by.is_some() {
                    result.diagnostics.push(Diagnostic {
                        severity: DiagnosticSeverity::Warning,
                        category: DiagnosticCategory::PatternLoad,
                        message: "Both files_yaml and files_group_by set; using files_yaml".into(),
                    });
                }
                match PatternLoader::load_yaml_groups(
                    &yaml_content,
                    self.config.negation_patterns_first,
                ) {
                    Ok(groups) => return groups,
                    Err(e) => {
                        result.diagnostics.push(Diagnostic {
                            severity: DiagnosticSeverity::SoftError,
                            category: DiagnosticCategory::PatternLoad,
                            message: format!("YAML pattern load failed: {}", e),
                        });
                        return Vec::new();
                    }
                }
            }
            Ok(None) => {} // No YAML — fall through to group_by
            Err(e) => {
                result.diagnostics.push(Diagnostic {
                    severity: DiagnosticSeverity::SoftError,
                    category: DiagnosticCategory::PatternLoad,
                    message: format!("Failed to load YAML content: {}", e),
                });
                return Vec::new();
            }
        }

        // Try files_group_by template discovery
        if let Some(ref template_str) = self.config.files_group_by {
            match PatternLoader::parse_group_by_template(template_str) {
                Ok(template) => {
                    let key_mode = self
                        .config
                        .files_group_by_key
                        .as_deref()
                        .map(crate::types::GroupByKey::parse)
                        .unwrap_or_default();

                    let repo_root = self.git_ops.workdir();
                    match PatternLoader::discover_groups_from_template(
                        &template,
                        repo_root,
                        self.config.negation_patterns_first,
                        key_mode,
                    ) {
                        Ok(groups) => return groups,
                        Err(e) => {
                            result.diagnostics.push(Diagnostic {
                                severity: DiagnosticSeverity::SoftError,
                                category: DiagnosticCategory::PatternLoad,
                                message: format!("files_group_by discovery failed: {}", e),
                            });
                        }
                    }
                }
                Err(e) => {
                    result.diagnostics.push(Diagnostic {
                        severity: DiagnosticSeverity::SoftError,
                        category: DiagnosticCategory::PatternLoad,
                        message: format!("files_group_by template error: {}", e),
                    });
                }
            }
        }

        Vec::new()
    }

    /// Enhanced workflow checking with success tracking and job-level detail
    async fn check_workflows_enhanced(
        &self,
        current_files: &mut Vec<crate::types::ChangedFile>,
        yaml_groups: &[PatternGroup],
    ) -> Result<WorkflowCheckResult> {
        let current_branch =
            std::env::var("GITHUB_REF").unwrap_or_else(|_| "refs/heads/main".to_string());
        let branch = current_branch
            .strip_prefix("refs/heads/")
            .unwrap_or(&current_branch);

        let api_client = WorkflowApiClient::from_env()?;
        let tracker = WorkflowTracker::new(api_client, self.config, self.interner, yaml_groups);

        let result = tracker.check_workflows(branch, current_files).await?;

        // Merge failed files into current file list
        tracker.merge_failed_files(current_files, &result.failures);

        Ok(result)
    }

    /// Process via GitHub REST API
    async fn process_via_rest_api(&self) -> Result<ProcessedResult> {
        if self.config.files_ancestor_lookup_depth > 0 {
            eprintln!("Warning: files_ancestor_lookup_depth not available in REST API mode");
        }
        let client = GitHubApiClient::from_env()?;

        let diff = match GitHubApiClient::extract_pr_info_from_env() {
            Ok((owner, repo, pr_number)) => {
                client
                    .fetch_changed_files(&owner, &repo, pr_number, self.interner)
                    .await?
            }
            Err(_) => {
                // Non-PR context: use compare_refs with resolved SHAs
                let sha_resolver = ShaResolver::new(self.git_ops.path());
                let (base_sha, head_sha) = sha_resolver.resolve_event_aware(self.config)?;
                let (owner, repo) = extract_owner_repo()?;
                client
                    .compare_refs(&owner, &repo, &base_sha, &head_sha, self.interner)
                    .await?
            }
        };

        let mut result = ProcessedResult::from_unfiltered(diff);

        // Apply pattern filtering if configured
        let matcher = self.build_pattern_matcher()?;

        if let Some(matcher) = &matcher {
            let (matched, unmatched) =
                matcher.partition_files_parallel(&result.all_files, self.interner);
            result.filtered_indices = matched;
            result.unmatched_indices = unmatched;
            result.pattern_applied = true;
        }

        Ok(result)
    }

    /// Process submodules recursively
    async fn process_submodules(
        &self,
        base_sha: &str,
        head_sha: &str,
    ) -> Result<crate::types::DiffResult> {
        let max_depth = self.config.dir_names_max_depth.map(|d| d as u8);
        let processor = SubmoduleProcessor::new(self.git_ops.path(), max_depth);

        processor.process_submodule_changes(
            base_sha,
            head_sha,
            self.interner,
            0, // Start at depth 0
        )
    }

    /// Detect symlinks in parallel using Rayon
    fn detect_symlinks_parallel(&self, files: &mut [crate::types::ChangedFile]) {
        let file_ops = FileOps::new();

        files.par_iter_mut().for_each(|file| {
            if let Some(path_str) = self.interner.resolve(file.path) {
                let path = Path::new(path_str);
                if let Ok(is_link) = file_ops.is_symlink_sync(path) {
                    file.is_symlink = is_link;
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::GitRepository;
    use crate::interner::StringInterner;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_repo() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let repo_path = dir.path().to_path_buf();

        // Initialize git repo
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        std::process::Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        std::process::Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        // Create initial commit
        fs::write(repo_path.join("file1.txt"), "content1").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "Initial commit"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        (dir, repo_path)
    }

    #[tokio::test]
    async fn test_processor_basic_flow() {
        let (_dir, repo_path) = create_test_repo();

        // Create second commit
        fs::write(repo_path.join("file2.txt"), "content2").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "Add file2"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        // Change to repo directory
        std::env::set_current_dir(&repo_path).unwrap();

        let repo = GitRepository::discover(&repo_path).unwrap();
        let interner = StringInterner::new();
        let config = InputConfig::default();

        let processor = FileProcessor::new(&repo, &interner, &config);
        let result = processor.process().await.unwrap();

        assert_eq!(result.all_files.len(), 1);
        assert_eq!(
            interner.resolve(result.all_files[0].path),
            Some("file2.txt")
        );
        assert!(!result.pattern_applied);
        assert_eq!(result.filtered_indices, vec![0]);
        assert!(result.unmatched_indices.is_empty());
    }

    // --- Ancestor recovery tests ---

    /// Helper: set up a repo with stacks/prod/*.yaml and a changed .sql in migrations subdir
    fn setup_ancestor_test() -> (TempDir, std::path::PathBuf) {
        let (dir, repo_path) = create_test_repo();

        // Create stacks/prod/ with a matching .yaml file
        fs::create_dir_all(repo_path.join("stacks/prod")).unwrap();
        fs::write(repo_path.join("stacks/prod/config.yaml"), "key: value").unwrap();

        // Create stacks/prod/migrations/ with a .sql file
        fs::create_dir_all(repo_path.join("stacks/prod/migrations")).unwrap();
        fs::write(
            repo_path.join("stacks/prod/migrations/001.sql"),
            "CREATE TABLE t;",
        )
        .unwrap();

        // Create stacks/dev/ with a .yaml file
        fs::create_dir_all(repo_path.join("stacks/dev")).unwrap();
        fs::write(repo_path.join("stacks/dev/config.yaml"), "env: dev").unwrap();

        (dir, repo_path)
    }

    #[test]
    fn test_ancestor_recovery_same_dir() {
        // depth=1: .sql in same dir as .yaml → recovered
        let (_dir, repo_path) = setup_ancestor_test();
        let repo = GitRepository::discover(&repo_path).unwrap();
        let interner = StringInterner::new();

        // Pattern matches stacks/prod/**/*.yaml
        let matcher = PatternMatcher::new(&["stacks/prod/**/*.yaml"], &[], true).unwrap();

        // The .yaml file matches, but the .sql doesn't
        let sql_path = interner.intern("stacks/prod/config.sql");
        let yaml_path = interner.intern("stacks/prod/config.yaml");

        // Place a .sql next to the .yaml — create the file on disk first
        fs::write(repo_path.join("stacks/prod/config.sql"), "SELECT 1").unwrap();

        let all_files = vec![
            crate::types::ChangedFile {
                path: yaml_path,
                change_type: crate::types::ChangeType::Modified,
                previous_path: None,
                is_symlink: false,
                submodule_depth: 0,
                origin: crate::types::FileOrigin::default(),
            },
            crate::types::ChangedFile {
                path: sql_path,
                change_type: crate::types::ChangeType::Added,
                previous_path: None,
                is_symlink: false,
                submodule_depth: 0,
                origin: crate::types::FileOrigin::default(),
            },
        ];

        let mut filtered = vec![0u32]; // yaml matches
        let mut unmatched = vec![1u32]; // sql doesn't match
        let mut diagnostics = Vec::new();

        let config = InputConfig {
            files_ancestor_lookup_depth: 1,
            ..Default::default()
        };

        let processor = FileProcessor::new(&repo, &interner, &config);
        processor.recover_unmatched_by_ancestor(
            &all_files,
            &mut filtered,
            &mut unmatched,
            &matcher,
            &mut diagnostics,
        );

        // sql should be recovered (same dir has config.yaml which matches pattern)
        assert!(unmatched.is_empty());
        assert_eq!(filtered, vec![0, 1]);
        assert!(diagnostics
            .iter()
            .any(|d| d.message.contains("Recovered 1")));
    }

    #[test]
    fn test_ancestor_recovery_parent_dir() {
        // depth=2: .sql in subdir, .yaml in parent → recovered
        let (_dir, repo_path) = setup_ancestor_test();
        let repo = GitRepository::discover(&repo_path).unwrap();
        let interner = StringInterner::new();

        let matcher = PatternMatcher::new(&["stacks/prod/**/*.yaml"], &[], true).unwrap();

        let sql_path = interner.intern("stacks/prod/migrations/001.sql");

        let all_files = vec![crate::types::ChangedFile {
            path: sql_path,
            change_type: crate::types::ChangeType::Added,
            previous_path: None,
            is_symlink: false,
            submodule_depth: 0,
            origin: crate::types::FileOrigin::default(),
        }];

        let mut filtered = Vec::new();
        let mut unmatched = vec![0u32];
        let mut diagnostics = Vec::new();

        let config = InputConfig {
            files_ancestor_lookup_depth: 2,
            ..Default::default()
        };

        let processor = FileProcessor::new(&repo, &interner, &config);
        processor.recover_unmatched_by_ancestor(
            &all_files,
            &mut filtered,
            &mut unmatched,
            &matcher,
            &mut diagnostics,
        );

        // migrations/ has no .yaml, but parent stacks/prod/ does → recovered at depth 2
        assert!(unmatched.is_empty());
        assert_eq!(filtered, vec![0]);
        assert!(diagnostics
            .iter()
            .any(|d| d.message.contains("Recovered 1")));
    }

    #[test]
    fn test_ancestor_recovery_depth_zero() {
        // depth=0: no recovery (backward compat)
        let (_dir, repo_path) = setup_ancestor_test();
        let repo = GitRepository::discover(&repo_path).unwrap();
        let interner = StringInterner::new();

        let matcher = PatternMatcher::new(&["stacks/prod/**/*.yaml"], &[], true).unwrap();
        let sql_path = interner.intern("stacks/prod/migrations/001.sql");

        let all_files = vec![crate::types::ChangedFile {
            path: sql_path,
            change_type: crate::types::ChangeType::Added,
            previous_path: None,
            is_symlink: false,
            submodule_depth: 0,
            origin: crate::types::FileOrigin::default(),
        }];

        let mut filtered = Vec::new();
        let mut unmatched = vec![0u32];
        let mut diagnostics = Vec::new();

        // depth=0 → method should not be called, but even if it is, it clamps to 0
        let config = InputConfig {
            files_ancestor_lookup_depth: 0,
            ..Default::default()
        };

        let processor = FileProcessor::new(&repo, &interner, &config);
        // Calling with depth 0 — the process() method won't call this, but test the function directly
        processor.recover_unmatched_by_ancestor(
            &all_files,
            &mut filtered,
            &mut unmatched,
            &matcher,
            &mut diagnostics,
        );

        // depth=0 clamps to min(0,3)=0, loop runs 0 times → no recovery
        assert_eq!(unmatched, vec![0]);
        assert!(filtered.is_empty());
    }

    #[test]
    fn test_ancestor_recovery_no_match() {
        // depth=3, no matching file anywhere → stays unmatched
        let (_dir, repo_path) = setup_ancestor_test();
        let repo = GitRepository::discover(&repo_path).unwrap();
        let interner = StringInterner::new();

        // Pattern that matches nothing in our test setup
        let matcher = PatternMatcher::new(&["nonexistent/**/*.yaml"], &[], true).unwrap();

        let sql_path = interner.intern("stacks/prod/migrations/001.sql");
        let all_files = vec![crate::types::ChangedFile {
            path: sql_path,
            change_type: crate::types::ChangeType::Added,
            previous_path: None,
            is_symlink: false,
            submodule_depth: 0,
            origin: crate::types::FileOrigin::default(),
        }];

        let mut filtered = Vec::new();
        let mut unmatched = vec![0u32];
        let mut diagnostics = Vec::new();

        let config = InputConfig {
            files_ancestor_lookup_depth: 3,
            ..Default::default()
        };

        let processor = FileProcessor::new(&repo, &interner, &config);
        processor.recover_unmatched_by_ancestor(
            &all_files,
            &mut filtered,
            &mut unmatched,
            &matcher,
            &mut diagnostics,
        );

        assert_eq!(unmatched, vec![0]);
        assert!(filtered.is_empty());
    }

    #[test]
    fn test_ancestor_recovery_depth_limit() {
        // depth=1: match only in grandparent → NOT recovered
        let (_dir, repo_path) = setup_ancestor_test();
        let repo = GitRepository::discover(&repo_path).unwrap();
        let interner = StringInterner::new();

        let matcher = PatternMatcher::new(&["stacks/prod/**/*.yaml"], &[], true).unwrap();

        // File is in migrations subdir, yaml is in stacks/prod/ (2 levels up from file's dir)
        let sql_path = interner.intern("stacks/prod/migrations/001.sql");
        let all_files = vec![crate::types::ChangedFile {
            path: sql_path,
            change_type: crate::types::ChangeType::Added,
            previous_path: None,
            is_symlink: false,
            submodule_depth: 0,
            origin: crate::types::FileOrigin::default(),
        }];

        let mut filtered = Vec::new();
        let mut unmatched = vec![0u32];
        let mut diagnostics = Vec::new();

        // depth=1: only scans stacks/prod/migrations/ (no yaml there)
        let config = InputConfig {
            files_ancestor_lookup_depth: 1,
            ..Default::default()
        };

        let processor = FileProcessor::new(&repo, &interner, &config);
        processor.recover_unmatched_by_ancestor(
            &all_files,
            &mut filtered,
            &mut unmatched,
            &matcher,
            &mut diagnostics,
        );

        // Not recovered at depth 1 — yaml is in parent, not same dir
        assert_eq!(unmatched, vec![0]);
        assert!(filtered.is_empty());
    }

    #[test]
    fn test_ancestor_recovery_multiple() {
        // Mixed: one recoverable, one not
        let (_dir, repo_path) = setup_ancestor_test();

        // Create an orphan dir with no yaml files
        fs::create_dir_all(repo_path.join("orphan")).unwrap();
        fs::write(repo_path.join("orphan/data.bin"), "binary").unwrap();

        let repo = GitRepository::discover(&repo_path).unwrap();
        let interner = StringInterner::new();

        let matcher = PatternMatcher::new(&["stacks/**/*.yaml"], &[], true).unwrap();

        let sql_path = interner.intern("stacks/prod/migrations/001.sql");
        let orphan_path = interner.intern("orphan/data.bin");

        let all_files = vec![
            crate::types::ChangedFile {
                path: sql_path,
                change_type: crate::types::ChangeType::Added,
                previous_path: None,
                is_symlink: false,
                submodule_depth: 0,
                origin: crate::types::FileOrigin::default(),
            },
            crate::types::ChangedFile {
                path: orphan_path,
                change_type: crate::types::ChangeType::Added,
                previous_path: None,
                is_symlink: false,
                submodule_depth: 0,
                origin: crate::types::FileOrigin::default(),
            },
        ];

        let mut filtered = Vec::new();
        let mut unmatched = vec![0u32, 1u32];
        let mut diagnostics = Vec::new();

        let config = InputConfig {
            files_ancestor_lookup_depth: 2,
            ..Default::default()
        };

        let processor = FileProcessor::new(&repo, &interner, &config);
        processor.recover_unmatched_by_ancestor(
            &all_files,
            &mut filtered,
            &mut unmatched,
            &matcher,
            &mut diagnostics,
        );

        // sql recovered (parent has yaml), orphan stays
        assert_eq!(unmatched, vec![1]);
        assert_eq!(filtered, vec![0]);
    }

    #[test]
    fn test_ancestor_recovery_clamped() {
        // depth=10 → clamped to 3 with diagnostic
        let (_dir, repo_path) = setup_ancestor_test();
        let repo = GitRepository::discover(&repo_path).unwrap();
        let interner = StringInterner::new();

        let matcher = PatternMatcher::new(&["stacks/**/*.yaml"], &[], true).unwrap();
        let sql_path = interner.intern("stacks/prod/migrations/001.sql");

        let all_files = vec![crate::types::ChangedFile {
            path: sql_path,
            change_type: crate::types::ChangeType::Added,
            previous_path: None,
            is_symlink: false,
            submodule_depth: 0,
            origin: crate::types::FileOrigin::default(),
        }];

        let mut filtered = Vec::new();
        let mut unmatched = vec![0u32];
        let mut diagnostics = Vec::new();

        let config = InputConfig {
            files_ancestor_lookup_depth: 10,
            ..Default::default()
        };

        let processor = FileProcessor::new(&repo, &interner, &config);
        processor.recover_unmatched_by_ancestor(
            &all_files,
            &mut filtered,
            &mut unmatched,
            &matcher,
            &mut diagnostics,
        );

        // Should have a clamping diagnostic
        assert!(diagnostics
            .iter()
            .any(|d| d.message.contains("clamped from 10 to 3")));
    }

    #[test]
    fn test_build_pattern_matcher_with_inline_patterns() {
        use std::borrow::Cow;

        let (_dir, repo_path) = create_test_repo();
        let repo = GitRepository::discover(&repo_path).unwrap();
        let interner = StringInterner::new();

        let config = InputConfig {
            files: Some(vec![
                Cow::Borrowed("src/**/*.rs"),
                Cow::Borrowed("tests/**"),
            ]),
            ..Default::default()
        };

        let processor = FileProcessor::new(&repo, &interner, &config);
        let matcher = processor.build_pattern_matcher().unwrap();

        // With inline patterns, should return Some
        assert!(
            matcher.is_some(),
            "Expected Some(PatternMatcher) when inline patterns are set"
        );

        let matcher = matcher.unwrap();
        assert!(matcher.matches_sync("src/lib.rs"));
        assert!(matcher.matches_sync("src/utils/helpers.rs"));
        assert!(matcher.matches_sync("tests/integration.rs"));
        assert!(!matcher.matches_sync("docs/README.md"));
        assert!(!matcher.matches_sync("Cargo.toml"));
    }

    #[test]
    fn test_build_pattern_matcher_no_patterns() {
        let (_dir, repo_path) = create_test_repo();
        let repo = GitRepository::discover(&repo_path).unwrap();
        let interner = StringInterner::new();

        // Default config has no files, no source file patterns
        let config = InputConfig::default();

        let processor = FileProcessor::new(&repo, &interner, &config);
        let matcher = processor.build_pattern_matcher().unwrap();

        // Without any patterns configured, should return None
        assert!(
            matcher.is_none(),
            "Expected None when no patterns are configured"
        );
    }
}
