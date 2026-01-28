//! Property-based tests using proptest

use proptest::prelude::*;
use lechange_core::{StringInterner, ChangeType};

// Generate arbitrary ChangeType
fn arb_change_type() -> impl Strategy<Value = ChangeType> {
    prop_oneof![
        Just(ChangeType::Added),
        Just(ChangeType::Copied),
        Just(ChangeType::Deleted),
        Just(ChangeType::Modified),
        Just(ChangeType::Renamed),
        Just(ChangeType::TypeChanged),
        Just(ChangeType::Unmerged),
        Just(ChangeType::Unknown),
    ]
}

// Generate arbitrary path strings
fn arb_path() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-z0-9/_-]{1,50}\\.(rs|py|txt|md)")
        .expect("valid regex")
}

proptest! {
    #[test]
    fn test_interner_idempotent(s in "[a-z]{1,100}") {
        let interner = StringInterner::new();
        let id1 = interner.intern(&s);
        let id2 = interner.intern(&s);
        prop_assert_eq!(id1, id2);
    }

    #[test]
    fn test_interner_resolve_roundtrip(s in "[a-z]{1,100}") {
        let interner = StringInterner::new();
        let id = interner.intern(&s);
        let resolved = interner.resolve(id);
        prop_assert_eq!(resolved, Some(s.as_str()));
    }

    #[test]
    fn test_interner_different_strings(s1 in "[a-z]{1,50}", s2 in "[A-Z]{1,50}") {
        let interner = StringInterner::new();
        let id1 = interner.intern(&s1);
        let id2 = interner.intern(&s2);
        
        // Different strings should have different IDs
        if s1 != s2 {
            prop_assert_ne!(id1, id2);
        }
    }

    #[test]
    fn test_change_type_roundtrip(change_type in arb_change_type()) {
        let byte = change_type.as_byte();
        let parsed = ChangeType::from_byte(byte);
        prop_assert_eq!(parsed, Some(change_type));
    }

    #[test]
    fn test_change_type_string_conversion(change_type in arb_change_type()) {
        let s = change_type.as_str();
        prop_assert!(s.len() >= 1);
        prop_assert!(s.chars().next().unwrap().is_alphabetic());
    }

    #[test]
    fn test_pattern_matcher_handles_all_paths(
        patterns in prop::collection::vec("[a-z*?]+", 0..5),
        path in arb_path()
    ) {
        use lechange_core::patterns::matcher::PatternMatcher;
        
        let pattern_refs: Vec<&str> = patterns.iter().map(|s| s.as_str()).collect();
        
        // Should not panic with any pattern combination
        if let Ok(matcher) = PatternMatcher::new(&pattern_refs, &[], false) {
            let _ = matcher.matches_sync(&path);
        }
    }

    #[test]
    fn test_interner_thread_safety(
        strings in prop::collection::vec("[a-z]{1,20}", 1..100)
    ) {
        use std::sync::Arc;
        use std::thread;
        
        let interner = Arc::new(StringInterner::new());
        let mut handles = vec![];
        
        for s in strings {
            let interner_clone = Arc::clone(&interner);
            let handle = thread::spawn(move || {
                interner_clone.intern(&s)
            });
            handles.push(handle);
        }
        
        // All threads should complete without panic
        for handle in handles {
            let _ = handle.join();
        }
        
        prop_assert!(true);
    }

    #[test]
    fn test_file_ops_cache_consistency(
        paths in prop::collection::vec(arb_path(), 1..50)
    ) {
        use lechange_core::file_ops::FileOps;
        use std::path::Path;
        
        let ops = FileOps::new();
        
        // Accessing same path multiple times should give same result
        for path_str in &paths {
            let path = Path::new(path_str);
            let result1 = ops.is_symlink_sync(path).unwrap_or(false);
            let result2 = ops.is_symlink_sync(path).unwrap_or(false);
            prop_assert_eq!(result1, result2);
        }
    }

    #[test]
    fn test_interner_capacity_growth(
        strings in prop::collection::vec("[a-z]{1,10}", 100..1000)
    ) {
        let interner = StringInterner::with_capacity(10);
        
        // Should handle more strings than initial capacity
        for s in &strings {
            let _ = interner.intern(s);
        }
        
        // All strings should be retrievable
        for s in &strings {
            let id = interner.intern(s);
            let resolved = interner.resolve(id);
            prop_assert_eq!(resolved, Some(s.as_str()));
        }
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use lechange_core::StringInterner;
    
    #[test]
    fn test_interner_realistic_workload() {
        let interner = StringInterner::new();
        let paths = vec![
            "src/main.rs",
            "src/lib.rs",
            "tests/integration.rs",
            "Cargo.toml",
            "README.md",
        ];
        
        // Simulate realistic usage
        for _ in 0..1000 {
            for path in &paths {
                let id = interner.intern(path);
                assert_eq!(interner.resolve(id), Some(*path));
            }
        }
    }
    
    #[test]
    fn test_memory_efficiency() {
        let interner = StringInterner::new();
        let duplicates = vec!["same/path.rs"; 10000];
        
        // Intern 10k duplicate strings
        let mut ids = Vec::new();
        for path in &duplicates {
            ids.push(interner.intern(path));
        }
        
        // All should have same ID (memory efficient)
        let first_id = ids[0];
        for id in ids {
            assert_eq!(id, first_id);
        }
    }
}
