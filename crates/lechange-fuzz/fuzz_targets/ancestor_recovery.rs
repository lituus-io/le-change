#![no_main]
use lechange_core::patterns::matcher::PatternMatcher;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 3 {
        return;
    }
    // First byte = depth config, rest split between pattern and path
    let depth_byte = data[0];
    let depth = (depth_byte as u32).min(3);

    if let Ok(s) = std::str::from_utf8(&data[1..]) {
        let mid = s.len() / 2;
        // Find nearest char boundary at or after mid
        let split_pos = s.ceil_char_boundary(mid);
        if split_pos == 0 || split_pos >= s.len() {
            return;
        }
        let (pattern_str, file_path) = s.split_at(split_pos);

        if let Ok(matcher) = PatternMatcher::new(&[pattern_str], &[], false) {
            // Test matching on the file path itself
            let _ = matcher.matches_sync(file_path);

            // Simulate ancestor directory scanning: walk parent dirs
            let path = std::path::Path::new(file_path);
            let mut current = path.parent();
            for _ in 0..depth {
                if let Some(dir) = current {
                    if let Some(dir_str) = dir.to_str() {
                        let test_path = format!("{}/test.yaml", dir_str);
                        let _ = matcher.matches_sync(&test_path);
                    }
                    current = dir.parent();
                } else {
                    break;
                }
            }
        }
    }
});
