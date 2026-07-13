//! Vanished-file detection: first-parent history walk over base..head.
//!
//! The endpoint diff (`diff_tree_to_tree(base, head)`) cannot see a file that
//! was added in some commit of a PR and removed again before head — it exists
//! in neither endpoint tree. This walker finds those files and records the
//! last commit where each existed, so consumers can reconstruct content
//! (e.g. to destroy infrastructure the file defined).
//!
//! Scoping guarantee: the revwalk pushes head, hides base, and simplifies to
//! first parents — only commits introduced by this base..head range are ever
//! visited, never another branch's work. Side-branch-internal churn (a file
//! added and deleted entirely within a merged side branch) is invisible by
//! design: it never existed on the mainline.
//!
//! Determinism guarantee: detection is purely path-based. Renames are NOT
//! detected — `git mv a b` is a delete of `a` plus an add of `b`, and each path
//! is judged on its own exact repo-relative string. There is no content-
//! similarity heuristic, so the result depends only on which paths were added
//! and removed, never on how alike two files happen to be. This is essential
//! for path-keyed deployment: a stack whose file is gone at head must have its
//! path's state destroyed even if a lookalike stack was added elsewhere in the
//! same range.
//!
//! Concurrency: the walk is single-threaded by necessity — `git2::Repository`,
//! `Tree`, and `Diff` are `!Send`, and after pathspec filtering the per-commit
//! delta sets are tiny (usually empty), so shipping paths across a rayon
//! boundary would cost more than the glob checks it saves. The feature is
//! zero-cost when disabled: one bool branch in the pipeline.

use std::collections::HashMap;
use std::path::Path;

use crate::error::{Error, Result};
use crate::git::sha::ShaResolver;
use crate::interner::StringInterner;
use crate::types::{InternedString, VanishedFile};

/// Result of a vanished-file scan
#[derive(Debug, Default)]
pub struct VanishedScan {
    /// Vanished files, ordered newest-deletion-first (the first entry's
    /// `last_seen_sha` is the natural group-level reconstruction commit)
    pub vanished: Vec<VanishedFile>,
    /// Commits visited on the first-parent chain
    pub commits_walked: u32,
    /// True when the vanished_max_commits cap was hit (results may be partial)
    pub truncated: bool,
    /// Soft anomalies (e.g. rename cycle) — caller maps these to diagnostics
    pub anomalies: Vec<String>,
}

/// Per-path event record (newest event wins — the walk is newest-first).
///
/// Detection is deterministic and purely path-based: a path is tracked by its
/// exact repo-relative string, never by content similarity. Renames are
/// intentionally NOT detected — `git mv a b` is seen as a delete of `a` plus an
/// add of `b`, and each path is judged on its own. For a path-keyed deployment
/// (each stack's state lives at a fixed backend path), a file absent at head but
/// present intra-range must have its path destroyed regardless of whether some
/// other path gained similar content; content-similarity rename pairing would
/// wrongly suppress that destroy — the exact bug this design avoids.
struct PathEvents {
    added_in_range: bool,
    /// First parent of the deleting commit (the last commit where the file
    /// existed); set on the newest deletion observed for this path.
    deleted_at: Option<git2::Oid>,
}

/// First-parent history walker detecting files added then removed within
/// base..head. Holds the repo path (git2 objects are `!Send`) and opens the
/// repository once per `detect_sync` call, mirroring `ShaResolver`.
pub struct VanishedDetector<'a> {
    repo_path: &'a Path,
    interner: &'a StringInterner,
}

impl<'a> VanishedDetector<'a> {
    /// Create a new detector rooted at the repository path
    pub fn new(repo_path: &'a Path, interner: &'a StringInterner) -> Self {
        Self {
            repo_path,
            interner,
        }
    }

