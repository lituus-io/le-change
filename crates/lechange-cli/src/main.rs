#[cfg(target_env = "musl")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use clap::Parser;
use lechange_core::output::computed::ComputedOutputs;
use lechange_core::output::json_format::{format_deploy_matrix, safe_output_escape};
use lechange_core::types::{GroupDeployAction, InputConfig};
use lechange_core::StringInterner;
use std::io::Write;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "lechange", version, about = "Ultraperformant change detection")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Detect changes and output deploy matrix
    Detect(DetectArgs),
}

#[derive(clap::Args)]
struct DetectArgs {
    /// Glob patterns to include (comma-separated)
    #[arg(long, env = "LECHANGE_FILES", value_delimiter = ',')]
    files: Option<Vec<String>>,

    /// Glob patterns to exclude (comma-separated)
    #[arg(long, env = "LECHANGE_FILES_IGNORE", value_delimiter = ',')]
    files_ignore: Option<Vec<String>>,

    /// Template for group discovery (e.g. stacks/{group}/**)
    #[arg(long, env = "LECHANGE_FILES_GROUP_BY")]
    files_group_by: Option<String>,

    /// Group key mode: name, path, or hash
    #[arg(long, env = "LECHANGE_FILES_GROUP_BY_KEY", default_value = "name")]
    files_group_by_key: String,

    /// Ancestor directory lookup depth (0=disabled, max=3)
    #[arg(
        long,
        env = "LECHANGE_FILES_ANCESTOR_LOOKUP_DEPTH",
        default_value_t = 0
    )]
    files_ancestor_lookup_depth: u32,

    /// Enable workflow failure tracking
    #[arg(long, env = "LECHANGE_TRACK_WORKFLOW_FAILURES")]
    track_workflow_failures: bool,

    /// Tracking granularity: run or job
    #[arg(long, env = "LECHANGE_FAILURE_TRACKING_LEVEL", default_value = "run")]
    failure_tracking_level: String,

    /// Wait for concurrent overlapping workflows to complete
    #[arg(long, env = "LECHANGE_WAIT_FOR_ACTIVE_WORKFLOWS")]
    wait_for_active_workflows: bool,

    /// Max seconds to wait for active workflows
    #[arg(
        long,
        env = "LECHANGE_WORKFLOW_MAX_WAIT_SECONDS",
        default_value_t = 300
    )]
    workflow_max_wait_seconds: u32,

    /// Glob pattern to filter workflow names
    #[arg(long, env = "LECHANGE_WORKFLOW_NAME_FILTER")]
    workflow_name_filter: Option<String>,

    /// Include action/reason fields in deploy matrix
    #[arg(long, env = "LECHANGE_DEPLOY_MATRIX_INCLUDE_REASON")]
    deploy_matrix_include_reason: bool,

    /// Include concurrency_blocked fields in deploy matrix
    #[arg(long, env = "LECHANGE_DEPLOY_MATRIX_INCLUDE_CONCURRENCY")]
    deploy_matrix_include_concurrency: bool,

    /// Detect files added then removed within the PR history (base..head
    /// first-parent walk); requires sufficient clone depth (fetch-depth: 0)
    #[arg(long, env = "LECHANGE_DETECT_VANISHED")]
    detect_vanished: bool,

    /// Max commits the vanished-detection walk visits (0 = unlimited)
    #[arg(long, env = "LECHANGE_VANISHED_MAX_COMMITS", default_value_t = 500)]
    vanished_max_commits: u32,

    /// Emit Destroy deploy-matrix entries for groups whose files were all
    /// deleted at the endpoint diff (reconstruct_sha = base SHA)
    #[arg(long, env = "LECHANGE_DELETED_TO_DESTROY")]
    deleted_to_destroy: bool,

    /// GitHub token for API access
    #[arg(long, env = "GITHUB_TOKEN")]
    token: Option<String>,

    /// Override base commit SHA
    #[arg(long, env = "LECHANGE_BASE_SHA")]
    base_sha: Option<String>,

    /// Override head commit SHA
    #[arg(long, env = "LECHANGE_SHA")]
    sha: Option<String>,

    /// Output format: gha, json, text (default: auto-detect)
    #[arg(long, env = "LECHANGE_OUTPUT_FORMAT")]
    output_format: Option<String>,

    /// Repository path (default: current directory)
    #[arg(long, env = "LECHANGE_REPO_PATH")]
    repo_path: Option<String>,
}

