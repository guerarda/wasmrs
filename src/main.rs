use core::fmt;
use std::fs;

const WASM_MAGIC: [u8; 4] = *b"\0asm";
const WASM_VERSION: [u8; 4] = [0x01, 0x00, 0x00, 0x00];

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
enum SectionId {
    Custom = 0x00,
    Type = 0x01,
    Import = 0x02,
    Function = 0x03,
    Table = 0x04,
    Memory = 0x05,
    Global = 0x06,
    Export = 0x07,
    Start = 0x08,
    Element = 0x09,
    Code = 0x0a,
    Data = 0x0b,
    DataCount = 0x0c,
}

impl fmt::Display for SectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match *self {
            SectionId::Custom => "Custom(0)",
            SectionId::Type => "Type(1)",
            SectionId::Import => "Import(2)",
            SectionId::Function => "Function(3)",
            SectionId::Table => "Table(4)",
            SectionId::Memory => "Memory(5)",
            SectionId::Global => "Global(6)",
            SectionId::Export => "Export(7)",
            SectionId::Start => "Start(8)",
            SectionId::Element => "Element(9)",
            SectionId::Code => "Code(10)",
            SectionId::Data => "Data(11)",
            SectionId::DataCount => "Data Count(12)",
        };
        f.pad(s)
    }
}

impl TryFrom<u8> for SectionId {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(SectionId::Custom),
            0x01 => Ok(SectionId::Type),
            0x02 => Ok(SectionId::Import),
            0x03 => Ok(SectionId::Function),
            0x04 => Ok(SectionId::Table),
            0x05 => Ok(SectionId::Memory),
            0x06 => Ok(SectionId::Global),
            0x07 => Ok(SectionId::Export),
            0x08 => Ok(SectionId::Start),
            0x09 => Ok(SectionId::Element),
            0x0a => Ok(SectionId::Code),
            0x0b => Ok(SectionId::Data),
            0x0c => Ok(SectionId::DataCount),
            _ => Err("unknown section id"),
        }
    }
}

struct Module {
    bytes: Vec<u8>,
}

#[derive(Debug)]
struct SectionInfo {
    id: SectionId,
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

    if &bytes[4..8] != WASM_VERSION {
        return Err("bad version".into());
    }
    Ok(())
}

fn read_u8(bytes: &[u8], offset: usize) -> u8 {
    bytes[offset]
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    bytes[offset] as u32
}

fn read_section(bytes: &[u8], offset: usize) -> SectionInfo {
    let id = read_u8(bytes, offset);
    let size = read_u32(bytes, offset + 1) as usize;

    SectionInfo {
        id: id.try_into().unwrap(), // TODO: Errors
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
    let m = Module { bytes };

    read_preamble(&m.bytes)?;
    let sections = get_sections(&m.bytes, 8);

    println!("Sections:");
    for s in sections {
        println!(
            "{:>15}, start={:#010x}, end={:#010x} (size={:#010x})",
            s.id, s.start, s.end, s.size
        );
    }

    Ok(())
}
