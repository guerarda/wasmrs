use std::fmt;

use crate::{
    reader::{FromReader, ReadError, Reader, Result},
    types::{Export, FuncType, TypeIdx},
};

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SectionId {
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
    Unknown(u8),
}

impl fmt::Display for SectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            SectionId::Custom => f.pad("Custom(0)"),
            SectionId::Type => f.pad("Type(1)"),
            SectionId::Import => f.pad("Import(2)"),
            SectionId::Function => f.pad("Function(3)"),
            SectionId::Table => f.pad("Table(4)"),
            SectionId::Memory => f.pad("Memory(5)"),
            SectionId::Global => f.pad("Global(6)"),
            SectionId::Export => f.pad("Export(7)"),
            SectionId::Start => f.pad("Start(8)"),
            SectionId::Element => f.pad("Element(9)"),
            SectionId::Code => f.pad("Code(10)"),
            SectionId::Data => f.pad("Data(11)"),
            SectionId::DataCount => f.pad("Data Count(12)"),
            SectionId::Unknown(v) => {
                let s = format!("Unknown({})", v);
                f.pad(&s)
            }
        }
    }
}

impl From<u8> for SectionId {
    fn from(value: u8) -> Self {
        match value {
            0x00 => SectionId::Custom,
            0x01 => SectionId::Type,
            0x02 => SectionId::Import,
            0x03 => SectionId::Function,
            0x04 => SectionId::Table,
            0x05 => SectionId::Memory,
            0x06 => SectionId::Global,
            0x07 => SectionId::Export,
            0x08 => SectionId::Start,
            0x09 => SectionId::Element,
            0x0a => SectionId::Code,
            0x0b => SectionId::Data,
            0x0c => SectionId::DataCount,
            v => SectionId::Unknown(v),
        }
    }
}

/// Type Section
pub type TypeSection = Vec<FuncType>;

// impl<'a> FromReader<'a> for TypeSection {
//     fn from_reader(reader: &mut Reader<'a>) -> Result<Self> {
//         reader.read_vec::<FuncType>()
//     }
// }

/// Function Section
pub type FunctionSection = Vec<TypeIdx>;

/// Export Section
pub type ExportSection = Vec<Export>;

impl<'a> FromReader<'a> for Export {
    fn from_reader(reader: &mut Reader<'a>) -> Result<Self> {
        let offset = reader.position() as usize;
        Ok(Export {
            name: reader.read_name()?,
            kind: reader
                .read_u8()?
                .try_into()
                .map_err(|e| ReadError::at_offset(e, offset))?,
            index: reader.read_u32()?,
        })
    }
}