/// Output format for the CLI
enum OutputFormat {
    /// GitHub Actions: write to $GITHUB_OUTPUT + summary to stdout
    Gha,
    /// Full JSON to stdout
    Json,
    /// Human-readable text to stdout
    Text,
}

impl OutputFormat {
    fn detect(explicit: Option<&str>) -> Self {
        match explicit {
            Some("gha") => OutputFormat::Gha,
            Some("json") => OutputFormat::Json,
            Some("text") => OutputFormat::Text,
            _ => {
                if std::env::var("GITHUB_ACTIONS").is_ok() {
                    OutputFormat::Gha
                } else {
                    OutputFormat::Text
                }
            }
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let code = match cli.command {
        Commands::Detect(args) => run_detect(args),
    };
    std::process::exit(code);
}

/// Filter empty strings from Vec (env vars may produce [""] for empty values)
fn clean_vec(v: &Option<Vec<String>>) -> Option<Vec<&str>> {
    v.as_ref().and_then(|v| {
        let cleaned: Vec<&str> = v
            .iter()
            .map(|s| s.as_str())
            .filter(|s| !s.is_empty())
            .collect();
        if cleaned.is_empty() {
            None
        } else {
            Some(cleaned)
        }
    })
}

/// Filter empty string from Option (env vars may produce "" for empty values)
fn clean_opt(v: &Option<String>) -> Option<&str> {
    v.as_deref().filter(|s| !s.is_empty())
}

/// Pure arg-to-config mapping: cleaned env inputs threaded onto the core
/// builder. Separated from run_detect so it is unit-testable.
fn build_config(args: &DetectArgs) -> InputConfig<'_> {
    InputConfig::github_actions_defaults()
        .with_base_sha(clean_opt(&args.base_sha))
        .with_sha(clean_opt(&args.sha))
        .with_files(clean_vec(&args.files))
        .with_files_ignore(clean_vec(&args.files_ignore))
        .with_files_group_by(clean_opt(&args.files_group_by))
        .with_files_group_by_key(&args.files_group_by_key)
        .with_files_ancestor_lookup_depth(args.files_ancestor_lookup_depth)
        .with_track_workflow_failures(args.track_workflow_failures)
        .with_failure_tracking_level_str(&args.failure_tracking_level)
        .with_wait_for_active_workflows(args.wait_for_active_workflows)
        .with_workflow_max_wait_seconds(args.workflow_max_wait_seconds)
        .with_workflow_name_filter(clean_opt(&args.workflow_name_filter))
        .with_deploy_matrix_include_reason(args.deploy_matrix_include_reason)
        .with_deploy_matrix_include_concurrency(args.deploy_matrix_include_concurrency)
        .with_token(clean_opt(&args.token))
        .with_detect_vanished(args.detect_vanished)
        .with_vanished_max_commits(args.vanished_max_commits)
        .with_deleted_to_destroy(args.deleted_to_destroy)
}

fn run_detect(args: DetectArgs) -> i32 {
    let repo_path = args
        .repo_path
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let output_format = OutputFormat::detect(args.output_format.as_deref());
    let include_reason = args.deploy_matrix_include_reason;
    let include_concurrency = args.deploy_matrix_include_concurrency;

    let config = build_config(&args);

    // Run detection
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build();
    let rt = match rt {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("Error: failed to create runtime: {e}");
            return 1;
        }
    };

    let result = rt.block_on(async {
        let interner = StringInterner::with_capacity(2048);
        let repo = lechange_core::git::GitRepository::discover(&repo_path)?;

        let processor =
            lechange_core::coordination::processor::FileProcessor::new(&repo, &interner, &config);

        let processed = processor.process().await?;

        let blocked_groups = processed
            .workflow_result
            .as_ref()
            .map(|wr| &wr.blocked_groups);
        let outputs = ComputedOutputs::compute_full(
            &processed,
            false,
            blocked_groups,
            Some(&interner),
            args.deleted_to_destroy,
        );

        Ok::<_, lechange_core::Error>((processed, outputs, interner))
    });

