#![no_main]
use libfuzzer_sys::fuzz_target;
use lechange_core::platform::PathUtil;
use lechange_core::output::json_format::{escape_json_value, safe_output_escape};

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = PathUtil::to_posix(s);
        let _ = PathUtil::normalize_separator(s);
        let _ = PathUtil::has_separator(s);
        let _ = PathUtil::components(s).count();
        let _ = escape_json_value(s);
        let _ = safe_output_escape(s);
    }
});
