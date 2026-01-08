use std::{
    error,
    fmt::{self, Display},
    result,
};

use crate::{
    instructions::{Instruction, InstructionError, decode_instruction},
    limits::MAX_WASM_FUNCTION_LOCALS,
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

impl TryFrom<u8> for SectionId {
    type Error = InvalidEnumValueError;

    fn try_from(value: u8) -> result::Result<Self, Self::Error> {
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
            _ => Err(InvalidEnumValueError {
                value,
                enum_name: std::any::type_name::<Self>(),
            }),
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
    pub kind: SectionErrorKind,
    pub info: SectionInfo,
    pub idx: Option<usize>,
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
    // Preamble errors (before sections)
    Preamble(ReadError),
    Toc(ReadError),

    // Generic
    EntryCount(ReadError),
    EntrySize(ReadError),

    // Type Section
    FuncTypeMarker(ReadError),
    FuncTypeParams(ReadError),
    FuncTypeResults(ReadError),

    // Function Section
    FunctionIndex(ReadError),

    // Memory Section
    MemoryLimitFlag(ReadError),
    MemoryLimitMin(ReadError),
    MemoryLimitMax(ReadError),

    // Export Section
    ExportName(ReadError),
    ExportDescKind(ReadError),
    ExportDescIndex(ReadError),

    // Code Section
    CodeFuncLocal(ReadError),
    CodeFuncTooManyLocals,
    CodeFuncBody(InstructionError),

    // DataCount Section
    DataCount(ReadError),
}

impl Display for SectionErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SectionErrorKind::Preamble(_) => write!(f, "reading module preamble"),
            SectionErrorKind::Toc(_) => write!(f, "reading section table of contents"),

            SectionErrorKind::EntryCount(_) => write!(f, "reading the entry count"),
            SectionErrorKind::EntrySize(_) => write!(f, "reading this entry size"),

            SectionErrorKind::FuncTypeMarker(_) => write!(f, "reading the functype marker"),
            SectionErrorKind::FuncTypeParams(_) => write!(f, "reading the function param types"),
            SectionErrorKind::FuncTypeResults(_) => write!(f, "reading the function result types"),

            SectionErrorKind::FunctionIndex(_) => write!(f, "reading the function type index"),

            SectionErrorKind::MemoryLimitFlag(_) => write!(f, "reading the memory type limit flag"),
            SectionErrorKind::MemoryLimitMin(_) => write!(f, "reading the memory type limit min"),
            SectionErrorKind::MemoryLimitMax(_) => write!(f, "reading the memory type limit max"),

            SectionErrorKind::ExportName(_) => write!(f, "reading the export name"),
            SectionErrorKind::ExportDescKind(_) => write!(f, "reading the export kind"),
            SectionErrorKind::ExportDescIndex(_) => write!(f, "reading the export index"),

            SectionErrorKind::CodeFuncLocal(_) => write!(f, "reading function local"),
            SectionErrorKind::CodeFuncTooManyLocals => write!(f, "checking function locals count"),
            SectionErrorKind::CodeFuncBody(_) => write!(f, "reading function body"),

            SectionErrorKind::DataCount(_) => write!(f, "reading data count"),
        }
    }
}

impl error::Error for SectionErrorKind {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            SectionErrorKind::Preamble(e) => Some(e),
            SectionErrorKind::Toc(e) => Some(e),

            SectionErrorKind::EntryCount(e) => Some(e),
            SectionErrorKind::EntrySize(e) => Some(e),

            SectionErrorKind::FuncTypeMarker(e) => Some(e),
            SectionErrorKind::FuncTypeParams(e) => Some(e),
            SectionErrorKind::FuncTypeResults(e) => Some(e),

            SectionErrorKind::FunctionIndex(e) => Some(e),

            SectionErrorKind::MemoryLimitFlag(e) => Some(e),
            SectionErrorKind::MemoryLimitMin(e) => Some(e),
            SectionErrorKind::MemoryLimitMax(e) => Some(e),

            SectionErrorKind::ExportName(e) => Some(e),
            SectionErrorKind::ExportDescKind(e) => Some(e),
            SectionErrorKind::ExportDescIndex(e) => Some(e),

            SectionErrorKind::CodeFuncBody(e) => Some(e),
            SectionErrorKind::CodeFuncTooManyLocals => None,
            SectionErrorKind::CodeFuncLocal(e) => Some(e),

            SectionErrorKind::DataCount(e) => Some(e),
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

/// Memory Section
#[repr(u8)]
#[derive(Debug, Eq, PartialEq)]
pub enum LimitFlag {
    Min = 0x00,
    MinMax = 0x01,
}

impl TryFrom<u8> for LimitFlag {
    type Error = InvalidEnumValueError;