    let (processed, outputs, interner) = match result {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };

    // Resolve helper
    let resolve = |s: lechange_core::InternedString| -> Option<&str> { interner.resolve(s) };

    // Build output data
    let all_changed: Vec<&str> = processed
        .filtered_indices
        .iter()
        .filter_map(|&i| {
            let file = &processed.all_files[i as usize];
            interner.resolve(file.path)
        })
        .collect();

    let added: Vec<&str> = outputs
        .filtered_added
        .iter()
        .filter_map(|&i| interner.resolve(processed.all_files[i as usize].path))
        .collect();

    let modified: Vec<&str> = outputs
        .filtered_modified
        .iter()
        .filter_map(|&i| interner.resolve(processed.all_files[i as usize].path))
        .collect();

    let deleted: Vec<&str> = outputs
        .filtered_deleted
        .iter()
        .filter_map(|&i| interner.resolve(processed.all_files[i as usize].path))
        .collect();

    let files_to_rebuild: Vec<&str> = processed
        .ci_decision
        .as_ref()
        .map(|ci| {
            ci.files_to_rebuild
                .iter()
                .filter_map(|&s| interner.resolve(s))
                .collect()
        })
        .unwrap_or_default();

    let files_to_skip: Vec<&str> = processed
        .ci_decision
        .as_ref()
        .map(|ci| {
            ci.files_to_skip
                .iter()
                .filter_map(|&s| interner.resolve(s))
                .collect()
        })
        .unwrap_or_default();

    let vanished: Vec<&str> = processed
        .vanished_files
        .iter()
        .filter_map(|v| interner.resolve(v.path))
        .collect();
    let vanished_json = lechange_core::output::json_format::format_vanished_json(
        &processed.vanished_files,
        resolve,
    );

    let deploy_matrix = format_deploy_matrix(
        &outputs.group_deploy_decisions,
        resolve,
        " ",
        include_reason,
        include_concurrency,
    );

    let has_changes = !all_changed.is_empty()
        || outputs.has_deployable_groups()
        || outputs.has_destroyable_groups();
    let any_changed = !all_changed.is_empty();

    // Diagnostics
    let diagnostics_json = {
        let diags: Vec<serde_json::Value> = processed
            .diagnostics
            .iter()
            .map(|d| {
                serde_json::json!({
                    "severity": d.severity.as_str(),
                    "category": d.category.as_str(),
                    "message": d.message,
                })
            })
            .collect();
        serde_json::to_string(&diags).unwrap_or_else(|_| "[]".to_string())
    };

    // Deploy decisions JSON
    let deploy_decisions_json = {
        let decisions: Vec<serde_json::Value> = outputs
            .group_deploy_decisions
            .iter()
            .map(|d| {
                let action = d.action.as_str();
                let files: Vec<&str> = d
                    .files_to_rebuild
                    .iter()
                    .filter_map(|&s| interner.resolve(s))
                    .collect();
                let mut obj = serde_json::json!({
                    "key": interner.resolve(d.key).unwrap_or(""),
                    "action": action,
                    "files": files,
                    "count": files.len(),
                });
                if include_reason {
                    obj["reason"] = serde_json::json!(d.reason.map(|r| r.as_str()));
                }
                if include_concurrency {
                    obj["concurrency_blocked"] = serde_json::json!(d.concurrency_blocked);
                    obj["concurrency_blocked_by"] = serde_json::json!(d.concurrency_blocked_by);
                }
                if !d.vanished_files.is_empty() {
                    let vanished: Vec<&str> = d
                        .vanished_files
                        .iter()
                        .filter_map(|v| interner.resolve(v.path))
                        .collect();
                    obj["vanished"] = serde_json::json!(vanished);
                }
                if let Some(sha) = d.reconstruct_sha.and_then(|s| interner.resolve(s)) {
                    obj["last_seen_sha"] = serde_json::json!(sha);
                }
                obj
            })
            .collect();
        serde_json::to_string(&decisions).unwrap_or_else(|_| "[]".to_string())
    };

