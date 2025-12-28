use std::{
    fs,
    io::{Seek, SeekFrom},
};

mod leb128;
mod reader;

use crate::{
    reader::ReadErrorKind,
    sections::{
        read_code_section, read_export_section, read_function_section, read_type_section,
        CodeSection, ExportSection, FunctionSection, SectionId, TypeSection,
    },
};
use reader::{ReadError, Reader, Result};

mod sections;
use crate::sections::SectionInfo;

mod instructions;
mod types;

const WASM_MAGIC: [u8; 4] = *b"\0asm";
const WASM_VERSION: [u8; 4] = [0x01, 0x00, 0x00, 0x00];

struct Module {
    bytes: Vec<u8>,
    sections: Vec<SectionInfo>,

    types: Option<TypeSection>,
    functions: Option<FunctionSection>,
    exports: Option<ExportSection>,
    codes: Option<CodeSection>,
}

impl Module {
    fn from_bytes(bytes: Vec<u8>) -> Self {
        Module {
            bytes,
            sections: Vec::new(),

            types: None,
            functions: None,
            exports: None,
            codes: None,
        }
    }
}

struct ModuleReader<'a> {
    reader: Reader<'a>,
}

impl<'a> ModuleReader<'a> {
    fn from_module(module: &'a Module) -> Self {
        ModuleReader {
            reader: Reader::from_bytes(&module.bytes, 0),
        }
    }

    fn read_preamble(&mut self) -> Result<()> {
        let mut buf = [0u8; 4];

        self.reader.read_exact(&mut buf)?;
        if buf != WASM_MAGIC {
            return Err(ReadError::at_offset(ReadErrorKind::BadMagic, 0));
        }

        self.reader.read_exact(&mut buf)?;
        if buf != WASM_VERSION {
            return Err(ReadError::at_offset(ReadErrorKind::BadVersion, 4));
        }
        Ok(())
    }

    fn read_toc(&mut self) -> Result<Vec<SectionInfo>> {
        let mut v = Vec::new();
        while self.reader.has_data_left()? {
            let id: SectionId = self.reader.read_u8()?.into();
            let size = self.reader.read_u32()?;

            let info = SectionInfo {
                id,
                start: self.reader.position(),
                end: self
                    .reader
                    .cursor
                    .seek(SeekFrom::Current(size as i64))
                    .unwrap(),
                size,
            };
            v.push(info);
        }
        Ok(v)
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <input.wasm>", args[0]);
        std::process::exit(1);
    }
    let bytes = fs::read(&args[1])?;
    let mut m = Module::from_bytes(bytes);

    let mut r = ModuleReader::from_module(&m);

    r.read_preamble()?;
    m.sections = r.read_toc()?;

    println!("Sections:");
    for s in &m.sections {
        println!(
            "{:>15}, start={:#010x}, end={:#010x} (size={:#010x})",
            s.id, s.start, s.end, s.size
        );
    }

    for item in m.sections.iter() {
        let start = item.start as usize;
        let end = item.end as usize;

        let mut reader = Reader::from_bytes(&m.bytes[..end], start);

        match item.id {
            SectionId::Custom => todo!(),
            SectionId::Type => {
                m.types = Some(read_type_section(&mut reader, *item)?);
            }
            SectionId::Import => todo!(),
            SectionId::Function => {
                m.functions = Some(read_function_section(&mut reader, *item)?);
            }
            SectionId::Table => todo!(),
            SectionId::Memory => todo!(),
            SectionId::Global => todo!(),
            SectionId::Export => {
                m.exports = Some(read_export_section(&mut reader, *item)?);
            }
            SectionId::Start => todo!(),
            SectionId::Element => todo!(),
            SectionId::Code => {
                m.codes = Some(read_code_section(&mut reader, *item)?);
            }
            SectionId::Data => todo!(),
            SectionId::DataCount => todo!(),
            SectionId::Unknown(_) => todo!(),
        };
    }

    Ok(())
}
