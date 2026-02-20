#![no_main]

use libfuzzer_sys::fuzz_target;
use lechange_core::interner::StringInterner;

fuzz_target!(|data: &[u8]| {
    // Try to parse as UTF-8
    if let Ok(text) = std::str::from_utf8(data) {
        let interner = StringInterner::new();

        // Test interning arbitrary strings
        let strings: Vec<&str> = text.lines().take(100).collect();

        for s in &strings {
            let id = interner.intern(s);

            // Verify resolve returns the same string
            if let Some(resolved) = interner.resolve(id) {
                assert_eq!(*s, resolved);
            }
        }

        // Test that re-interning gives same ID
        for s in &strings {
            let id1 = interner.intern(s);
            let id2 = interner.intern(s);
            assert_eq!(id1, id2);
        }
    }

    // Test with raw bytes that might not be valid UTF-8
    let interner = StringInterner::new();

    // Convert invalid UTF-8 to lossy string
    let lossy = String::from_utf8_lossy(data);
    let id = interner.intern(&lossy);

    // Should never panic
    let _ = interner.resolve(id);
});
