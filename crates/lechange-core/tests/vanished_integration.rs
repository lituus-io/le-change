//! Full-pipeline integration tests for vanished-file detection.

use std::fs;
use std::path::Path;
use std::process::Command;

use lechange_core::coordination::processor::FileProcessor;
use lechange_core::git::GitRepository;
use lechange_core::output::ComputedOutputs;
use lechange_core::types::{GroupDeployAction, GroupDeployReason};
use lechange_core::{InputConfig, StringInterner};
use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn commit_all(dir: &Path, msg: &str) -> String {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", msg]);
    git(dir, &["rev-parse", "HEAD"])
}

/// base(README) -> add stacks/gone (Pulumi.yaml + schema.json) -> remove it
fn vanished_fixture() -> (TempDir, String, String, String) {
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    fs::write(dir.path().join("README.md"), "r").unwrap();
    let base = commit_all(dir.path(), "base");

    let stack = dir.path().join("stacks/gone");
    fs::create_dir_all(&stack).unwrap();
    fs::write(stack.join("Pulumi.yaml"), "name: gone").unwrap();
    fs::write(stack.join("schema.json"), "[]").unwrap();
    let add_sha = commit_all(dir.path(), "add stack");

    git(dir.path(), &["rm", "-rq", "stacks/gone"]);
    let head = commit_all(dir.path(), "remove stack");
    (dir, base, add_sha, head)
}

fn run_pipeline(
    dir: &Path,
    interner: &StringInterner,
    config: &InputConfig<'_>,
) -> lechange_core::types::ProcessedResult {
    let repo = GitRepository::discover(dir).unwrap();
    let processor = FileProcessor::new(&repo, interner, config);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(processor.process()).unwrap()
}

#[test]
fn test_full_pipeline_vanished_destroy_decision() {
    let (dir, base, add_sha, head) = vanished_fixture();
    let interner = StringInterner::new();

    let config = InputConfig::default()
        .with_base_sha(Some(&base))
        .with_sha(Some(&head))
        .with_files_group_by(Some("stacks/{group}/**"))
        .with_detect_vanished(true);
    let result = run_pipeline(dir.path(), &interner, &config);

    // Both stack files vanished; group synthesized despite the dir being gone
    assert_eq!(result.vanished_files.len(), 2);
    let outputs = ComputedOutputs::compute_full(&result, false, None, Some(&interner), false);
    let destroys: Vec<_> = outputs
        .group_deploy_decisions
        .iter()
        .filter(|d| d.action == GroupDeployAction::Destroy)
        .collect();
    assert_eq!(destroys.len(), 1);
    let d = destroys[0];
    assert_eq!(interner.resolve(d.key), Some("gone"));
    assert_eq!(d.reason, Some(GroupDeployReason::Vanished));
    assert_eq!(
        interner.resolve(d.reconstruct_sha.unwrap()),
        Some(add_sha.as_str())
    );
    assert!(outputs.has_destroyable_groups());

    // Matrix carries a routable destroy entry
    let matrix = lechange_core::output::json_format::format_deploy_matrix(
        &outputs.group_deploy_decisions,
        |s| interner.resolve(s),
        " ",
        false,
        false,
    );
    let parsed: serde_json::Value = serde_json::from_str(&matrix).unwrap();
    let entry = &parsed["include"][0];
    assert_eq!(entry["action"], "destroy");
    assert_eq!(entry["last_seen_sha"], add_sha.as_str());
}

