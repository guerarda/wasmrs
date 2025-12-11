use std::fs;

const WASM_MAGIC: [u8; 4] = *b"\0asm";
const WASM_VERSION: [u8; 4] = [0x01, 0x00, 0x00, 0x00];

#[derive(Debug)]
struct SectionInfo {
    id: u32,
    start: usize,
    end: usize,
    size: usize,
}

fn read_preamble(bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    if bytes.len() < 8 {
        return Err("file too short".into());
    }
    if &bytes[0..4] != WASM_MAGIC {
        return Err("bad magic".into());
    }

    // Read version
    if &bytes[4..8] != WASM_VERSION {
        return Err("bad version".into());
    }
    Ok(())
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    bytes[offset] as u32
}

fn read_section(bytes: &[u8], offset: usize) -> SectionInfo {
    let id = read_u32(bytes, offset);
    let size = read_u32(bytes, offset + 1) as usize;

    SectionInfo {
        id,
        start: offset + 2,
        end: offset + size + 2,
        size,
    }
}

fn get_sections(bytes: &[u8], mut offset: usize) -> Vec<SectionInfo> {
    let mut sections = Vec::new();

    while offset < bytes.len() {
        let section = read_section(bytes, offset);
        offset += section.size + 2; // id + size
        sections.push(section);
    }

    sections
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <input.wasm>", args[0]);
        std::process::exit(1);
    }
    let bytes = fs::read(&args[1])?;
    read_preamble(&bytes)?;
    let sections = get_sections(&bytes, 8);

    println!("Sections:");
    for s in sections {
        println!(
            "{:>10}, start={:#010x}, end={:#010x}, (size={:#010x})",
            s.id, s.start, s.end, s.size
        );
    }

    Ok(())
}