    let out = DetectOutput {
        deploy_matrix: &deploy_matrix,
        has_changes,
        all_changed: &all_changed,
        added: &added,
        modified: &modified,
        deleted: &deleted,
        any_changed,
        deploy_decisions_json: &deploy_decisions_json,
        files_to_rebuild: &files_to_rebuild,
        files_to_skip: &files_to_skip,
        diagnostics_json: &diagnostics_json,
        vanished: &vanished,
        vanished_json: &vanished_json,
    };

    let write_result = match output_format {
        OutputFormat::Gha => write_gha_output(&out),
        OutputFormat::Json => write_json_output(&out),
        OutputFormat::Text => write_text_output(&out, &outputs, &interner, &processed),
    };
    if let Err(e) = write_result {
        eprintln!("Error: failed to write output: {e}");
        return 1;
    }

    // GHA: always exit 0 — users check has_changes output
    // Non-GHA: exit 2 signals "no changes" for scripting
    match output_format {
        OutputFormat::Gha => 0,
        _ if has_changes => 0,
        _ => 2,
    }
}

/// Bundled output data passed to format writers.
struct DetectOutput<'a> {
    deploy_matrix: &'a str,
    has_changes: bool,
    all_changed: &'a [&'a str],
    added: &'a [&'a str],
    modified: &'a [&'a str],
    deleted: &'a [&'a str],
    any_changed: bool,
    deploy_decisions_json: &'a str,
    files_to_rebuild: &'a [&'a str],
    files_to_skip: &'a [&'a str],
    diagnostics_json: &'a str,
    vanished: &'a [&'a str],
    vanished_json: &'a str,
}

/// Write outputs using GitHub Actions multiline syntax to $GITHUB_OUTPUT
fn write_gha_output(out: &DetectOutput) -> std::io::Result<()> {
    let output_file = match std::env::var("GITHUB_OUTPUT") {
        Ok(f) => f,
        Err(_) => {
            eprintln!("Warning: GITHUB_OUTPUT not set, falling back to stdout");
            return write_json_output(out);
        }
    };

    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&output_file)?;

    let delim = format!(
        "LECHANGE_EOF_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );

    writeln!(f, "matrix<<{delim}")?;
    writeln!(f, "{}", safe_output_escape(out.deploy_matrix))?;
    writeln!(f, "{delim}")?;
    writeln!(f, "has_changes={}", out.has_changes)?;

    let changed_str = out.all_changed.join(" ");
    writeln!(f, "changed_files<<{delim}")?;
    writeln!(f, "{}", safe_output_escape(&changed_str))?;
    writeln!(f, "{delim}")?;
    writeln!(f, "changed_files_count={}", out.all_changed.len())?;
    writeln!(f, "any_changed={}", out.any_changed)?;

    for (name, files) in [
        ("added_files", out.added),
        ("modified_files", out.modified),
        ("deleted_files", out.deleted),
        ("vanished_files", out.vanished),
        ("files_to_rebuild", out.files_to_rebuild),
        ("files_to_skip", out.files_to_skip),
    ] {
        writeln!(f, "{name}<<{delim}")?;
        writeln!(f, "{}", safe_output_escape(&files.join(" ")))?;
        writeln!(f, "{delim}")?;
    }

    writeln!(f, "deploy_decisions<<{delim}")?;
    writeln!(f, "{}", safe_output_escape(out.deploy_decisions_json))?;
    writeln!(f, "{delim}")?;
    writeln!(f, "diagnostics<<{delim}")?;
    writeln!(f, "{}", safe_output_escape(out.diagnostics_json))?;
    writeln!(f, "{delim}")?;
    writeln!(f, "vanished<<{delim}")?;
    writeln!(f, "{}", safe_output_escape(out.vanished_json))?;
    writeln!(f, "{delim}")?;

    // Summary to stdout (visible in job log)
    let stdout = std::io::stdout();
    let mut w = stdout.lock();
    writeln!(w, "Le Change Detection Results")?;
    writeln!(w, "===========================")?;
    writeln!(w, "Changed files: {}", out.all_changed.len())?;
    writeln!(
        w,
        "  Added: {}, Modified: {}, Deleted: {}",
        out.added.len(),
        out.modified.len(),
        out.deleted.len()
    )?;
    if !out.vanished.is_empty() {
        writeln!(
            w,
            "Vanished (added then removed in range): {}",
            out.vanished.len()
        )?;
    }
    writeln!(w, "Has deployable changes: {}", out.has_changes)?;
    if !out.files_to_rebuild.is_empty() {
        writeln!(w, "Files to rebuild: {}", out.files_to_rebuild.len())?;
    }
    if !out.files_to_skip.is_empty() {
        writeln!(w, "Files to skip: {}", out.files_to_skip.len())?;
    }
    Ok(())
}

