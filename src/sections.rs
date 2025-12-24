use std::{
    error,
    fmt::{self, Display},
    result,
};

use crate::{
    instructions::{decode_instruction, Instruction, InstructionError},
    reader::{FromReader, InvalidEnumValueError, ReadError, Reader, Result},
    types::{FuncType, TypeIdx, ValType},
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

#[derive(Debug, Copy, Clone)]
pub struct SectionInfo {
    pub id: SectionId,
    pub start: u64,
    pub end: u64,
    pub size: u32,
}

#[derive(Debug)]
#[non_exhaustive]
pub struct SectionError {
    kind: SectionErrorKind,
    info: SectionInfo,
    idx: Option<usize>,
}

impl Display for SectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(idx) = self.idx {
            write!(f, "reading section {}, entries[{}]", self.info.id, idx)
        } else {
            write!(f, "reading section {}", self.info.id)
        }
    }
}

impl error::Error for SectionError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        Some(&self.kind)
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum SectionErrorKind {
    // Generic
    EntryCount(ReadError),
    EntrySize(ReadError),

    // Type Section
    FuncTypeMarker(ReadError),
    FuncTypeParams(ReadError),
    FuncTypeResults(ReadError),

    // Function Section
    FunctionIndex(ReadError),

    // Export Section
    ExportName(ReadError),
    ExportDescKind(ReadError),
    ExportDescIndex(ReadError),

    // Code Section
    CodeFuncLocal(ReadError),
    CodeFuncBody(InstructionError),
}

impl Display for SectionErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SectionErrorKind::EntryCount(_) => write!(f, "reading the entry count"),
            SectionErrorKind::EntrySize(_) => write!(f, "reading this entry size"),

            SectionErrorKind::FuncTypeMarker(_) => write!(f, "reading the functype marker"),
            SectionErrorKind::FuncTypeParams(_) => write!(f, "reading the function param types"),
            SectionErrorKind::FuncTypeResults(_) => write!(f, "reading the function result types"),

            SectionErrorKind::FunctionIndex(_) => write!(f, "reading the function type index"),

            SectionErrorKind::ExportName(_) => write!(f, "reading the export name"),
            SectionErrorKind::ExportDescKind(_) => write!(f, "reading the export kind"),
            SectionErrorKind::ExportDescIndex(_) => write!(f, "reading the export index"),

            SectionErrorKind::CodeFuncLocal(_) => write!(f, "reading function local"),
            SectionErrorKind::CodeFuncBody(_) => write!(f, "reading function body"),
        }
    }
}

impl error::Error for SectionErrorKind {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            SectionErrorKind::EntryCount(e) => Some(e),
            SectionErrorKind::EntrySize(e) => Some(e),

            SectionErrorKind::FuncTypeMarker(e) => Some(e),
            SectionErrorKind::FuncTypeParams(e) => Some(e),
            SectionErrorKind::FuncTypeResults(e) => Some(e),

            SectionErrorKind::FunctionIndex(e) => Some(e),

            SectionErrorKind::ExportName(e) => Some(e),
            SectionErrorKind::ExportDescKind(e) => Some(e),
            SectionErrorKind::ExportDescIndex(e) => Some(e),

            SectionErrorKind::CodeFuncBody(e) => Some(e),
            SectionErrorKind::CodeFuncLocal(e) => Some(e),
        }
    }
}

impl From<InstructionError> for SectionErrorKind {
    fn from(value: InstructionError) -> Self {
        SectionErrorKind::CodeFuncBody(value)
    }
}

/// Type Section
pub type TypeSection = Vec<FuncType>;

fn read_type_entry(reader: &mut Reader) -> result::Result<FuncType, SectionErrorKind> {
    let _: u8 = reader
        .expect(0x60) // TODO Enum or const
        .map_err(SectionErrorKind::FuncTypeMarker)?;

    Ok(FuncType {
        params: reader.read().map_err(SectionErrorKind::FuncTypeParams)?,
        results: reader.read().map_err(SectionErrorKind::FuncTypeResults)?,
    })
}

pub fn read_type_section(
    reader: &mut Reader,
    info: SectionInfo,
) -> std::result::Result<TypeSection, SectionError> {
    let len: u32 = reader.read().map_err(|e| SectionError {
        kind: SectionErrorKind::EntryCount(e),
        info,
        idx: None,
    })?;

    (0..len)
        .map(|idx| {
            read_type_entry(reader).map_err(|kind| SectionError {
                kind,
                info,
                idx: Some(idx as usize),
            })
        })
        .collect()
}

