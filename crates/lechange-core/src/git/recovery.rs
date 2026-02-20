//! Deleted file recovery via git2 blob lookup

use crate::error::{Error, Result};
use crate::interner::StringInterner;
use crate::types::InternedString;
use rayon::prelude::*;
use std::path::{Path, PathBuf};

/// Recovers deleted file contents from git history
pub struct FileRecovery {
    repo_path: PathBuf,
}

impl FileRecovery {
    /// Create a new file recovery instance
    pub fn new<P: AsRef<Path>>(repo_path: P) -> Self {
        Self {
            repo_path: repo_path.as_ref().to_path_buf(),
        }
    }

    /// Recover a single deleted file from the given commit
    pub fn recover_file(&self, sha: &str, file_path: &str, output_dir: &Path) -> Result<PathBuf> {
        let repo = git2::Repository::open(&self.repo_path)?;
        let oid = git2::Oid::from_str(sha)
            .map_err(|e| Error::Recovery(format!("Invalid SHA '{}': {}", sha, e)))?;

        let commit = repo
            .find_commit(oid)
            .map_err(|e| Error::Recovery(format!("Commit not found '{}': {}", sha, e)))?;

        let tree = commit.tree()?;

        let entry = tree
            .get_path(Path::new(file_path))
            .map_err(|e| Error::Recovery(format!("File '{}' not in tree: {}", file_path, e)))?;

        let blob = repo
            .find_blob(entry.id())
            .map_err(|e| Error::Recovery(format!("Blob not found for '{}': {}", file_path, e)))?;

        // Write to output directory
        let output_path = output_dir.join(file_path);
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&output_path, blob.content())?;

        Ok(output_path)
    }

    /// Recover multiple deleted files in parallel
    pub fn recover_all_parallel(
        &self,
        sha: &str,
        file_paths: &[InternedString],
        interner: &StringInterner,
        output_dir: &Path,
    ) -> Vec<Result<PathBuf>> {
        file_paths
            .par_iter()
            .map(|path| {
                let path_str = interner.resolve(*path).ok_or_else(|| {
                    Error::Recovery("Could not resolve interned path".to_string())
                })?;
                self.recover_file(sha, path_str, output_dir)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_repo() -> (TempDir, String) {
        let dir = TempDir::new().unwrap();
        let repo_path = dir.path();

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

        // Create a file and commit
        std::fs::write(repo_path.join("recoverable.txt"), "original content").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(repo_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "Add recoverable file"])
            .current_dir(repo_path)
            .output()
            .unwrap();

        // Get the commit SHA
        let output = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo_path)
            .output()
            .unwrap();
        let sha = String::from_utf8(output.stdout).unwrap().trim().to_string();

        (dir, sha)
    }

    #[test]
    fn test_recover_file() {
        let (dir, sha) = create_test_repo();
        let output_dir = TempDir::new().unwrap();

        let recovery = FileRecovery::new(dir.path());
        let result = recovery.recover_file(&sha, "recoverable.txt", output_dir.path());
        assert!(result.is_ok());

        let output_path = result.unwrap();
        assert!(output_path.exists());
        let content = std::fs::read_to_string(&output_path).unwrap();
        assert_eq!(content, "original content");
    }

    #[test]
    fn test_recover_file_not_in_tree() {
        let (dir, sha) = create_test_repo();
        let output_dir = TempDir::new().unwrap();

        let recovery = FileRecovery::new(dir.path());
        let result = recovery.recover_file(&sha, "nonexistent.txt", output_dir.path());
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("not in tree"));
    }

    #[test]
    fn test_recover_file_invalid_sha() {
        let (dir, _) = create_test_repo();
        let output_dir = TempDir::new().unwrap();

        let recovery = FileRecovery::new(dir.path());
        let result = recovery.recover_file("invalid_sha", "recoverable.txt", output_dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_recover_all_parallel() {
        let (dir, _sha) = create_test_repo();
        let repo_path = dir.path();

        // Add more files
        std::fs::create_dir_all(repo_path.join("sub")).unwrap();
        std::fs::write(repo_path.join("sub/file2.txt"), "content2").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(repo_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "Add sub/file2.txt"])
            .current_dir(repo_path)
            .output()
            .unwrap();
        let output = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo_path)
            .output()
            .unwrap();
        let sha2 = String::from_utf8(output.stdout).unwrap().trim().to_string();

        let output_dir = TempDir::new().unwrap();
        let interner = StringInterner::new();
        let paths = vec![
            interner.intern("recoverable.txt"),
            interner.intern("sub/file2.txt"),
        ];

        let recovery = FileRecovery::new(repo_path);
        let results = recovery.recover_all_parallel(&sha2, &paths, &interner, output_dir.path());

        assert_eq!(results.len(), 2);
        for result in &results {
            assert!(result.is_ok());
            assert!(result.as_ref().unwrap().exists());
        }
    }
}