/// Write full JSON output to stdout
fn write_json_output(out: &DetectOutput) -> std::io::Result<()> {
    let matrix_val: serde_json::Value =
        serde_json::from_str(out.deploy_matrix).unwrap_or(serde_json::json!({"include":[]}));
    let decisions_val: serde_json::Value =
        serde_json::from_str(out.deploy_decisions_json).unwrap_or(serde_json::json!([]));
    let diags_val: serde_json::Value =
        serde_json::from_str(out.diagnostics_json).unwrap_or(serde_json::json!([]));
    let vanished_val: serde_json::Value =
        serde_json::from_str(out.vanished_json).unwrap_or(serde_json::json!([]));

    let output = serde_json::json!({
        "matrix": matrix_val,
        "has_changes": out.has_changes,
        "changed_files": out.all_changed,
        "changed_files_count": out.all_changed.len(),
        "any_changed": out.any_changed,
        "added_files": out.added,
        "modified_files": out.modified,
        "deleted_files": out.deleted,
        "vanished_files": out.vanished,
        "vanished": vanished_val,
        "deploy_decisions": decisions_val,
        "files_to_rebuild": out.files_to_rebuild,
        "files_to_skip": out.files_to_skip,
        "diagnostics": diags_val,
    });

    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    serde_json::to_writer(&mut lock, &output).map_err(std::io::Error::other)?;
    writeln!(lock)?;
    Ok(())
}

