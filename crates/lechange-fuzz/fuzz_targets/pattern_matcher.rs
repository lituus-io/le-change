#![no_main]
use lechange_core::patterns::matcher::PatternMatcher;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Split input at a valid char boundary: first half = pattern, second half = path
        let mid = s.len() / 2;
        // Find nearest char boundary at or after mid
        let split_pos = s.ceil_char_boundary(mid);
        if split_pos == 0 || split_pos >= s.len() {
            return;
        }
        let (pattern_str, path) = s.split_at(split_pos);
        if let Ok(matcher) = PatternMatcher::new(&[pattern_str], &[], false) {
            let _ = matcher.matches_sync(path);
        }
    }
});
