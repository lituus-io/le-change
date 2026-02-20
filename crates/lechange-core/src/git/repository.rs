//! Git repository operations with async support

use std::future::Future;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::git::diff::DiffParser;
use crate::git::sha::ShaResolver;
use crate::interner::StringInterner;
use crate::traits::AsyncGitOps;
use crate::types::{ChangeType, ChangedFile, DiffResult};

/// Git repository wrapper that handles Send/Sync constraints
///
/// git2::Repository is not Send/Sync due to internal raw pointers.
/// We work around this by storing the path and using spawn_blocking
/// for all git operations.
pub struct GitRepository {
    path: PathBuf,
}

impl GitRepository {
    /// Open a repository at the given path
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        // Verify the repository exists
        let _repo = git2::Repository::open(&path)?;

        Ok(Self { path })
    }

    /// Discover a repository starting from the given path
    pub fn discover<P: AsRef<Path>>(path: P) -> Result<Self> {
        // discover_path() returns the path to the .git directory (PathBuf)
        let git_path = git2::Repository::discover_path(path.as_ref(), &[] as &[&std::ffi::OsStr])?;
        // Verify it can be opened
        let _repo = git2::Repository::open(&git_path)?;

        Ok(Self { path: git_path })
    }

    /// Get the repository path (the .git directory or workdir path)
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Get the working directory root (parent of .git directory).
    ///
    /// If `path` ends with `.git`, returns its parent. Otherwise returns `path` as-is.
    /// This is the directory that contains the actual source files.
    pub fn workdir(&self) -> &Path {
        if self.path.ends_with(".git") {
            self.path.parent().unwrap_or(&self.path)
        } else {
            &self.path
        }
    }

    /// Get or create a repository instance (for internal use)
    fn get_repo(&self) -> Result<git2::Repository> {
        // Always reopen the repository
        // git2 has internal caching so this is cheap
        Ok(git2::Repository::open(&self.path)?)
    }

    /// Resolve a SHA to its tree, handling the empty tree SHA (initial push).
    ///
    /// The empty tree SHA `4b825dc...` is a tree object, not a commit, so
    /// `find_commit()` fails on it. This method tries `find_commit().tree()`
    /// first, then falls back to `find_tree()` for the empty tree case.
    fn sha_to_tree<'r>(
        repo: &'r git2::Repository,
        oid: git2::Oid,
        sha: &str,
    ) -> Result<git2::Tree<'r>> {
        if sha == ShaResolver::empty_tree_sha() {
            // Empty tree SHA is a tree object, not a commit
            Ok(repo.find_tree(oid).or_else(|_| {
                // If the empty tree isn't in the ODB, create it
                let empty_oid = repo
                    .treebuilder(None)
                    .map_err(|e| Error::Git(format!("Failed to create empty tree: {}", e)))?
                    .write()
                    .map_err(|e| Error::Git(format!("Failed to write empty tree: {}", e)))?;
                repo.find_tree(empty_oid)
                    .map_err(|e| Error::Git(format!("Failed to find empty tree: {}", e)))
            })?)
        } else {
            Ok(repo.find_commit(oid)?.tree()?)
        }
    }

    /// Ensure repository has sufficient depth for diff
    pub async fn ensure_depth(&self, depth: u32) -> Result<()> {
        if depth == 0 {
            return Ok(());
        }

        let path = self.path.clone();

        tokio::task::spawn_blocking(move || {
            // Check if repository is shallow
            let _repo = git2::Repository::open(&path)?;

            // git2 doesn't have a direct shallow check, so we use git command
            let output = std::process::Command::new("git")
                .args(["rev-parse", "--is-shallow-repository"])
                .current_dir(&path)
                .output()
                .map_err(|e| Error::Git(format!("Failed to check shallow status: {}", e)))?;

            let is_shallow = String::from_utf8_lossy(&output.stdout).trim() == "true";

            if is_shallow {
                // Fetch additional depth
                let _output = std::process::Command::new("git")
                    .args(["fetch", &format!("--depth={}", depth)])
                    .current_dir(&path)
                    .output()
                    .map_err(|e| Error::Git(format!("Failed to deepen repository: {}", e)))?;
            }

            Ok::<_, Error>(())
        })
        .await
        .map_err(|e| Error::Runtime(format!("Task join error: {}", e)))?
    }

    /// Ensure repository has sufficient depth with retry and exponential deepening
    ///
    /// Tries to find the merge base between base and head. If the merge base
    /// is not reachable (shallow clone), deepens the repository and retries.
    pub async fn ensure_depth_with_retry(
        &self,
        base_sha: &str,
        head_sha: &str,
        initial_depth: u32,
        max_retries: u32,
    ) -> Result<()> {
        let path = self.path.clone();
        let base = base_sha.to_string();
        let head = head_sha.to_string();

        tokio::task::spawn_blocking(move || {
            let repo = git2::Repository::open(&path)?;

            // Check if repo is shallow
            let output = std::process::Command::new("git")
                .args(["rev-parse", "--is-shallow-repository"])
                .current_dir(&path)
                .output()
                .map_err(|e| Error::Git(format!("Failed to check shallow status: {}", e)))?;

            let is_shallow = String::from_utf8_lossy(&output.stdout).trim() == "true";

            if !is_shallow {
                return Ok(()); // Full clone, no deepening needed
            }

            // Try to find merge base; if it fails, deepen
            let base_oid = git2::Oid::from_str(&base)
                .map_err(|e| Error::Git(format!("Invalid base SHA: {}", e)))?;
            let head_oid = git2::Oid::from_str(&head)
                .map_err(|e| Error::Git(format!("Invalid head SHA: {}", e)))?;

            let mut depth = initial_depth.max(1);

            for attempt in 0..=max_retries {
                if repo.merge_base(base_oid, head_oid).is_ok() {
                    return Ok(()); // Merge base found
                }

                if attempt == max_retries {
                    return Err(Error::ShallowExhausted(format!(
                        "Could not find merge base between {} and {} after {} retries (depth={}). \
                         Consider using a deeper clone.",
                        base, head, max_retries, depth
                    )));
                }

                // Deepen the repository
                let deepen_output = std::process::Command::new("git")
                    .args(["fetch", &format!("--deepen={}", depth)])
                    .current_dir(&path)
                    .output()
                    .map_err(|e| Error::Git(format!("Failed to deepen repository: {}", e)))?;

                if !deepen_output.status.success() {
                    let stderr = String::from_utf8_lossy(&deepen_output.stderr);
                    return Err(Error::Git(format!(
                        "git fetch --deepen={} failed: {}",
                        depth, stderr
                    )));
                }

                depth *= 2; // Exponential backoff
            }

            Ok(())
        })
        .await
        .map_err(|e| Error::Runtime(format!("Task join error: {}", e)))?
    }

    /// Check if a path is a symlink in a specific tree (by SHA)
    ///
    /// Useful for detecting symlinks in deleted files where the working tree
    /// no longer has the file. Falls back to checking the git tree object.
    pub fn is_symlink_in_tree(&self, sha: &str, file_path: &str) -> Result<bool> {
        let repo = self.get_repo()?;
        let oid = git2::Oid::from_str(sha)
            .map_err(|e| Error::Git(format!("Invalid SHA '{}': {}", sha, e)))?;
        let commit = repo.find_commit(oid)?;
        let tree = commit.tree()?;

        match tree.get_path(std::path::Path::new(file_path)) {
            Ok(entry) => {
                // Symlinks have filemode 0o120000 (0x8000 in git)
                Ok(entry.filemode() == 0o120000)
            }
            Err(_) => Ok(false), // Path not found in tree
        }
    }

    /// Compute diff between two commits (sync version)
    pub fn diff_sync(
        &self,
        base_sha: &str,
        head_sha: &str,
        interner: &StringInterner,
        diff_filter: &str,
    ) -> Result<DiffResult> {
        let repo = self.get_repo()?;

        // Parse OIDs
        let base_oid = git2::Oid::from_str(base_sha)
            .map_err(|e| Error::Git(format!("Invalid base SHA '{}': {}", base_sha, e)))?;
        let head_oid = git2::Oid::from_str(head_sha)
            .map_err(|e| Error::Git(format!("Invalid head SHA '{}': {}", head_sha, e)))?;

        // Get trees (handles empty tree SHA for initial pushes)
        let base_tree = Self::sha_to_tree(&repo, base_oid, base_sha)?;
        let head_tree = Self::sha_to_tree(&repo, head_oid, head_sha)?;

        // Create diff options
        let mut opts = git2::DiffOptions::new();
        opts.ignore_submodules(true);

        // Compute diff
        let diff = repo.diff_tree_to_tree(Some(&base_tree), Some(&head_tree), Some(&mut opts))?;

        let mut result = DiffResult::default();
        let _parser = DiffParser::new(interner);

        // Process each delta
        diff.foreach(
            &mut |delta, _progress| {
                let status = delta.status();

                // Map git2 status to our ChangeType
                use crate::types::ChangeType;
                let change_type = match status {
                    git2::Delta::Added => ChangeType::Added,
                    git2::Delta::Deleted => ChangeType::Deleted,
                    git2::Delta::Modified => ChangeType::Modified,
                    git2::Delta::Renamed => ChangeType::Renamed,
                    git2::Delta::Copied => ChangeType::Copied,
                    git2::Delta::Typechange => ChangeType::TypeChanged,
                    git2::Delta::Conflicted => ChangeType::Unmerged,
                    _ => ChangeType::Unknown,
                };

                // Filter by diff_filter
                let type_char = change_type
                    .as_str()
                    .chars()
                    .next()
                    .unwrap_or('X')
                    .to_ascii_uppercase();
                if !diff_filter.contains(type_char) {
                    return true; // Continue
                }

                // Get file paths
                let new_file = delta.new_file();
                let old_file = delta.old_file();

                if let Some(new_path) = new_file.path().and_then(|p| p.to_str()) {
                    let previous_path = if change_type == ChangeType::Renamed
                        || change_type == ChangeType::Copied
                    {
                        old_file
                            .path()
                            .and_then(|p| p.to_str())
                            .map(|s| interner.intern(s))
                    } else {
                        None
                    };

                    result.files.push(crate::types::ChangedFile {
                        path: interner.intern(new_path),
                        change_type,
                        previous_path,
                        is_symlink: false,
                        submodule_depth: 0,
                        origin: crate::types::FileOrigin {
                            in_current_changes: true,
                            in_previous_failure: false,
                            in_previous_success: false,
                        },
                    });
                }

                true // Continue iteration
            },
            None,
            None,
            None,
        )?;

        Ok(result)
    }

    /// Resolve a reference to a SHA (sync version)
    pub fn resolve_sha_sync(&self, reference: &str) -> Result<String> {
        let repo = self.get_repo()?;

        // Try as direct OID first
        if let Ok(oid) = git2::Oid::from_str(reference) {
            // Verify it exists
            if repo.find_object(oid, None).is_ok() {
                return Ok(oid.to_string());
            }
        }

        // Try as reference
        let resolved = repo.revparse_single(reference).map_err(|e| {
            Error::Git(format!(
                "Failed to resolve reference '{}': {}",
                reference, e
            ))
        })?;

        Ok(resolved.id().to_string())
    }

    /// Get list of submodule paths (sync version)
    pub fn submodules_sync(&self) -> Result<Vec<String>> {
        let repo = self.get_repo()?;
        let mut result = Vec::new();

        for submodule in repo.submodules()? {
            if let Some(path) = submodule.path().to_str() {
                result.push(path.to_string());
            }
        }

        Ok(result)
    }
}

