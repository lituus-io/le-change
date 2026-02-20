#[cfg(target_env = "musl")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use clap::Parser;
use lechange_core::output::computed::ComputedOutputs;
use lechange_core::output::json_format::{format_deploy_matrix, safe_output_escape};
use lechange_core::types::{GroupDeployAction, InputConfig};
use lechange_core::StringInterner;
use std::borrow::Cow;
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

fn run_detect(args: DetectArgs) -> i32 {
    let repo_path = args
        .repo_path
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let output_format = OutputFormat::detect(args.output_format.as_deref());
    let include_reason = args.deploy_matrix_include_reason;
    let include_concurrency = args.deploy_matrix_include_concurrency;

    // Clean env var inputs (GHA sets empty strings for unset optional inputs)
    let files = clean_vec(&args.files);
    let files_ignore = clean_vec(&args.files_ignore);
    let files_group_by = clean_opt(&args.files_group_by);
    let workflow_name_filter = clean_opt(&args.workflow_name_filter);
    let base_sha = clean_opt(&args.base_sha);
    let sha = clean_opt(&args.sha);
    let token = clean_opt(&args.token);

    // Build InputConfig — borrowing from args (zero-copy)
    let config = InputConfig {
        base_sha: base_sha.map(Cow::Borrowed),
        sha: sha.map(Cow::Borrowed),
        files: files.map(|v| v.into_iter().map(Cow::Borrowed).collect()),
        files_ignore: files_ignore.map(|v| v.into_iter().map(Cow::Borrowed).collect()),
        files_group_by: files_group_by.map(Cow::Borrowed),
        files_group_by_key: Some(Cow::Borrowed(&args.files_group_by_key)),
        files_ancestor_lookup_depth: args.files_ancestor_lookup_depth,
        track_workflow_failures: args.track_workflow_failures,
        failure_tracking_level: match args.failure_tracking_level.as_str() {
            "job" | "Job" => lechange_core::FailureTrackingLevel::Job,
            _ => lechange_core::FailureTrackingLevel::Run,
        },
        wait_for_active_workflows: args.wait_for_active_workflows,
        workflow_max_wait_seconds: args.workflow_max_wait_seconds,
        workflow_name_filter: workflow_name_filter.map(Cow::Borrowed),
        deploy_matrix_include_reason: include_reason,
        deploy_matrix_include_concurrency: include_concurrency,
        token: token.map(Cow::Borrowed),
        safe_output: true,
        json: true,
        escape_json: true,
        use_posix_path_separator: true,
        skip_initial_fetch: true,
        ..Default::default()
    };

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
        let outputs = ComputedOutputs::compute_with_concurrency(
            &processed,
            false,
            blocked_groups,
            Some(&interner),
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

    let deploy_matrix = format_deploy_matrix(
        &outputs.group_deploy_decisions,
        resolve,
        " ",
        include_reason,
        include_concurrency,
    );

    let has_changes = !all_changed.is_empty() || outputs.has_deployable_groups();
    let any_changed = !all_changed.is_empty();

    // Diagnostics
    let diagnostics_json = {
        let diags: Vec<serde_json::Value> = processed
            .diagnostics
            .iter()
            .map(|d| {
                serde_json::json!({
                    "severity": format!("{:?}", d.severity).to_lowercase(),
                    "category": format!("{:?}", d.category),
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
                let action = match d.action {
                    GroupDeployAction::Deploy => "deploy",
                    GroupDeployAction::Skip => "skip",
                };
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
                    let reason = d.reason.map(|r| match r {
                        lechange_core::types::GroupDeployReason::NewChange => "new_change",
                        lechange_core::types::GroupDeployReason::PreviousFailure => {
                            "previous_failure"
                        }
                        lechange_core::types::GroupDeployReason::BothNewAndFailed => {
                            "both_new_and_failed"
                        }
                    });
                    obj["reason"] = serde_json::json!(reason);
                }
                if include_concurrency {
                    obj["concurrency_blocked"] = serde_json::json!(d.concurrency_blocked);
                    obj["concurrency_blocked_by"] = serde_json::json!(d.concurrency_blocked_by);
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
    };

    match output_format {
        OutputFormat::Gha => write_gha_output(&out),
        OutputFormat::Json => write_json_output(&out),
        OutputFormat::Text => write_text_output(&out, &outputs, &interner, &processed),
    }

    if has_changes {
        0
    } else {
        2
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
}

/// Write outputs using GitHub Actions multiline syntax to $GITHUB_OUTPUT
fn write_gha_output(out: &DetectOutput) {
    let output_file = match std::env::var("GITHUB_OUTPUT") {
        Ok(f) => f,
        Err(_) => {
            eprintln!("Warning: GITHUB_OUTPUT not set, falling back to stdout");
            write_json_output(out);
            return;
        }
    };

    let mut f = match std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&output_file)
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Error: cannot open GITHUB_OUTPUT ({output_file}): {e}");
            return;
        }
    };

    let delim = "LECHANGE_EOF";

    let _ = writeln!(f, "matrix<<{delim}");
    let _ = writeln!(f, "{}", safe_output_escape(out.deploy_matrix));
    let _ = writeln!(f, "{delim}");
    let _ = writeln!(f, "has_changes={}", out.has_changes);

    let changed_str = out.all_changed.join(" ");
    let _ = writeln!(f, "changed_files<<{delim}");
    let _ = writeln!(f, "{}", safe_output_escape(&changed_str));
    let _ = writeln!(f, "{delim}");
    let _ = writeln!(f, "changed_files_count={}", out.all_changed.len());
    let _ = writeln!(f, "any_changed={}", out.any_changed);

    for (name, files) in [
        ("added_files", out.added),
        ("modified_files", out.modified),
        ("deleted_files", out.deleted),
        ("files_to_rebuild", out.files_to_rebuild),
        ("files_to_skip", out.files_to_skip),
    ] {
        let _ = writeln!(f, "{name}<<{delim}");
        let _ = writeln!(f, "{}", safe_output_escape(&files.join(" ")));
        let _ = writeln!(f, "{delim}");
    }

    let _ = writeln!(f, "deploy_decisions<<{delim}");
    let _ = writeln!(f, "{}", safe_output_escape(out.deploy_decisions_json));
    let _ = writeln!(f, "{delim}");
    let _ = writeln!(f, "diagnostics<<{delim}");
    let _ = writeln!(f, "{}", safe_output_escape(out.diagnostics_json));
    let _ = writeln!(f, "{delim}");

    // Summary to stdout (visible in job log)
    let stdout = std::io::stdout();
    let mut w = stdout.lock();
    let _ = writeln!(w, "Le Change Detection Results");
    let _ = writeln!(w, "===========================");
    let _ = writeln!(w, "Changed files: {}", out.all_changed.len());
    let _ = writeln!(
        w,
        "  Added: {}, Modified: {}, Deleted: {}",
        out.added.len(),
        out.modified.len(),
        out.deleted.len()
    );
    let _ = writeln!(w, "Has deployable changes: {}", out.has_changes);
    if !out.files_to_rebuild.is_empty() {
        let _ = writeln!(w, "Files to rebuild: {}", out.files_to_rebuild.len());
    }
    if !out.files_to_skip.is_empty() {
        let _ = writeln!(w, "Files to skip: {}", out.files_to_skip.len());
    }
}

/// Write full JSON output to stdout
fn write_json_output(out: &DetectOutput) {
    let matrix_val: serde_json::Value =
        serde_json::from_str(out.deploy_matrix).unwrap_or(serde_json::json!({"include":[]}));
    let decisions_val: serde_json::Value =
        serde_json::from_str(out.deploy_decisions_json).unwrap_or(serde_json::json!([]));
    let diags_val: serde_json::Value =
        serde_json::from_str(out.diagnostics_json).unwrap_or(serde_json::json!([]));

    let output = serde_json::json!({
        "matrix": matrix_val,
        "has_changes": out.has_changes,
        "changed_files": out.all_changed,
        "changed_files_count": out.all_changed.len(),
        "any_changed": out.any_changed,
        "added_files": out.added,
        "modified_files": out.modified,
        "deleted_files": out.deleted,
        "deploy_decisions": decisions_val,
        "files_to_rebuild": out.files_to_rebuild,
        "files_to_skip": out.files_to_skip,
        "diagnostics": diags_val,
    });

    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    let _ = serde_json::to_writer(&mut lock, &output);
    let _ = writeln!(lock);
}

/// Write human-readable text to stdout
fn write_text_output(
    out: &DetectOutput,
    outputs: &ComputedOutputs,
    interner: &StringInterner,
    processed: &lechange_core::ProcessedResult,
) {
    let stdout = std::io::stdout();
    let mut w = stdout.lock();

    let _ = writeln!(w, "Le Change Detection Results");
    let _ = writeln!(w, "===========================");
    let _ = writeln!(w);
    let _ = writeln!(w, "Changed files: {}", out.all_changed.len());

    if !out.added.is_empty() {
        let _ = writeln!(w, "\nAdded ({}):", out.added.len());
        for f in out.added {
            let _ = writeln!(w, "  + {f}");
        }
    }

    if !out.modified.is_empty() {
        let _ = writeln!(w, "\nModified ({}):", out.modified.len());
        for f in out.modified {
            let _ = writeln!(w, "  ~ {f}");
        }
    }

    if !out.deleted.is_empty() {
        let _ = writeln!(w, "\nDeleted ({}):", out.deleted.len());
        for f in out.deleted {
            let _ = writeln!(w, "  - {f}");
        }
    }

    if !outputs.group_deploy_decisions.is_empty() {
        let _ = writeln!(w, "\nDeploy Decisions:");
        for d in &outputs.group_deploy_decisions {
            let key = interner.resolve(d.key).unwrap_or("?");
            let action = match d.action {
                GroupDeployAction::Deploy => "DEPLOY",
                GroupDeployAction::Skip => "skip",
            };
            let _ = writeln!(w, "  [{action}] {key} ({} files)", d.total_files);
        }
    }

    if let Some(ref ci) = processed.ci_decision {
        if !ci.files_to_rebuild.is_empty() {
            let _ = writeln!(w, "\nFiles to rebuild: {}", ci.files_to_rebuild.len());
        }
        if !ci.files_to_skip.is_empty() {
            let _ = writeln!(w, "Files to skip: {}", ci.files_to_skip.len());
        }
    }

    if !processed.diagnostics.is_empty() {
        let _ = writeln!(w, "\nDiagnostics:");
        for d in &processed.diagnostics {
            let _ = writeln!(w, "  [{:?}] {}", d.severity, d.message);
        }
    }

    let _ = writeln!(w, "\nHas deployable changes: {}", out.has_changes);
}
