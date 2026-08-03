// Copyright (c) 2024-2026 Lituus-io. All rights reserved.
//! File operations with symlink detection and caching

use crate::error::{Error, Result};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Bounded cache for symlink detection results.
///
/// Eviction is clear-on-full: when the map reaches `max_size` it is emptied
/// wholesale. That is deliberate — entries are cheap to recompute (one lstat)
/// and a CI run rarely revisits evicted paths, so tracking access order would
/// cost more than it saves.
struct BoundedCache<K, V> {
    map: HashMap<K, V>,
    max_size: usize,
}

impl<K: Eq + std::hash::Hash + Clone, V> BoundedCache<K, V> {
    fn new(max_size: usize) -> Self {
        Self {
            map: HashMap::with_capacity(max_size),
            max_size,
        }
    }

    fn get(&self, key: &K) -> Option<&V> {
        self.map.get(key)
    }

    fn put(&mut self, key: K, value: V) {
        if self.map.len() >= self.max_size {
            self.map.clear();
        }
        self.map.insert(key, value);
    }
}

/// File operations handler with symlink caching
pub struct FileOps {
    symlink_cache: RwLock<BoundedCache<PathBuf, bool>>,
}

impl FileOps {
    /// Create a new file operations handler
    pub fn new() -> Self {
        Self::with_cache_size(1024)
    }

    /// Create with custom cache size
    pub fn with_cache_size(cache_size: usize) -> Self {
        Self {
            symlink_cache: RwLock::new(BoundedCache::new(cache_size)),
        }
    }

    /// Check if a path is a symlink (sync, called from Rayon workers)
    pub fn is_symlink_sync(&self, path: &Path) -> Result<bool> {
        let path_buf = path.to_path_buf();

        // Check cache first
        {
            let cache = self.symlink_cache.read();
            if let Some(&cached) = cache.get(&path_buf) {
                return Ok(cached);
            }
        }

        // Check disk with std::fs
        let metadata = std::fs::symlink_metadata(&path_buf).map_err(Error::Io)?;

        let is_link = metadata.file_type().is_symlink();

        // Cache result
        {
            let mut cache = self.symlink_cache.write();
            cache.put(path_buf, is_link);
        }

        Ok(is_link)
    }
}

impl Default for FileOps {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_is_symlink_sync() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("regular_file.txt");
        fs::write(&file_path, "content").unwrap();

        let ops = FileOps::new();
        let is_link = ops.is_symlink_sync(&file_path).unwrap();
        assert!(!is_link);
    }

    #[test]
    fn test_symlink_cache() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "content").unwrap();

        let ops = FileOps::new();

        // First call - not cached
        let result1 = ops.is_symlink_sync(&file_path).unwrap();

        // Second call - should be cached
        let result2 = ops.is_symlink_sync(&file_path).unwrap();

        assert_eq!(result1, result2);
        assert!(!result1);
    }

    #[test]
    fn test_cache_eviction_clears_when_full() {
        let dir = TempDir::new().unwrap();
        let ops = FileOps::with_cache_size(2);

        for i in 0..5 {
            let p = dir.path().join(format!("f{i}.txt"));
            fs::write(&p, "content").unwrap();
            assert!(!ops.is_symlink_sync(&p).unwrap());
        }

        // Entries remain resolvable after eviction cycles
        let p0 = dir.path().join("f0.txt");
        assert!(!ops.is_symlink_sync(&p0).unwrap());
    }
}
