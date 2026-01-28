#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    use lechange_core::patterns::matcher::PatternMatcher;
    
    // Try to parse as UTF-8
    if let Ok(text) = std::str::from_utf8(data) {
        let patterns: Vec<&str> = text.lines().take(10).collect();
        
        // Try to create pattern matcher
        if let Ok(matcher) = PatternMatcher::new(&patterns, &[], false) {
            // Try to match against some paths
            let test_paths = vec![
                "src/main.rs",
                "tests/test.py", 
                "docs/README.md",
            ];
            
            for path in test_paths {
                let _ = matcher.matches_sync(path);
            }
        }
    }
});