    /// Walk base..head (first-parent) and report files that were added in the
    /// range, match `matches`, and no longer exist in the head tree.
    ///
    /// `matches` is a statically-dispatched predicate over repo-relative
    /// paths. `pathspecs` are coarse literal prefixes (see
    /// [`pathspec_prefixes`]) used to keep the common per-commit diff cheap;
    /// correctness never depends on them. `max_commits` of 0 means unlimited.
    pub fn detect_sync<F: Fn(&str) -> bool>(
        &self,
        base_sha: &str,
        head_sha: &str,
        matches: F,
        pathspecs: &[String],
        max_commits: u32,
    ) -> Result<VanishedScan> {
        let mut scan = VanishedScan::default();
        if base_sha == head_sha {
            return Ok(scan);
        }

        let repo = git2::Repository::open(self.repo_path)?;
        let head_oid = git2::Oid::from_str(head_sha)?;
        let head_tree = repo.find_commit(head_oid)?.tree()?;

        // Initial-push base (the empty tree SHA) has no commit to hide; the
        // walk then covers head's whole first-parent history, bounded by the
        // cap. A missing/unreachable base (shallow clone, force push) is a
        // hard error here — the caller soft-fails it into a diagnostic.
        let base_tree = if base_sha == ShaResolver::empty_tree_sha() {
            None
        } else {
            let base_oid = git2::Oid::from_str(base_sha)?;
            let base_commit = repo.find_commit(base_oid).map_err(|e| {
                Error::Git(format!(
                    "base SHA {} not found (shallow clone or force push?): {}",
                    base_sha, e
                ))
            })?;
            Some(base_commit.tree()?)
        };

        let mut walk = repo.revwalk()?;
        walk.push(head_oid)?;
        if base_tree.is_some() {
            walk.hide(git2::Oid::from_str(base_sha)?)?;
        }
        walk.simplify_first_parent()?;
        walk.set_sorting(git2::Sort::TOPOLOGICAL)?;

        let mut events: HashMap<InternedString, PathEvents> = HashMap::new();
        let mut deletion_order: HashMap<InternedString, u32> = HashMap::new();

        for oid in walk {
            if max_commits > 0 && scan.commits_walked >= max_commits {
                scan.truncated = true;
                break;
            }
            let oid = oid?;
            scan.commits_walked += 1;

            let commit = repo.find_commit(oid)?;
            let commit_tree = commit.tree()?;
            // Root commit diffs against the empty tree
            let parent = commit.parent(0).ok();
            let parent_tree = match &parent {
                Some(p) => Some(p.tree()?),
                None => None,
            };

            // Single pathspec-filtered diff against the first parent. Rename
            // detection is intentionally OFF (no `find_similar`): `git mv a b`
            // yields Delete(a) + Add(b), and each path is judged on its own.
            // Determinism over guessing — content-similarity rename pairing would
            // suppress the destroy of a path whose file merely moved, or whose
            // lookalike was added elsewhere, which is wrong for path-keyed state.
            // Commits touching nothing under the prefixes produce an empty delta
            // set for near-zero cost.
            let mut opts = git2::DiffOptions::new();
            opts.ignore_submodules(true);
            for p in pathspecs {
                opts.pathspec(p);
            }
            let diff =
                repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&commit_tree), Some(&mut opts))?;

