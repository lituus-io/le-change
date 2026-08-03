//! Regression tests for a divergent / stale base SHA.
//!
//! `github.event.pull_request.base.sha` is the base BRANCH TIP at event time,
//! not the PR's fork point. When the base branch advances after a PR branch is
//! cut, a two-dot endpoint diff reports every path added to the base branch as
//! Deleted for that PR — which downstream consumers turn into `destroy`.
//!
//! Incident this pins: a PR touching two stacks emitted destroy entries for four
//! unrelated stacks that had landed on the base branch minutes after the branch
//! was cut, and real infrastructure was torn down. The processor now normalizes
//! a divergent base to the merge base (three-dot semantics).

use std::fs;
use std::path::Path;
use std::process::Command;

use lechange_core::coordination::processor::FileProcessor;
use lechange_core::git::GitRepository;
use lechange_core::types::{ChangeType, DiagnosticCategory};
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

fn write_stack(dir: &Path, name: &str) {
    let stack = dir.join(format!("stacks/{name}"));
    fs::create_dir_all(&stack).unwrap();
    fs::write(stack.join("Pulumi.yaml"), format!("name: {name}")).unwrap();
}

/// fork(existing) -> PR modifies existing; base branch separately gains 4 stacks.
/// Returns (dir, fork_sha, base_tip_sha, head_sha).
fn divergent_fixture() -> (TempDir, String, String, String) {
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    write_stack(dir.path(), "existing");
    let fork = commit_all(dir.path(), "base: existing");

    // PR branch, cut at `fork`, touches only its own stack.
    git(dir.path(), &["checkout", "-qb", "pr"]);
    fs::write(
        dir.path().join("stacks/existing/Pulumi.yaml"),
        "name: existing # touched",
    )
    .unwrap();
    let head = commit_all(dir.path(), "pr: modify existing");

    // Base branch advances AFTER the branch point with unrelated stacks.
    git(dir.path(), &["checkout", "-q", "main"]);
    for n in 1..=4 {
        write_stack(dir.path(), &format!("other{n}"));
    }
    let base_tip = commit_all(dir.path(), "main: add 4 unrelated stacks");
    git(dir.path(), &["checkout", "-q", "pr"]);

    (dir, fork, base_tip, head)
}

fn run(dir: &Path, interner: &StringInterner, config: &InputConfig<'_>) -> lechange_core::types::ProcessedResult {
    let repo = GitRepository::discover(dir).unwrap();
    let processor = FileProcessor::new(&repo, interner, config);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(processor.process()).unwrap()
}

#[test]
fn test_divergent_base_reports_no_phantom_deletions() {
    let (dir, _fork, base_tip, head) = divergent_fixture();
    let interner = StringInterner::new();

    // Caller passes the base BRANCH TIP, exactly as the GitHub event provides it.
    let config = InputConfig::default()
        .with_base_sha(Some(&base_tip))
        .with_sha(Some(&head))
        .with_detect_vanished(true);
    let result = run(dir.path(), &interner, &config);

    let deleted: Vec<_> = result
        .all_files
        .iter()
        .filter(|f| f.change_type == ChangeType::Deleted)
        .collect();
    assert!(
        deleted.is_empty(),
        "stacks added to the base branch after the fork point must never be \
         reported as deleted by this PR; got {} deletion(s)",
        deleted.len()
    );

    // The PR's own change is still detected.
    assert_eq!(result.all_files.len(), 1);
    assert_eq!(result.all_files[0].change_type, ChangeType::Modified);
}

#[test]
fn test_divergent_base_emits_diagnostic() {
    let (dir, fork, base_tip, head) = divergent_fixture();
    let interner = StringInterner::new();

    let config = InputConfig::default()
        .with_base_sha(Some(&base_tip))
        .with_sha(Some(&head))
        .with_detect_vanished(true);
    let result = run(dir.path(), &interner, &config);

    let diag = result
        .diagnostics
        .iter()
        .find(|d| d.category == DiagnosticCategory::DivergentBase)
        .expect("divergent base must surface a diagnostic, not be silent");
    assert!(diag.message.contains(&fork), "diagnostic names the merge base");

    // The normalized base is what downstream consumers see (last_seen_sha).
    assert_eq!(interner.resolve(result.base_sha.unwrap()), Some(fork.as_str()));
}

#[test]
fn test_ancestor_base_is_exact_noop() {
    let (dir, fork, _base_tip, head) = divergent_fixture();
    let interner = StringInterner::new();

    // base IS an ancestor: merge_base(base, head) == base, so nothing changes.
    let config = InputConfig::default()
        .with_base_sha(Some(&fork))
        .with_sha(Some(&head))
        .with_detect_vanished(true);
    let result = run(dir.path(), &interner, &config);

    assert_eq!(result.all_files.len(), 1);
    assert_eq!(result.all_files[0].change_type, ChangeType::Modified);
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.category == DiagnosticCategory::DivergentBase),
        "no diagnostic on the common path"
    );
    assert_eq!(interner.resolve(result.base_sha.unwrap()), Some(fork.as_str()));
}

#[test]
fn test_genuine_deletion_still_detected() {
    // The fix must not suppress real deletions: a PR that deletes a stack still
    // reports it, so legitimate teardown keeps working.
    let (dir, _fork, base_tip, _head) = divergent_fixture();
    let interner = StringInterner::new();

    git(dir.path(), &["checkout", "-q", "-b", "pr-del", "main"]);
    git(dir.path(), &["rm", "-rq", "stacks/other1"]);
    let del_head = commit_all(dir.path(), "pr: delete other1");

    let config = InputConfig::default()
        .with_base_sha(Some(&base_tip))
        .with_sha(Some(&del_head))
        .with_detect_vanished(true);
    let result = run(dir.path(), &interner, &config);

    let deleted: Vec<_> = result
        .all_files
        .iter()
        .filter(|f| f.change_type == ChangeType::Deleted)
        .collect();
    assert_eq!(deleted.len(), 1, "a real deletion must still be reported");
}
