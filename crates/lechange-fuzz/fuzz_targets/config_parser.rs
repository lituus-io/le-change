#![no_main]
use lechange_core::patterns::loader::PatternLoader;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Fuzz YAML pattern loading
        let _ = PatternLoader::load_yaml_groups(s, true);
        let _ = PatternLoader::load_yaml_groups(s, false);
    }
});