// Implement AsyncGitOps using spawn_blocking
impl AsyncGitOps for GitRepository {
    type Error = Error;

    type DiffFuture<'a>
        = impl Future<Output = Result<DiffResult>> + Send + 'a
    where
        Self: 'a;

    type ResolveShaFuture<'a>
        = impl Future<Output = Result<String>> + Send + 'a
    where
        Self: 'a;

    type SubmodulesFuture<'a>
        = impl Future<Output = Result<Vec<String>>> + Send + 'a
    where
        Self: 'a;

    fn diff<'a>(
        &'a self,
        base_sha: &'a str,
        head_sha: &'a str,
        interner: &'a StringInterner,
        diff_filter: &'a str,
    ) -> Self::DiffFuture<'a> {
        async move {
            // Clone necessary data for move into spawn_blocking
            let base_sha = base_sha.to_string();
            let head_sha = head_sha.to_string();
            let diff_filter = diff_filter.to_string();

            // StringInterner is Send + Sync, but we can't move it
            // Instead, we'll use the sync version directly since we're already async
            let repo = self.get_repo()?;
            let base_oid = git2::Oid::from_str(&base_sha)?;
            let head_oid = git2::Oid::from_str(&head_sha)?;

            // Get trees (handles empty tree SHA for initial pushes)
            let base_tree = Self::sha_to_tree(&repo, base_oid, &base_sha)?;
            let head_tree = Self::sha_to_tree(&repo, head_oid, &head_sha)?;

            let mut opts = git2::DiffOptions::new();
            opts.ignore_submodules(true);

            let diff =
                repo.diff_tree_to_tree(Some(&base_tree), Some(&head_tree), Some(&mut opts))?;
            let mut result = DiffResult::default();

            diff.foreach(
                &mut |delta, _progress| {
                    let status = delta.status();
                    let change_type = match status {
                        git2::Delta::Added => ChangeType::Added,
                        git2::Delta::Deleted => ChangeType::Deleted,
                        git2::Delta::Modified => ChangeType::Modified,
                        git2::Delta::Renamed => ChangeType::Renamed,
                        git2::Delta::Copied => ChangeType::Copied,
                        git2::Delta::Typechange => ChangeType::TypeChanged,
                        git2::Delta::Conflicted => ChangeType::Unmerged,
                        _ => ChangeType::Unknown,
                    };

                    let type_char = change_type
                        .as_str()
                        .chars()
                        .next()
                        .unwrap_or('X')
                        .to_ascii_uppercase();
                    if !diff_filter.contains(type_char) {
                        return true;
                    }

                    let new_file = delta.new_file();
                    let old_file = delta.old_file();

                    if let Some(new_path) = new_file.path().and_then(|p| p.to_str()) {
                        let previous_path = if change_type == ChangeType::Renamed
                            || change_type == ChangeType::Copied
                        {
                            old_file
                                .path()
                                .and_then(|p| p.to_str())
                                .map(|s| interner.intern(s))
                        } else {
                            None
                        };

                        result.files.push(ChangedFile {
                            path: interner.intern(new_path),
                            change_type,
                            previous_path,
                            is_symlink: false,
                            submodule_depth: 0,
                            origin: crate::types::FileOrigin {
                                in_current_changes: true,
                                in_previous_failure: false,
                                in_previous_success: false,
                            },
                        });
                    }

                    true
                },
                None,
                None,
                None,
            )?;

            Ok(result)
        }
    }

    fn resolve_sha<'a>(&'a self, reference: &'a str) -> Self::ResolveShaFuture<'a> {
        async move {
            let path = self.path.clone();
            let reference = reference.to_string();

            tokio::task::spawn_blocking(move || {
                let temp_repo = GitRepository::open(&path)?;
                temp_repo.resolve_sha_sync(&reference)
            })
            .await
            .map_err(|e| Error::Runtime(format!("Task join error: {}", e)))?
        }
    }

    fn submodules<'a>(&'a self) -> Self::SubmodulesFuture<'a> {
        async move {
            let path = self.path.clone();

            tokio::task::spawn_blocking(move || {
                let temp_repo = GitRepository::open(&path)?;
                temp_repo.submodules_sync()
            })
            .await
            .map_err(|e| Error::Runtime(format!("Task join error: {}", e)))?
        }
    }
}