#[test]
fn test_deleted_to_destroy_endpoint_parity() {
    // Stack exists at base and is deleted at head (plain endpoint deletion)
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    let stack = dir.path().join("stacks/old");
    fs::create_dir_all(&stack).unwrap();
    fs::write(stack.join("Pulumi.yaml"), "name: old").unwrap();
    let base = commit_all(dir.path(), "base");
    git(dir.path(), &["rm", "-rq", "stacks/old"]);
    let head = commit_all(dir.path(), "rm");

    let interner = StringInterner::new();
    let config = InputConfig::default()
        .with_base_sha(Some(&base))
        .with_sha(Some(&head))
        .with_files_group_by(Some("stacks/{group}/**"))
        .with_detect_vanished(true)
        .with_deleted_to_destroy(true);
    let result = run_pipeline(dir.path(), &interner, &config);

    assert!(
        result.vanished_files.is_empty(),
        "endpoint deletion is not vanished"
    );
    let outputs = ComputedOutputs::compute_full(&result, false, None, Some(&interner), true);
    let d = outputs
        .group_deploy_decisions
        .iter()
        .find(|d| d.action == GroupDeployAction::Destroy)
        .expect("endpoint-deleted group must destroy with the flag");
    assert_eq!(d.reason, Some(GroupDeployReason::EndpointDeleted));
    assert_eq!(
        interner.resolve(d.reconstruct_sha.unwrap()),
        Some(base.as_str()),
        "reconstruction source for endpoint deletions is the base SHA"
    );
}

#[test]
fn test_mixed_group_stays_deploy() {
    // Stack modified at head + a sibling file vanished within the range
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    let stack = dir.path().join("stacks/live");
    fs::create_dir_all(&stack).unwrap();
    fs::write(stack.join("Pulumi.yaml"), "v1").unwrap();
    let base = commit_all(dir.path(), "base");
    fs::write(stack.join("extra.yaml"), "tmp").unwrap();
    commit_all(dir.path(), "add extra");
    git(dir.path(), &["rm", "-q", "stacks/live/extra.yaml"]);
    commit_all(dir.path(), "rm extra");
    fs::write(stack.join("Pulumi.yaml"), "v2").unwrap();
    let head = commit_all(dir.path(), "modify");

    let interner = StringInterner::new();
    let config = InputConfig::default()
        .with_base_sha(Some(&base))
        .with_sha(Some(&head))
        .with_files_group_by(Some("stacks/{group}/**"))
        .with_detect_vanished(true);
    let result = run_pipeline(dir.path(), &interner, &config);

    assert_eq!(result.vanished_files.len(), 1);
    let outputs = ComputedOutputs::compute_full(&result, false, None, Some(&interner), true);
    let d = &outputs.group_deploy_decisions[0];
    assert_eq!(d.action, GroupDeployAction::Deploy, "live changes win");
    assert_eq!(d.vanished_files.len(), 1, "vanished info still rides along");
    assert!(d.reconstruct_sha.is_none());
}

#[test]
fn test_default_off_no_behavior_change() {
    let (dir, base, _add_sha, head) = vanished_fixture();
    let interner = StringInterner::new();

    let config = InputConfig::default()
        .with_base_sha(Some(&base))
        .with_sha(Some(&head))
        .with_files_group_by(Some("stacks/{group}/**"));
    let result = run_pipeline(dir.path(), &interner, &config);

    assert!(result.vanished_files.is_empty());
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.category == lechange_core::types::DiagnosticCategory::VanishedDetection),
        "default-off must add no diagnostics"
    );
    let outputs = ComputedOutputs::compute_full(&result, false, None, Some(&interner), false);
    assert!(!outputs.has_destroyable_groups());
    assert!(outputs.group_deploy_decisions.is_empty());
}

#[test]
fn test_missing_base_fails_at_sha_resolution() {
    // A base SHA absent from the repository (force push, insufficient clone
    // depth) fails during SHA resolution — BEFORE vanished detection runs.
    // This is pre-existing pipeline behavior. The walker's own soft-fail path
    // covers errors after resolution succeeds (unit-tested in git/vanished.rs);
    // this test pins the boundary so it stays explicit.
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    fs::write(dir.path().join("README.md"), "r").unwrap();
    let head = commit_all(dir.path(), "base");

    let interner = StringInterner::new();
    let fake_base = "1111111111111111111111111111111111111111";
    let config = InputConfig::default()
        .with_base_sha(Some(fake_base))
        .with_sha(Some(&head))
        .with_files_group_by(Some("stacks/{group}/**"))
        .with_detect_vanished(true);

    let repo = GitRepository::discover(dir.path()).unwrap();
    let processor = FileProcessor::new(&repo, &interner, &config);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let err = rt.block_on(processor.process()).unwrap_err();
    assert!(err.to_string().contains("not found"));
}
