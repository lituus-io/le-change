//! Main file processing coordinator

use crate::coordination::ci_decision::CiDecisionEngine;
use crate::coordination::WorkflowTracker;
use crate::error::Result;
use crate::file_ops::FileOps;
use crate::git::{GitRepository, ShaResolver, SubmoduleProcessor};
use crate::http::{GitHubApiClient, WorkflowApiClient};
use crate::interner::StringInterner;
use crate::patterns::loader::PatternLoader;
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

        // Step 5: Workflow intelligence
        if self.config.track_workflow_failures {
            match self.check_workflows_enhanced(&mut result.all_files).await {
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

        // Step 7: YAML group filtering
        if let Some(yaml_content) = self.load_yaml_content()? {
            match PatternLoader::load_yaml_groups(&yaml_content, self.config.negation_patterns_first)
            {
                Ok(groups) => {
                    result.group_results = groups
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
                                key: group.name.clone(),
                                matched_indices: matched,
                            }
                        })
                        .collect();
                }
                Err(e) => {
                    result.diagnostics.push(Diagnostic {
                        severity: DiagnosticSeverity::SoftError,
                        category: DiagnosticCategory::PatternLoad,
                        message: format!("YAML pattern load failed: {}", e),
                    });
                }
            }
        }

        // Step 8: Detect symlinks (parallel)
        if !self.config.exclude_symlinks {
            self.detect_symlinks_parallel(&mut result.all_files);
        }

        // Step 9: Recover deleted files if configured
        if self.config.recover_deleted_files {
            if let Some(ref output_dir) = self.config.output_dir {
                let recovery = crate::git::recovery::FileRecovery::new(&repo_path);
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

                let outputs =
                    crate::output::ComputedOutputs::compute(&result, self.config.output_renamed_as_deleted_added);

                // Write filtered files list
                let filtered_paths: Vec<&str> = result
                    .filtered_indices
                    .iter()
                    .filter_map(|&i| {
                        self.interner
                            .resolve(result.all_files[i as usize].path)
                    })
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
                        .filter_map(|&i| {
                            self.interner
                                .resolve(result.all_files[i as usize].path)
                        })
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
        let mut all_patterns: Vec<String> = Vec::new();

        // Inline patterns
        if let Some(ref files_patterns) = self.config.files {
            all_patterns.extend(files_patterns.iter().map(|c| c.to_string()));
        }

        // Source file patterns
        if let Some(ref source_file) = self.config.files_from_source_file {
            let mut buf = String::new();
            match PatternLoader::load_from_file(source_file, &mut buf) {
                Ok(patterns) => {
                    all_patterns.extend(patterns.iter().map(|s| s.to_string()));
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }

        // .gitignore integration
        let mut gitignore_excludes: Vec<String> = Vec::new();
        if self.config.match_gitignore_files {
            let gitignore_path = std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join(".gitignore");
            if gitignore_path.exists() {
                let mut buf = String::new();
                if let Ok(patterns) =
                    PatternLoader::load_from_file(gitignore_path.to_str().unwrap_or(""), &mut buf)
                {
                    gitignore_excludes.extend(patterns.iter().map(|s| s.to_string()));
                }
            }
        }

        if all_patterns.is_empty() {
            return Ok(None);
        }

        let patterns: Vec<&str> = all_patterns.iter().map(|s| s.as_str()).collect();

        let mut ignore_patterns: Vec<&str> = self
            .config
            .files_ignore
            .as_ref()
            .map(|ignore| ignore.iter().map(|c| c.as_ref()).collect())
            .unwrap_or_default();

        // Add .gitignore patterns as excludes
        for p in &gitignore_excludes {
            ignore_patterns.push(p.as_str());
        }

        let matcher = PatternMatcher::new(
            &patterns,
            &ignore_patterns,
            self.config.negation_patterns_first,
        )?;

        Ok(Some(matcher))
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

    /// Enhanced workflow checking with success tracking and job-level detail
    async fn check_workflows_enhanced(
        &self,
        current_files: &mut Vec<crate::types::ChangedFile>,
    ) -> Result<WorkflowCheckResult> {
        let current_branch =
            std::env::var("GITHUB_REF").unwrap_or_else(|_| "refs/heads/main".to_string());
        let branch = current_branch
            .strip_prefix("refs/heads/")
            .unwrap_or(&current_branch);

        let api_client = WorkflowApiClient::from_env()?;
        let tracker = WorkflowTracker::new(api_client, self.config, self.interner);

        let result = tracker.check_workflows(branch, current_files).await?;

        // Merge failed files into current file list
        tracker.merge_failed_files(current_files, &result.failures);

        Ok(result)
    }

    /// Process via GitHub REST API
    async fn process_via_rest_api(&self) -> Result<ProcessedResult> {
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
                let (owner, repo) = Self::extract_owner_repo_from_env()?;
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

    /// Extract owner and repo from GITHUB_REPOSITORY env var
    fn extract_owner_repo_from_env() -> Result<(String, String)> {
        let repository = std::env::var("GITHUB_REPOSITORY").map_err(|_| {
            crate::error::Error::Config("GITHUB_REPOSITORY not set".to_string())
        })?;

        let parts: Vec<&str> = repository.split('/').collect();
        if parts.len() != 2 {
            return Err(crate::error::Error::Config(format!(
                "Invalid GITHUB_REPOSITORY format: {}",
                repository
            )));
        }

        Ok((parts[0].to_string(), parts[1].to_string()))
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
}