/// Function Section
pub type FunctionSection = Vec<TypeIdx>;

pub fn read_function_section(
    reader: &mut Reader,
    info: SectionInfo,
) -> std::result::Result<FunctionSection, SectionError> {
    let len: u32 = reader.read().map_err(|e| SectionError {
        kind: SectionErrorKind::EntryCount(e),
        info,
        idx: None,
    })?;

    (0..len)
        .map(|idx| {
            TypeIdx::from_reader(reader).map_err(|e| SectionError {
                kind: SectionErrorKind::FunctionIndex(e),
                info,
                idx: Some(idx as usize),
            })
        })
        .collect()
}

/// Export Section
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExportKind {
    Func = 0x00,
    Table = 0x01,
    Memory = 0x02,
    Global = 0x03,
}

impl TryFrom<u8> for ExportKind {
    type Error = InvalidEnumValueError;

    fn try_from(value: u8) -> std::result::Result<Self, Self::Error> {
        match value {
            0x00 => Ok(ExportKind::Func),
            0x01 => Ok(ExportKind::Table),
            0x02 => Ok(ExportKind::Memory),
            0x03 => Ok(ExportKind::Global),
            _ => Err(InvalidEnumValueError {
                value,
                enum_name: std::any::type_name::<ExportKind>(),
            }),
        }
    }
}

#[derive(Debug)]
pub struct Export {
    pub name: String,
    pub kind: ExportKind,
    pub index: u32,
}

pub type ExportSection = Vec<Export>;

impl<'a> FromReader<'a> for ExportKind {
    fn from_reader(reader: &mut Reader<'a>) -> Result<Self> {
        let pos = reader.position() as usize;
        reader
            .read_u8()?
            .try_into()
            .map_err(|e| ReadError::at_offset(e, pos))
    }
}

fn read_export_entry(reader: &mut Reader) -> result::Result<Export, SectionErrorKind> {
    let name = reader.read_name().map_err(SectionErrorKind::ExportName)?;
    let kind = reader.read().map_err(SectionErrorKind::ExportDescKind)?;

    let index = reader
        .read_u32()
        .map_err(SectionErrorKind::ExportDescIndex)?;

    Ok(Export { name, kind, index })
}

pub fn read_export_section(
    reader: &mut Reader,
    info: SectionInfo,
) -> std::result::Result<ExportSection, SectionError> {
    let len: u32 = reader.read().map_err(|e| SectionError {
        kind: SectionErrorKind::EntryCount(e),
        info,
        idx: None,
    })?;

    (0..len)
        .map(|idx| {
            read_export_entry(reader).map_err(|kind| SectionError {
                kind,
                info,
                idx: Some(idx as usize),
            })
        })
        .collect()
}

/// Code Section
#[derive(Debug)]
pub struct FuncLocal {
    pub count: u32,
    pub valtype: ValType,
}

#[derive(Debug)]
pub struct CodeEntry {
    pub size: usize,
    pub locals: Vec<FuncLocal>,
    pub body: Vec<Instruction>,
}

pub type CodeSection = Vec<CodeEntry>;

fn read_code_entry(reader: &mut Reader) -> result::Result<CodeEntry, SectionErrorKind> {
    let size = reader.read_u32().map_err(SectionErrorKind::EntrySize)?;
    let mut reader = reader.scoped(size).map_err(SectionErrorKind::EntrySize)?;

    let locals: Vec<FuncLocal> = reader.read().map_err(SectionErrorKind::CodeFuncLocal)?;

    let mut body = Vec::new();
    while !reader.is_exhausted() {
        let instr = decode_instruction(&mut reader)?;
        body.push(instr);
    }

    Ok(CodeEntry {
        size: size as usize,
        locals,
        body,
    })
}

pub fn read_code_section(
    reader: &mut Reader,
    info: SectionInfo,
) -> std::result::Result<CodeSection, SectionError> {
    let len: u32 = reader.read().map_err(|e| SectionError {
        kind: SectionErrorKind::EntryCount(e),
        info,
        idx: None,
    })?;

    (0..len)
        .map(|idx| {
            read_code_entry(reader).map_err(|kind| SectionError {
                kind,
                info,
                idx: Some(idx as usize),
            })
        })
        .collect()
}

impl<'a> FromReader<'a> for FuncLocal {
    fn from_reader(reader: &mut Reader<'a>) -> Result<Self> {
        Ok(FuncLocal {
            count: reader.read()?,
            valtype: reader.read()?,
        })
    }
}
