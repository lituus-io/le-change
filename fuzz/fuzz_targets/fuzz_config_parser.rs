#![no_main]

use libfuzzer_sys::fuzz_target;
use lechange_core::InputConfig;
use std::borrow::Cow;

fuzz_target!(|data: &[u8]| {
    // Try to parse as UTF-8
    if let Ok(text) = std::str::from_utf8(data) {
        let lines: Vec<&str> = text.lines().collect();

        // Test various config fields with arbitrary input
        if let Some(&base_sha) = lines.first() {
            let config = InputConfig {
                base_sha: Some(Cow::Borrowed(base_sha)),
                ..Default::default()
            };

            // Should not panic when accessing fields
            let _ = config.base_sha.as_ref();
            let _ = config.diff_filter.as_ref();
        }

        // Test with pattern lists
        if lines.len() > 1 {
            let patterns: Vec<Cow<'_, str>> = lines.iter()
                .take(10)
                .map(|s| Cow::Borrowed(*s))
                .collect();

            let config = InputConfig {
                files: Some(patterns),
                ..Default::default()
            };

            // Access pattern fields
            if let Some(ref files) = config.files {
                let _ = files.len();
                let _ = files.iter().count();
            }
        }

        // Test boolean flags with random data
        if data.len() >= 8 {
            let config = InputConfig {
                include_all_old_new_renamed_files: data[0] & 1 == 1,
                write_output_files: data[1] & 1 == 1,
                include_submodules: data[2] & 1 == 1,
                json: data[3] & 1 == 1,
                quotepath: data[4] & 1 == 1,
                negation_patterns_first: data[5] & 1 == 1,
                safe_output: data[6] & 1 == 1,
                escape_json: data[7] & 1 == 1,
                ..Default::default()
            };

            // Should handle all boolean combinations
            let _ = config.include_all_old_new_renamed_files;
            let _ = config.json;
        }

        // Test numeric fields
        if data.len() >= 8 {
            let fetch_depth = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
            let files_ancestor_lookup_depth = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

            let config = InputConfig {
                fetch_depth,
                files_ancestor_lookup_depth,
                ..Default::default()
            };

            let _ = config.fetch_depth;
            let _ = config.files_ancestor_lookup_depth;
        }

        // Test diff_filter field with arbitrary characters
        if let Some(&filter_line) = lines.get(1) {
            let config = InputConfig {
                diff_filter: Cow::Borrowed(filter_line),
                ..Default::default()
            };

            let _ = config.diff_filter.as_ref();
        }
    }

    // Test with completely invalid UTF-8
    let lossy = String::from_utf8_lossy(data);
    let config = InputConfig {
        base_sha: Some(Cow::Owned(lossy.to_string())),
        ..Default::default()
    };

    // Should never panic
    let _ = config.base_sha.as_ref();
});