            let parent_oid = parent.as_ref().map(|p| p.id());
            for delta in diff.deltas() {
                match delta.status() {
                    git2::Delta::Deleted => {
                        let Some(path) = delta.old_file().path().and_then(|p| p.to_str()) else {
                            continue;
                        };
                        if !matches(path) {
                            continue;
                        }
                        let key = self.interner.intern(path);
                        let entry = events.entry(key).or_insert(PathEvents {
                            added_in_range: false,
                            deleted_at: None,
                        });
                        // Newest deletion wins (walk is newest-first). A deletion
                        // in a root commit is impossible (nothing existed before
                        // it), so parent_oid is always Some here.
                        if entry.deleted_at.is_none() {
                            if let Some(parent_oid) = parent_oid {
                                entry.deleted_at = Some(parent_oid);
                                deletion_order.insert(key, scan.commits_walked);
                            }
                        }
                    }
                    git2::Delta::Added => {
                        let Some(path) = delta.new_file().path().and_then(|p| p.to_str()) else {
                            continue;
                        };
                        if !matches(path) {
                            continue;
                        }
                        let key = self.interner.intern(path);
                        events
                            .entry(key)
                            .or_insert(PathEvents {
                                added_in_range: false,
                                deleted_at: None,
                            })
                            .added_in_range = true;
                    }
                    _ => {}
                }
            }
        }

        // Resolution: a path vanished when it was added in-range, is absent from
        // the head tree, did not exist at base (the endpoint diff already reports
        // base-existing deletions), and a deletion was observed for it on the
        // first-parent chain. Every path stands on its own — no rename chains, no
        // content similarity, no guessing.
        for (&path, ev) in &events {
            if !ev.added_in_range {
                continue;
            }
            let Some(path_str) = self.interner.resolve(path) else {
                continue;
            };
            if head_tree.get_path(Path::new(path_str)).is_ok() {
                continue; // alive at head (re-added or never really gone)
            }
            if let Some(base_tree) = &base_tree {
                if base_tree.get_path(Path::new(path_str)).is_ok() {
                    continue; // existed at base -> endpoint Deleted covers it
                }
            }

            match ev.deleted_at {
                Some(parent_oid) => scan.vanished.push(VanishedFile {
                    path,
                    last_seen_sha: self.interner.intern(&parent_oid.to_string()),
                }),
                None => scan.anomalies.push(format!(
                    "'{}' was added in range and is absent at head, but no \
                     deletion was observed on the first-parent chain within \
                     the walked window",
                    path_str
                )),
            }
        }

        // Deterministic order: newest deletion first, path as tie-break.
        scan.vanished.sort_by(|a, b| {
            let oa = deletion_order.get(&a.path).copied().unwrap_or(u32::MAX);
            let ob = deletion_order.get(&b.path).copied().unwrap_or(u32::MAX);
            oa.cmp(&ob).then_with(|| {
                self.interner
                    .resolve(a.path)
                    .cmp(&self.interner.resolve(b.path))
            })
        });

        Ok(scan)
    }
}

