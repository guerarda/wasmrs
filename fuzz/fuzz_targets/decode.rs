#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = match wasmrs::parse_module(data) {
        Ok(module) => module,
        Err(_) => return,
    };
});