    fn try_from(value: u8) -> std::result::Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Self::Min),
            0x01 => Ok(Self::MinMax),
            _ => Err(InvalidEnumValueError {
                value,
                enum_name: std::any::type_name::<Self>(),
            }),
        }
    }
}

impl<'a> FromReader<'a> for LimitFlag {
    fn from_reader(reader: &mut Reader<'a>) -> Result<Self> {
        let pos = reader.position() as usize;
        reader
            .read_u8()?
            .try_into()
            .map_err(|e| ReadError::at_offset(e, pos))
    }
}

#[derive(Debug)]
pub struct Limit {
    #[allow(dead_code)]
    pub min: u32,
    #[allow(dead_code)]
    pub max: Option<u32>,
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct MemType(pub Limit);

pub type MemorySection = Vec<MemType>;

fn read_memory_entry(reader: &mut Reader) -> std::result::Result<MemType, SectionErrorKind> {
    let flag: LimitFlag = reader.read().map_err(SectionErrorKind::MemoryLimitFlag)?;
    let min: u32 = reader.read().map_err(SectionErrorKind::MemoryLimitMin)?;

    match flag {
        LimitFlag::Min => Ok(MemType(Limit { min, max: None })),
        LimitFlag::MinMax => Ok(MemType(Limit {
            min,
            max: Some(reader.read().map_err(SectionErrorKind::MemoryLimitMax)?),
        })),
    }
}

pub fn read_memory_section(
    reader: &mut Reader,
    info: SectionInfo,
) -> std::result::Result<MemorySection, SectionError> {
    let len: u32 = reader.read().map_err(|e| SectionError {
        kind: SectionErrorKind::EntryCount(e),
        info,
        idx: None,
    })?;

    (0..len)
        .map(|idx| {
            read_memory_entry(reader).map_err(|kind| SectionError {
                kind,
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
                enum_name: std::any::type_name::<Self>(),
            }),
        }
    }
}

#[derive(Debug)]
pub struct ExportEntry {
    pub name: String,
    #[allow(dead_code)]
    pub kind: ExportKind,
    pub index: u32,
}

pub type ExportSection = Vec<ExportEntry>;

impl<'a> FromReader<'a> for ExportKind {
    fn from_reader(reader: &mut Reader<'a>) -> Result<Self> {
        let pos = reader.position() as usize;
        reader
            .read_u8()?
            .try_into()
            .map_err(|e| ReadError::at_offset(e, pos))
    }
}

fn read_export_entry(reader: &mut Reader) -> result::Result<ExportEntry, SectionErrorKind> {
    let name = reader.read_name().map_err(SectionErrorKind::ExportName)?;
    let kind = reader.read().map_err(SectionErrorKind::ExportDescKind)?;

    let index = reader
        .read_u32()
        .map_err(SectionErrorKind::ExportDescIndex)?;

    Ok(ExportEntry { name, kind, index })
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
    #[allow(dead_code)]
    pub size: usize,
    pub locals: Vec<FuncLocal>,
    pub body: Vec<Instruction>,
}

pub type CodeSection = Vec<CodeEntry>;

fn read_code_entry(reader: &mut Reader) -> result::Result<CodeEntry, SectionErrorKind> {
    let size = reader.read_u32().map_err(SectionErrorKind::EntrySize)?;
    let mut reader = reader.scoped(size).map_err(SectionErrorKind::EntrySize)?;

    let locals: Vec<FuncLocal> = reader.read().map_err(SectionErrorKind::CodeFuncLocal)?;

    // There's a limit for number of functions locals
    // TODO It should consider function parameters as implicit locals
    let _ = locals
        .iter()
        .try_fold(0u32, |acc, &FuncLocal { count, .. }| acc.checked_add(count))
        .filter(|&n| n <= MAX_WASM_FUNCTION_LOCALS)
        .ok_or(SectionErrorKind::CodeFuncTooManyLocals)?;

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

#[derive(Debug)]
#[allow(dead_code)]
pub struct DataCountSection(pub u32);

pub fn read_data_count_section(
    reader: &mut Reader,
    info: SectionInfo,
) -> std::result::Result<DataCountSection, SectionError> {
    let count: u32 = reader.read().map_err(|e| SectionError {
        kind: SectionErrorKind::DataCount(e),
        info,
        idx: None,
    })?;
    Ok(DataCountSection(count))
}

impl<'a> FromReader<'a> for FuncLocal {
    fn from_reader(reader: &mut Reader<'a>) -> Result<Self> {
        Ok(FuncLocal {
            count: reader.read()?,
            valtype: reader.read()?,
        })
    }
}
