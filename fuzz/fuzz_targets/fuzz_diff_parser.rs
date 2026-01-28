#![no_main]

use libfuzzer_sys::fuzz_target;
use lechange_core::git::diff::DiffParser;
use lechange_core::interner::StringInterner;

fuzz_target!(|data: &[u8]| {
    // Fuzz the diff parser with arbitrary bytes
    let interner = StringInterner::new();
    let parser = DiffParser::new(&interner);
    
    // Try to parse as diff line
    let _ = parser.parse_diff_line(data);
    
    // Should never panic, regardless of input
});
