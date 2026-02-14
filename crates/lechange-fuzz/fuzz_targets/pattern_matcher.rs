#![no_main]
use libfuzzer_sys::fuzz_target;
use lechange_core::patterns::matcher::PatternMatcher;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Split input: first half = pattern, second half = path
        let mid = s.len() / 2;
        let (pattern_str, path) = s.split_at(mid);
        if let Ok(matcher) = PatternMatcher::new(&[pattern_str], &[], false) {
            let _ = matcher.matches_sync(path);
        }
    }
});
