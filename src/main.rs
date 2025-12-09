use std::fs;

const WASM_MAGIC: [u8; 4] = *b"\0asm";
const WASM_VERSION: [u8; 4] = [0x01, 0x00, 0x00, 0x00];

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <input.wasm>", args[0]);
    }
    let bytes = fs::read(&args[1])?;

    // Read magic number
    if &bytes[0..4] != WASM_MAGIC {
        eprintln!("bad magic")
    }

    // Read version
    if &bytes[4..8] != WASM_VERSION {
        eprintln!("bad version")
    }

    Ok(())
}
