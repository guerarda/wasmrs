#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let module = match wasmrs::parse_module(data) {
        Ok(module) => module,
        Err(_) => return,
    };
    let _ = wasmrs::validate_module(&module);
});
