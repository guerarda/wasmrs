// From https://webassembly.github.io/spec/js-api/#limits

pub const MAX_WASM_FUNCTION_LOCALS: u32 = 50_000;
pub const MAX_WASM_32BIT_MEMORY_PAGES: u32 = 65_536;

pub const MAX_STACK_DEPTH: usize = 1024;
pub const MAX_TABLE_SIZE: u32 = u32::MAX;
