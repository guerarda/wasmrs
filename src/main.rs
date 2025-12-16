use core::fmt;
use std::{
    fs,
    io::{Error, ErrorKind},
};

mod leb128;
mod reader;
use reader::{FromReader, Reader};

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
    start: u64,
    end: u64,
    size: u32,
}
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
enum ValType {
    I32 = 0x7f,
    I64 = 0x7e,
    F32 = 0x7d,
    F64 = 0x7c,

    V128 = 0x7b,
    // Reference Types
}

impl TryFrom<u8> for ValType {
    type Error = std::io::Error;

    fn try_from(value: u8) -> std::io::Result<Self> {
        match value {
            0x7f => Ok(ValType::I32),
            0x7e => Ok(ValType::I64),
            0x7d => Ok(ValType::F32),
            0x7c => Ok(ValType::F64),
            0x7b => Ok(ValType::V128),
            _ => Err(Error::new(ErrorKind::Other, "bad magic")),
        }
    }
}

impl<'a> FromReader<'a> for ValType {
    fn from_reader(reader: &mut Reader<'a>) -> std::io::Result<Self> {
        reader.read_u8()?.try_into()
    }
}

#[derive(Debug)]
struct FuncType {
    params: Vec<ValType>,
    results: Vec<ValType>,
}

#[derive(Debug)]
struct Section<T> {
    id: SectionId,
    offset: u64,
    end: u64,
    size: u32,
    data: T,
}

type TypeSection = Vec<FuncType>;

impl<'a> FromReader<'a> for FuncType {
    fn from_reader(reader: &mut Reader<'a>) -> std::io::Result<FuncType> {
        let b = reader.read_u8()?;
        if b != 0x60 {
            return Err(Error::new(ErrorKind::Other, "bad byte"));
        }
        Ok(FuncType {
            params: reader.read_vec::<ValType>()?,
            results: reader.read_vec::<ValType>()?,
        })
    }
}

impl<'a> FromReader<'a> for TypeSection {
    fn from_reader(reader: &mut Reader<'a>) -> std::io::Result<TypeSection> {
        reader.read_vec::<FuncType>()
    }
}

struct ModuleReader<'a> {
    reader: Reader<'a>,
}

impl<'a> ModuleReader<'a> {
    fn from_module(module: &'a Module) -> Self {
        ModuleReader {
            reader: Reader::from_bytes(&module.bytes),
        }
    }

    fn read_preamble(&mut self) -> std::io::Result<()> {
        let mut buf = [0u8; 4];

        self.reader.read_exact(&mut buf)?;
        if buf != WASM_MAGIC {
            return Err(Error::new(ErrorKind::Other, "bad magic"));
        }

        self.reader.read_exact(&mut buf)?;
        if buf != WASM_VERSION {
            return Err(Error::new(ErrorKind::Other, "bad version"));
        }
        Ok(())
    }

    fn read_section(&mut self) -> std::io::Result<SectionInfo> {
        let id = self.reader.read_u8()?;
        let size = self.reader.read_u32()?;

        Ok(SectionInfo {
            id: id.try_into().unwrap(), // TODO: Errors
            start: self.reader.position(),
            end: self.reader.position() + (size as u64),
            size,
        })
    }

    fn read_all_sections(&mut self) -> std::io::Result<Vec<SectionInfo>> {
        let mut sections = Vec::new();

        while let Ok(section) = self.read_section() {
            sections.push(section);
        }

        Ok(sections)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <input.wasm>", args[0]);
        std::process::exit(1);
    }
    let bytes = fs::read(&args[1])?;
    let m = Module { bytes };

    let mut r = ModuleReader::from_module(&m);

    r.read_preamble()?;
    let sections = r.read_all_sections()?;

    println!("Sections:");
    for s in &sections {
        println!(
            "{:>15}, start={:#010x}, end={:#010x} (size={:#010x})",
            s.id, s.start, s.end, s.size
        );
    }

    for item in sections.iter() {
        if item.id == SectionId::Function {
            let start = item.start as usize;
            let end = item.end as usize;
            let data = TypeSection::from_reader(&mut Reader::from_bytes(&m.bytes[start..end]));
            dbg!(&data);
        }
    }

    Ok(())
}
