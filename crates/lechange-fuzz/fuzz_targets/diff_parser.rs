#![no_main]
use lechange_core::ChangeType;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Fuzz ChangeType::from_byte with arbitrary bytes
    for &byte in data {
        let _ = ChangeType::from_byte(byte);
    }
    // Fuzz diff filter parsing via string
    if let Ok(s) = std::str::from_utf8(data) {
        for ch in s.bytes() {
            let _ = ChangeType::from_byte(ch);
        }
    }
});