/// Write human-readable text to stdout
fn write_text_output(
    out: &DetectOutput,
    outputs: &ComputedOutputs,
    interner: &StringInterner,
    processed: &lechange_core::ProcessedResult,
) -> std::io::Result<()> {
    let stdout = std::io::stdout();
    let mut w = stdout.lock();

    writeln!(w, "Le Change Detection Results")?;
    writeln!(w, "===========================")?;
    writeln!(w)?;
    writeln!(w, "Changed files: {}", out.all_changed.len())?;

    if !out.added.is_empty() {
        writeln!(w, "\nAdded ({}):", out.added.len())?;
        for f in out.added {
            writeln!(w, "  + {f}")?;
        }
    }

    if !out.modified.is_empty() {
        writeln!(w, "\nModified ({}):", out.modified.len())?;
        for f in out.modified {
            writeln!(w, "  ~ {f}")?;
        }
    }

    if !out.deleted.is_empty() {
        writeln!(w, "\nDeleted ({}):", out.deleted.len())?;
        for f in out.deleted {
            writeln!(w, "  - {f}")?;
        }
    }

    if !outputs.group_deploy_decisions.is_empty() {
        writeln!(w, "\nDeploy Decisions:")?;
        for d in &outputs.group_deploy_decisions {
            let key = interner.resolve(d.key).unwrap_or("?");
            let action = match d.action {
                GroupDeployAction::Deploy => "DEPLOY",
                GroupDeployAction::Skip => "skip",
                GroupDeployAction::Destroy => "DESTROY",
            };
            writeln!(w, "  [{action}] {key} ({} files)", d.total_files)?;
        }
    }

    if let Some(ref ci) = processed.ci_decision {
        if !ci.files_to_rebuild.is_empty() {
            writeln!(w, "\nFiles to rebuild: {}", ci.files_to_rebuild.len())?;
        }
        if !ci.files_to_skip.is_empty() {
            writeln!(w, "Files to skip: {}", ci.files_to_skip.len())?;
        }
    }

    if !processed.diagnostics.is_empty() {
        writeln!(w, "\nDiagnostics:")?;
        for d in &processed.diagnostics {
            writeln!(w, "  [{:?}] {}", d.severity, d.message)?;
        }
    }

    writeln!(w, "\nHas deployable changes: {}", out.has_changes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_args() -> DetectArgs {
        DetectArgs {
            files: None,
            files_ignore: None,
            files_group_by: None,
            files_group_by_key: "name".to_string(),
            files_ancestor_lookup_depth: 0,
            track_workflow_failures: false,
            failure_tracking_level: "run".to_string(),
            wait_for_active_workflows: false,
            workflow_max_wait_seconds: 300,
            workflow_name_filter: None,
            deploy_matrix_include_reason: false,
            deploy_matrix_include_concurrency: false,
            token: None,
            base_sha: None,
            sha: None,
            output_format: None,
            repo_path: None,
            detect_vanished: false,
            vanished_max_commits: 500,
            deleted_to_destroy: false,
        }
    }

    #[test]
    fn test_clean_vec_filters_empty_env_strings() {
        // GHA sets "" for unset optional inputs; value_delimiter yields [""]
        assert_eq!(clean_vec(&Some(vec!["".to_string()])), None);
        assert_eq!(clean_vec(&None), None);
        assert_eq!(
            clean_vec(&Some(vec![
                "a".to_string(),
                "".to_string(),
                "b".to_string()
            ])),
            Some(vec!["a", "b"])
        );
    }

    #[test]
    fn test_clean_opt_filters_empty_env_strings() {
        assert_eq!(clean_opt(&Some("".to_string())), None);
        assert_eq!(clean_opt(&None), None);
        assert_eq!(clean_opt(&Some("x".to_string())), Some("x"));
    }

    #[test]
    fn test_output_format_detection() {
        assert!(matches!(
            OutputFormat::detect(Some("json")),
            OutputFormat::Json
        ));
        assert!(matches!(
            OutputFormat::detect(Some("gha")),
            OutputFormat::Gha
        ));
        assert!(matches!(
            OutputFormat::detect(Some("text")),
            OutputFormat::Text
        ));
        // Explicit beats environment
        assert!(matches!(
            OutputFormat::detect(Some("text")),
            OutputFormat::Text
        ));
    }

    #[test]
    fn test_build_config_applies_gha_defaults() {
        let args = base_args();
        let config = build_config(&args);
        assert!(config.safe_output);
        assert!(config.json);
        assert!(config.escape_json);
        assert!(config.use_posix_path_separator);
        assert!(config.skip_initial_fetch);
        assert!(config.base_sha.is_none());
        assert!(config.files.is_none());
    }

    #[test]
    fn test_build_config_maps_args() {
        let mut args = base_args();
        args.base_sha = Some("abc123".to_string());
        args.sha = Some("".to_string()); // empty env input must clean to None
        args.files = Some(vec!["stacks/**/Pulumi.yaml".to_string()]);
        args.files_group_by = Some("stacks/{group}/**".to_string());
        args.files_group_by_key = "path".to_string();
        args.failure_tracking_level = "job".to_string();
        args.deploy_matrix_include_reason = true;

        let config = build_config(&args);
        assert_eq!(config.base_sha.as_deref(), Some("abc123"));
        assert!(config.sha.is_none());
        assert_eq!(config.files.as_ref().map(|f| f.len()), Some(1));
        assert_eq!(config.files_group_by.as_deref(), Some("stacks/{group}/**"));
        assert_eq!(config.files_group_by_key.as_deref(), Some("path"));
        assert_eq!(
            config.failure_tracking_level,
            lechange_core::FailureTrackingLevel::Job
        );
        assert!(config.deploy_matrix_include_reason);
    }
}