// Implement Send + Sync since we handle git2::Repository correctly
unsafe impl Send for GitRepository {}
unsafe impl Sync for GitRepository {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_repo() -> (TempDir, GitRepository) {
        let dir = TempDir::new().unwrap();
        let repo_path = dir.path();

        // Initialize git repo
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(repo_path)
            .output()
            .unwrap();

        std::process::Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(repo_path)
            .output()
            .unwrap();

        std::process::Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(repo_path)
            .output()
            .unwrap();

        // Create initial commit
        fs::write(repo_path.join("file1.txt"), "content1").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(repo_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "Initial commit"])
            .current_dir(repo_path)
            .output()
            .unwrap();

        let git_repo = GitRepository::discover(repo_path).unwrap();
        (dir, git_repo)
    }

    #[test]
    fn test_open_repository() {
        let (_dir, repo) = create_test_repo();
        assert!(!repo.path.as_os_str().is_empty());
    }

    #[test]
    fn test_resolve_sha() {
        let (_dir, repo) = create_test_repo();

        // Resolve HEAD
        let sha = repo.resolve_sha_sync("HEAD").unwrap();
        assert_eq!(sha.len(), 40); // SHA is 40 hex characters
    }

    #[tokio::test]
    async fn test_async_resolve_sha() {
        let (_dir, repo) = create_test_repo();

        // Resolve HEAD asynchronously
        let sha = repo.resolve_sha("HEAD").await.unwrap();
        assert_eq!(sha.len(), 40);
    }

    #[test]
    fn test_diff_sync() {
        let (dir, repo) = create_test_repo();
        let repo_path = dir.path();

        // Get current SHA
        let base_sha = repo.resolve_sha_sync("HEAD").unwrap();

        // Create a new file
        fs::write(repo_path.join("file2.txt"), "content2").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(repo_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "Add file2"])
            .current_dir(repo_path)
            .output()
            .unwrap();

        let head_sha = repo.resolve_sha_sync("HEAD").unwrap();

        // Compute diff
        let interner = StringInterner::new();
        let result = repo
            .diff_sync(&base_sha, &head_sha, &interner, "ACDMRTUX")
            .unwrap();

        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].change_type, crate::types::ChangeType::Added);
        assert_eq!(interner.resolve(result.files[0].path), Some("file2.txt"));
    }

    #[test]
    fn test_is_symlink_in_tree() {
        let (dir, repo) = create_test_repo();
        let repo_path = dir.path();

        // Get the SHA with the regular file
        let sha = repo.resolve_sha_sync("HEAD").unwrap();

        // Regular file should not be a symlink
        let result = repo.is_symlink_in_tree(&sha, "file1.txt").unwrap();
        assert!(!result);

        // Create a symlink and commit it
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("file1.txt", repo_path.join("link.txt")).unwrap();
            std::process::Command::new("git")
                .args(["add", "link.txt"])
                .current_dir(repo_path)
                .output()
                .unwrap();
            std::process::Command::new("git")
                .args(["commit", "-m", "Add symlink"])
                .current_dir(repo_path)
                .output()
                .unwrap();

            let sha_with_link = repo.resolve_sha_sync("HEAD").unwrap();
            let is_link = repo.is_symlink_in_tree(&sha_with_link, "link.txt").unwrap();
            assert!(is_link);

            let is_regular = repo
                .is_symlink_in_tree(&sha_with_link, "file1.txt")
                .unwrap();
            assert!(!is_regular);
        }
    }

    #[test]
    fn test_is_symlink_in_tree_invalid_sha() {
        let (_dir, repo) = create_test_repo();

        let result =
            repo.is_symlink_in_tree("0000000000000000000000000000000000000000", "file1.txt");
        assert!(result.is_err());
    }

    #[test]
    fn test_is_symlink_in_tree_file_not_found() {
        let (_dir, repo) = create_test_repo();
        let sha = repo.resolve_sha_sync("HEAD").unwrap();

        // File not in tree returns false (not an error based on the implementation)
        let result = repo.is_symlink_in_tree(&sha, "nonexistent.txt").unwrap();
        assert!(!result);
    }

    #[test]
    fn test_git_repository_struct_size_no_arc_overhead() {
        // GitRepository should be exactly the size of a PathBuf (no Arc/Mutex wrapping).
        // If someone accidentally wraps the path in Arc<Mutex<PathBuf>>, this will fail.
        let repo_size = std::mem::size_of::<GitRepository>();
        let pathbuf_size = std::mem::size_of::<PathBuf>();
        assert_eq!(
            repo_size, pathbuf_size,
            "GitRepository size ({}) should equal PathBuf size ({}); no Arc/Mutex overhead expected",
            repo_size, pathbuf_size
        );
    }
}