/// Derive coarse literal pathspec prefixes from glob patterns: the substring
/// up to the first glob metacharacter (`*?[{`), truncated at the last `/`.
/// A pattern with no derivable prefix yields no entry; if ANY pattern lacks a
/// prefix the result is empty (meaning: diff everything — pathspec filtering
/// is an optimization, never a correctness gate).
pub fn pathspec_prefixes(patterns: &[&str]) -> Vec<String> {
    let mut prefixes: Vec<String> = Vec::new();
    for pat in patterns {
        let meta = pat.find(['*', '?', '[', '{']).unwrap_or(pat.len());
        let literal = &pat[..meta];
        let Some(slash) = literal.rfind('/') else {
            return Vec::new(); // pattern can match at repo root: no filter
        };
        let prefix = &literal[..slash];
        if prefix.is_empty() {
            return Vec::new();
        }
        if !prefixes.iter().any(|p| p == prefix) {
            prefixes.push(prefix.to_string());
        }
    }
    prefixes
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;
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
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    fn new_repo() -> TempDir {
        let dir = TempDir::new().unwrap();
        git(dir.path(), &["init", "-q", "-b", "main"]);
        dir
    }

    fn write_and_commit(dir: &Path, files: &[(&str, &str)], msg: &str) -> String {
        for (path, content) in files {
            let full = dir.join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, content).unwrap();
        }
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-qm", msg]);
        git(dir, &["rev-parse", "HEAD"])
    }

    fn rm_and_commit(dir: &Path, paths: &[&str], msg: &str) -> String {
        for p in paths {
            git(dir, &["rm", "-rq", p]);
        }
        git(dir, &["commit", "-qm", msg]);
        git(dir, &["rev-parse", "HEAD"])
    }

    fn matches_stacks(p: &str) -> bool {
        p.starts_with("stacks/") && p.ends_with("Pulumi.yaml")
    }

    fn detect(
        dir: &Path,
        interner: &StringInterner,
        base: &str,
        head: &str,
        max: u32,
    ) -> VanishedScan {
        VanishedDetector::new(dir, interner)
            .detect_sync(base, head, matches_stacks, &["stacks".to_string()], max)
            .unwrap()
    }

    fn resolve<'i>(interner: &'i StringInterner, scan: &VanishedScan) -> Vec<(&'i str, String)> {
        scan.vanished
            .iter()
            .map(|v| {
                (
                    interner.resolve(v.path).unwrap(),
                    interner.resolve(v.last_seen_sha).unwrap().to_string(),
                )
            })
            .collect()
    }

    #[test]
    fn test_basic_vanished() {
        let dir = new_repo();
        let base = write_and_commit(dir.path(), &[("README.md", "r")], "base");
        let add = write_and_commit(dir.path(), &[("stacks/a/Pulumi.yaml", "name: a")], "add");
        let head = rm_and_commit(dir.path(), &["stacks/a"], "remove");

        let interner = StringInterner::new();
        let scan = detect(dir.path(), &interner, &base, &head, 0);
        let got = resolve(&interner, &scan);
        assert_eq!(got, vec![("stacks/a/Pulumi.yaml", add)]);
        assert!(!scan.truncated);
        assert!(scan.anomalies.is_empty());
    }

    #[test]
    fn test_not_vanished_when_alive_at_head() {
        let dir = new_repo();
        let base = write_and_commit(dir.path(), &[("README.md", "r")], "base");
        write_and_commit(dir.path(), &[("stacks/a/Pulumi.yaml", "v1")], "add");
        let head = write_and_commit(dir.path(), &[("stacks/a/Pulumi.yaml", "v2")], "modify");

        let interner = StringInterner::new();
        let scan = detect(dir.path(), &interner, &base, &head, 0);
        assert!(scan.vanished.is_empty());
    }

    #[test]
    fn test_readded_at_head_not_vanished() {
        let dir = new_repo();
        let base = write_and_commit(dir.path(), &[("README.md", "r")], "base");
        write_and_commit(dir.path(), &[("stacks/a/Pulumi.yaml", "v1")], "add");
        rm_and_commit(dir.path(), &["stacks/a"], "remove");
        let head = write_and_commit(dir.path(), &[("stacks/a/Pulumi.yaml", "v2")], "re-add");

        let interner = StringInterner::new();
        let scan = detect(dir.path(), &interner, &base, &head, 0);
        assert!(scan.vanished.is_empty());
    }

    #[test]
    fn test_add_remove_readd_remove_uses_newest_deletion() {
        let dir = new_repo();
        let base = write_and_commit(dir.path(), &[("README.md", "r")], "base");
        write_and_commit(dir.path(), &[("stacks/a/Pulumi.yaml", "v1")], "add1");
        rm_and_commit(dir.path(), &["stacks/a"], "rm1");
        let add2 = write_and_commit(dir.path(), &[("stacks/a/Pulumi.yaml", "v2")], "add2");
        let head = rm_and_commit(dir.path(), &["stacks/a"], "rm2");

        let interner = StringInterner::new();
        let scan = detect(dir.path(), &interner, &base, &head, 0);
        let got = resolve(&interner, &scan);
        // last_seen must be add2 (parent of the NEWEST deletion), content v2
        assert_eq!(got, vec![("stacks/a/Pulumi.yaml", add2)]);
    }

    #[test]
    fn test_rename_within_scope_vanishes_old_path() {
        // Path-exact & deterministic: `git mv a b` = delete of a + add of b.
        // a was added in range and is gone at head -> a vanishes (its path's
        // deployed state must be destroyed). b is alive at head -> deployed, not
        // vanished. Content similarity between a and b is irrelevant.
        let dir = new_repo();
        let base = write_and_commit(dir.path(), &[("README.md", "r")], "base");
        let add = write_and_commit(
            dir.path(),
            &[("stacks/a/Pulumi.yaml", "same-content")],
            "add",
        );
        git(dir.path(), &["mv", "stacks/a", "stacks/b"]);
        git(dir.path(), &["commit", "-qm", "rename"]);
        let head = git(dir.path(), &["rev-parse", "HEAD"]);

        let interner = StringInterner::new();
        let scan = detect(dir.path(), &interner, &base, &head, 0);
        let got = resolve(&interner, &scan);
        // last_seen for a = parent of the rename commit = the add commit.
        assert_eq!(got, vec![("stacks/a/Pulumi.yaml", add)]);
    }

    #[test]
    fn test_rename_chain_then_delete_vanishes_every_path() {
        // add a -> rename a->b -> delete b. Both stacks/a and stacks/b held a
        // matching file that is gone at head, and both were added in range, so
        // both paths vanish (each may have deployed state to destroy). Ordered
        // newest-deletion-first: b (deleted last) before a.
        let dir = new_repo();
        let base = write_and_commit(dir.path(), &[("README.md", "r")], "base");
        let add = write_and_commit(
            dir.path(),
            &[("stacks/a/Pulumi.yaml", "same-content")],
            "add",
        );
        git(dir.path(), &["mv", "stacks/a", "stacks/b"]);
        git(dir.path(), &["commit", "-qm", "rename"]);
        let renamed = git(dir.path(), &["rev-parse", "HEAD"]);
        let head = rm_and_commit(dir.path(), &["stacks/b"], "remove");

        let interner = StringInterner::new();
        let scan = detect(dir.path(), &interner, &base, &head, 0);
        let got = resolve(&interner, &scan);
        assert_eq!(
            got,
            vec![
                ("stacks/b/Pulumi.yaml", renamed),
                ("stacks/a/Pulumi.yaml", add),
            ]
        );
    }

    #[test]
    fn test_move_out_of_scope_vanishes_old_path() {
        // Moving the file out of the glob scope removes it from its deploy path;
        // path-exact treats that as a deletion of stacks/a (destroy its state).
        // The out-of-scope add (archive/) does not match and is ignored.
        let dir = new_repo();
        let base = write_and_commit(dir.path(), &[("README.md", "r")], "base");
        let add = write_and_commit(
            dir.path(),
            &[("stacks/a/Pulumi.yaml", "same-content")],
            "add",
        );
        fs::create_dir_all(dir.path().join("archive")).unwrap();
        git(
            dir.path(),
            &["mv", "stacks/a/Pulumi.yaml", "archive/Pulumi.yaml"],
        );
        git(dir.path(), &["commit", "-qm", "archive"]);
        let head = git(dir.path(), &["rev-parse", "HEAD"]);

        let interner = StringInterner::new();
        let scan = detect(dir.path(), &interner, &base, &head, 0);
        let got = resolve(&interner, &scan);
        assert_eq!(got, vec![("stacks/a/Pulumi.yaml", add)]);
    }

    #[test]
    fn test_existed_at_base_excluded() {
        let dir = new_repo();
        let base = write_and_commit(
            dir.path(),
            &[("stacks/a/Pulumi.yaml", "v0"), ("README.md", "r")],
            "base-with-stack",
        );
        rm_and_commit(dir.path(), &["stacks/a"], "rm");
        write_and_commit(dir.path(), &[("stacks/a/Pulumi.yaml", "v1")], "re-add");
        let head = rm_and_commit(dir.path(), &["stacks/a"], "rm-again");

        let interner = StringInterner::new();
        let scan = detect(dir.path(), &interner, &base, &head, 0);
        assert!(
            scan.vanished.is_empty(),
            "existed at base -> endpoint diff reports Deleted; no double report"
        );
    }

    #[test]
    fn test_first_parent_scoping_ignores_side_branch() {
        let dir = new_repo();
        let base = write_and_commit(dir.path(), &[("README.md", "r")], "base");
        // side branch adds AND deletes a matching file, then merges
        git(dir.path(), &["checkout", "-qb", "side"]);
        write_and_commit(dir.path(), &[("stacks/side/Pulumi.yaml", "s")], "side-add");
        rm_and_commit(dir.path(), &["stacks/side"], "side-rm");
        git(dir.path(), &["checkout", "-q", "main"]);
        write_and_commit(dir.path(), &[("mainline.txt", "m")], "mainline");
        git(
            dir.path(),
            &["merge", "-q", "--no-ff", "-m", "merge side", "side"],
        );
        let head = git(dir.path(), &["rev-parse", "HEAD"]);

        let interner = StringInterner::new();
        let scan = detect(dir.path(), &interner, &base, &head, 0);
        assert!(
            scan.vanished.is_empty(),
            "side-branch churn never existed on the first-parent mainline"
        );
        // mainline: merge + mainline + base-excluded => walked visits merge and mainline commits only
        assert!(scan.commits_walked <= 3);
    }

    #[test]
    fn test_commit_cap_truncation() {
        let dir = new_repo();
        let base = write_and_commit(dir.path(), &[("README.md", "r")], "base");
        write_and_commit(dir.path(), &[("stacks/a/Pulumi.yaml", "a")], "add");
        for i in 0..4 {
            write_and_commit(dir.path(), &[("pad.txt", &format!("{i}"))], "pad");
        }
        let head = rm_and_commit(dir.path(), &["stacks/a"], "rm");

        let interner = StringInterner::new();
        let scan = detect(dir.path(), &interner, &base, &head, 2);
        assert!(scan.truncated);
        // The deletion IS seen (newest commits first) but the Add is beyond
        // the cap -> no vanished entry, plus no anomaly (added_in_range false)
        assert!(scan.vanished.is_empty());
    }

    #[test]
    fn test_base_equals_head_empty() {
        let dir = new_repo();
        let sha = write_and_commit(dir.path(), &[("README.md", "r")], "base");
        let interner = StringInterner::new();
        let scan = detect(dir.path(), &interner, &sha, &sha, 0);
        assert!(scan.vanished.is_empty());
        assert_eq!(scan.commits_walked, 0);
    }

    #[test]
    fn test_empty_tree_base_initial_push() {
        let dir = new_repo();
        write_and_commit(dir.path(), &[("stacks/a/Pulumi.yaml", "a")], "add");
        let head = rm_and_commit(dir.path(), &["stacks/a"], "rm");

        let interner = StringInterner::new();
        let scan = detect(
            dir.path(),
            &interner,
            ShaResolver::empty_tree_sha(),
            &head,
            0,
        );
        assert_eq!(scan.vanished.len(), 1);
    }

    #[test]
    fn test_missing_base_is_error() {
        let dir = new_repo();
        let head = write_and_commit(dir.path(), &[("README.md", "r")], "only");
        let interner = StringInterner::new();
        let err = VanishedDetector::new(dir.path(), &interner)
            .detect_sync(
                "1111111111111111111111111111111111111111",
                &head,
                matches_stacks,
                &[],
                0,
            )
            .unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn test_glob_filtering_ignores_non_matching() {
        let dir = new_repo();
        let base = write_and_commit(dir.path(), &[("README.md", "r")], "base");
        write_and_commit(dir.path(), &[("docs/temp.md", "d")], "add-doc");
        git(dir.path(), &["rm", "-q", "docs/temp.md"]);
        git(dir.path(), &["commit", "-qm", "rm-doc"]);
        let head = git(dir.path(), &["rev-parse", "HEAD"]);

        let interner = StringInterner::new();
        let scan = detect(dir.path(), &interner, &base, &head, 0);
        assert!(scan.vanished.is_empty());
    }

    #[test]
    fn test_pathspec_prefixes() {
        assert_eq!(
            pathspec_prefixes(&["stacks/**/Pulumi.yaml"]),
            vec!["stacks".to_string()]
        );
        assert_eq!(
            pathspec_prefixes(&["stacks/**/Pulumi.yaml", "apps/prod/*.yaml"]),
            vec!["stacks".to_string(), "apps/prod".to_string()]
        );
        // A rootless pattern disables filtering entirely (correctness first)
        assert!(pathspec_prefixes(&["*.md"]).is_empty());
        assert!(pathspec_prefixes(&["stacks/**", "*.md"]).is_empty());
        // Dedup
        assert_eq!(
            pathspec_prefixes(&["stacks/a/**", "stacks/b/**"]),
            vec!["stacks/a".to_string(), "stacks/b".to_string()]
        );
    }

    #[test]
    fn test_multiple_vanished_ordered_newest_first() {
        let dir = new_repo();
        let base = write_and_commit(dir.path(), &[("README.md", "r")], "base");
        write_and_commit(dir.path(), &[("stacks/a/Pulumi.yaml", "a")], "add-a");
        let add_b = write_and_commit(dir.path(), &[("stacks/b/Pulumi.yaml", "b")], "add-b");
        let rm_a_parent = add_b.clone(); // a deleted next -> parent is add-b commit
        rm_and_commit(dir.path(), &["stacks/a"], "rm-a");
        let rm_b_parent = git(dir.path(), &["rev-parse", "HEAD"]);
        let head = rm_and_commit(dir.path(), &["stacks/b"], "rm-b");

        let interner = StringInterner::new();
        let scan = detect(dir.path(), &interner, &base, &head, 0);
        let got = resolve(&interner, &scan);
        // b deleted last (newest) -> first in the list
        assert_eq!(
            got,
            vec![
                ("stacks/b/Pulumi.yaml", rm_b_parent),
                ("stacks/a/Pulumi.yaml", rm_a_parent),
            ]
        );
    }
}
