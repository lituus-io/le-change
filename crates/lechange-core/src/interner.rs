//! String interner for zero-copy path deduplication

use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use crate::types::InternedString;

/// Thread-safe string interner using Arc for shared ownership
/// Uses copy-on-write strategy to minimize allocations
pub struct StringInterner {
    // Map from string content to ID
    map: RwLock<HashMap<Arc<str>, InternedString>>,
    // Reverse map from ID to string
    strings: RwLock<Vec<Arc<str>>>,
}

impl StringInterner {
    /// Create a new string interner
    pub fn new() -> Self {
        Self {
            map: RwLock::new(HashMap::new()),
            strings: RwLock::new(Vec::new()),
        }
    }

    /// Create a new string interner with the given capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            map: RwLock::new(HashMap::with_capacity(capacity)),
            strings: RwLock::new(Vec::with_capacity(capacity)),
        }
    }

    /// Intern a string, returning its handle
    /// First checks read lock, only acquires write lock if needed
    pub fn intern(&self, s: &str) -> InternedString {
        // Fast path: check if already interned (read lock only)
        {
            let map = self.map.read();
            if let Some(&id) = map.get(s) {
                return id;
            }
        }

        // Slow path: insert new string (write lock)
        let mut map = self.map.write();
        let mut strings = self.strings.write();

        // Double-check after acquiring write lock (another thread may have inserted)
        if let Some(&id) = map.get(s) {
            return id;
        }

        let id = InternedString(strings.len() as u32);
        let arc_str: Arc<str> = Arc::from(s);

        strings.push(arc_str.clone());
        map.insert(arc_str, id);

        id
    }

    /// Resolve an interned string back to &str
    #[inline]
    pub fn resolve(&self, id: InternedString) -> Option<&str> {
        // SAFETY: We hold a read lock, but we're returning a reference that outlives the lock.
        // This is safe because:
        // 1. Strings are never removed from the interner
        // 2. The Arc<str> ensures the string lives as long as the interner
        // 3. We use unsafe to extend the lifetime beyond the lock
        unsafe {
            let strings = self.strings.read();
            let s = strings.get(id.0 as usize)?;
            Some(std::mem::transmute::<&str, &str>(s.as_ref()))
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

// SAFETY: StringInterner uses interior mutability with RwLock, which is Send + Sync
unsafe impl Send for StringInterner {}
unsafe impl Sync for StringInterner {}

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
}
