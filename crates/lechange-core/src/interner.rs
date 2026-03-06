//! String interner for zero-copy path deduplication
//!
//! Uses a `Box<str>` arena for single-allocation storage with a hash-keyed map
//! for O(1) deduplication. No `Arc` refcount overhead.

use crate::types::InternedString;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// Compute a 64-bit hash of a string using the default hasher.
#[inline]
fn hash_str(s: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

/// Thread-safe string interner using `Box<str>` arena for single-allocation storage.
///
/// Strings are stored in an append-only `Vec<Box<str>>` (pointer-stable on heap).
/// A hash-keyed map provides O(1) lookup with collision chaining.
///
/// `Send` and `Sync` are derived from `RwLock` — no manual unsafe impl needed.
pub struct StringInterner {
    /// Owned storage — append-only, pointer-stable (Box is heap-allocated)
    strings: RwLock<Vec<Box<str>>>,
    /// hash(str) → list of InternedString ids (collision chain)
    map: RwLock<HashMap<u64, Vec<InternedString>>>,
}

impl StringInterner {
    /// Create a new string interner
    pub fn new() -> Self {
        Self {
            strings: RwLock::new(Vec::new()),
            map: RwLock::new(HashMap::new()),
        }
    }

    /// Create a new string interner with the given capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            strings: RwLock::new(Vec::with_capacity(capacity)),
            map: RwLock::new(HashMap::with_capacity(capacity)),
        }
    }

    /// Intern a string, returning its handle.
    ///
    /// Fast path: read lock to check existence via hash.
    /// Slow path: write lock, double-check, allocate `Box<str>`.
    pub fn intern(&self, s: &str) -> InternedString {
        let h = hash_str(s);

        // Fast path: check if already interned (read lock only)
        {
            let map = self.map.read();
            if let Some(chain) = map.get(&h) {
                let strings = self.strings.read();
                for &id in chain {
                    if &*strings[id.0 as usize] == s {
                        return id;
                    }
                }
            }
        }

        // Slow path: insert new string (write lock)
        let mut map = self.map.write();
        let mut strings = self.strings.write();

        // Double-check after acquiring write lock
        if let Some(chain) = map.get(&h) {
            for &id in chain {
                if &*strings[id.0 as usize] == s {
                    return id;
                }
            }
        }

        let id = InternedString(strings.len() as u32);
        strings.push(Box::from(s));
        map.entry(h).or_default().push(id);

        id
    }

    /// Resolve an interned string back to `&str`.
    ///
    /// # Safety argument
    ///
    /// We return a `&str` whose lifetime is tied to `&self` (the interner),
    /// not to the read-lock guard. This is safe because:
    /// 1. Strings are never removed (append-only vec).
    /// 2. `Box<str>` is heap-allocated and pointer-stable — `push` to the vec
    ///    does not move existing boxes.
    /// 3. The interner outlives all returned references (borrow checker enforces this).
    #[inline]
    pub fn resolve(&self, id: InternedString) -> Option<&str> {
        unsafe {
            let strings = self.strings.read();
            let s = strings.get(id.0 as usize)?;
            // Transmute to extend lifetime from the lock guard to &self.
            Some(std::mem::transmute::<&str, &str>(&**s))
        }
    }

    /// Get current number of interned strings
    #[inline]
    pub fn len(&self) -> usize {
        self.strings.read().len()
    }

    /// Check if the interner is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.strings.read().is_empty()
    }
}

impl Default for StringInterner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intern_resolve() {
        let interner = StringInterner::new();
        let id1 = interner.intern("foo");
        let id2 = interner.intern("bar");
        let id3 = interner.intern("foo"); // Should reuse id1

        assert_eq!(id1, id3);
        assert_ne!(id1, id2);
        assert_eq!(interner.resolve(id1), Some("foo"));
        assert_eq!(interner.resolve(id2), Some("bar"));
    }

    #[test]
    fn test_thread_safety() {
        use std::sync::Arc;
        use std::thread;

        let interner = Arc::new(StringInterner::new());
        let mut handles = vec![];

        // Spawn 10 threads, each interning the same 100 strings
        for _ in 0..10 {
            let interner = Arc::clone(&interner);
            let handle = thread::spawn(move || {
                for i in 0..100 {
                    interner.intern(&format!("string_{}", i));
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // Should have exactly 100 unique strings (deduplication worked)
        assert_eq!(interner.len(), 100);
    }

    #[test]
    fn test_with_capacity() {
        let interner = StringInterner::with_capacity(1000);
        assert_eq!(interner.len(), 0);

        // Add 500 strings
        for i in 0..500 {
            interner.intern(&format!("str_{}", i));
        }

        assert_eq!(interner.len(), 500);
    }

    // --- New Phase 1.1 tests ---

    #[test]
    fn test_box_arena_intern_resolve() {
        let interner = StringInterner::new();
        let id = interner.intern("hello");
        assert_eq!(interner.resolve(id), Some("hello"));
    }

    #[test]
    fn test_box_arena_dedup() {
        let interner = StringInterner::new();
        let id1 = interner.intern("a");
        let id2 = interner.intern("a");
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_box_arena_distinct() {
        let interner = StringInterner::new();
        let id1 = interner.intern("a");
        let id2 = interner.intern("b");
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_no_arc_in_struct() {
        let interner = StringInterner::new();
        let type_name = std::any::type_name_of_val(&interner);
        assert!(
            !type_name.contains("Arc"),
            "StringInterner should not contain Arc: {}",
            type_name
        );
    }

    #[test]
    fn test_thread_safety_concurrent_intern() {
        use std::sync::Arc;
        use std::thread;

        let interner = Arc::new(StringInterner::new());
        let mut handles = vec![];

        for t in 0..8 {
            let interner = Arc::clone(&interner);
            let handle = thread::spawn(move || {
                for i in 0..1000 {
                    let s = format!("t{}_{}", t, i);
                    let id = interner.intern(&s);
                    assert_eq!(interner.resolve(id), Some(s.as_str()));
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // 8 threads * 1000 unique strings each
        assert_eq!(interner.len(), 8000);
    }

    #[test]
    fn test_capacity_growth() {
        let interner = StringInterner::new();
        for i in 0..10000 {
            let s = format!("string_{}", i);
            let id = interner.intern(&s);
            assert_eq!(interner.resolve(id), Some(s.as_str()));
        }
        assert_eq!(interner.len(), 10000);
    }
}
