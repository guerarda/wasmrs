use std::{
    error,
    fmt::{self, Display},
    result,
};

use crate::{
    instructions::{Instruction, InstructionError, decode_instruction},
    limits::MAX_WASM_FUNCTION_LOCALS,
    reader::{FromReader, InvalidEnumValueError, ReadError, Reader, Result},
    types::{FuncIdx, FuncType, RefType, TypeIdx, ValType},
};

#[repr(u8)]
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
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

/// Section ids do not always correspond to the order of sections in
/// the encoding of a module.
impl SectionId {
    pub fn order(&self) -> u8 {
        match self {
            SectionId::Custom => 0,
            SectionId::Type => 1,
            SectionId::Import => 2,
            SectionId::Function => 3,
            SectionId::Table => 4,
            SectionId::Memory => 5,
            SectionId::Global => 6,
            SectionId::Export => 7,
            SectionId::Start => 8,
            SectionId::Element => 9,
            SectionId::Code => 11,
            SectionId::Data => 12,
            SectionId::DataCount => 10,
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct SectionInfo {
    pub id: SectionId,
    pub offset: usize,
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
    // Generic
    EntryCount(ReadError),
    EntrySize(ReadError),

    // Type Section
    FuncTypeMarker(ReadError),
    FuncTypeParams(ReadError),
    FuncTypeResults(ReadError),

    // Function Section
    FunctionIndex(ReadError),

    // Table Section
    TableElementRefType(ReadError),
    TableLimit(LimitReadError),

    // Memory Section
    MemoryLimit(LimitReadError),

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
            SectionErrorKind::EntryCount(_) => write!(f, "reading the entry count"),
            SectionErrorKind::EntrySize(_) => write!(f, "reading this entry size"),

            SectionErrorKind::FuncTypeMarker(_) => write!(f, "reading the functype marker"),
            SectionErrorKind::FuncTypeParams(_) => write!(f, "reading the function param types"),
            SectionErrorKind::FuncTypeResults(_) => write!(f, "reading the function result types"),

            SectionErrorKind::FunctionIndex(_) => write!(f, "reading the function type index"),

            SectionErrorKind::TableElementRefType(_) => {
                write!(f, "reading the table type element reference type")
            }
            SectionErrorKind::TableLimit(_) => write!(f, "reading the table type limit"),

            SectionErrorKind::MemoryLimit(_) => write!(f, "reading the memory type limit"),

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
            SectionErrorKind::EntryCount(e) => Some(e),
            SectionErrorKind::EntrySize(e) => Some(e),

            SectionErrorKind::FuncTypeMarker(e) => Some(e),
            SectionErrorKind::FuncTypeParams(e) => Some(e),
            SectionErrorKind::FuncTypeResults(e) => Some(e),

            SectionErrorKind::FunctionIndex(e) => Some(e),

            SectionErrorKind::MemoryLimit(e) => Some(e),

            SectionErrorKind::TableElementRefType(e) => Some(e),
            SectionErrorKind::TableLimit(e) => Some(e),

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

/// Utility for decoding sections that are vector of entries. These
/// are most of the Sections present in a module.
pub trait SectionEntry: Sized {
    fn decode(reader: &mut Reader) -> std::result::Result<Self, SectionErrorKind>;
}

pub fn decode_section<T: SectionEntry>(
    reader: &mut Reader,
    info: SectionInfo,
) -> std::result::Result<Vec<T>, SectionError> {
    let len: u32 = reader.read().map_err(|e| SectionError {
        kind: SectionErrorKind::EntryCount(e),
        info,
        idx: None,
    })?;

    (0..len)
        .map(|idx| {
            T::decode(reader).map_err(|kind| SectionError {
                kind,
                info,
                idx: Some(idx as usize),
            })
        })
        .collect()
}

/// Type Section
pub type TypeSection = Vec<FuncType>;

impl SectionEntry for FuncType {
    fn decode(reader: &mut Reader) -> std::result::Result<Self, SectionErrorKind> {
        let _: u8 = reader
            .expect(0x60) // TODO Enum or const
            .map_err(SectionErrorKind::FuncTypeMarker)?;

        Ok(FuncType {
            params: reader.read().map_err(SectionErrorKind::FuncTypeParams)?,
            results: reader.read().map_err(SectionErrorKind::FuncTypeResults)?,
        })
    }
}

/// Table Section
pub type TableSection = Vec<TableType>;

#[derive(Debug)]
pub struct TableType {
    #[allow(dead_code)]
    pub etype: RefType,
    #[allow(dead_code)]
    pub limit: Limit,
}

impl<'a> FromReader<'a> for RefType {
    fn from_reader(reader: &mut Reader<'a>) -> Result<Self> {
        let pos = reader.position() as usize;
        reader
            .read_u8()?
            .try_into()
            .map_err(|e| ReadError::at_offset(e, pos))
    }
}

impl SectionEntry for TableType {
    fn decode(reader: &mut Reader) -> std::result::Result<Self, SectionErrorKind> {
        let etype = reader
            .read()
            .map_err(SectionErrorKind::TableElementRefType)?;

        let limit = Limit::read(reader).map_err(SectionErrorKind::TableLimit)?;

        Ok(TableType { etype, limit })
    }
}

/// Function Section
pub type FunctionSection = Vec<TypeIdx>;

impl SectionEntry for TypeIdx {
    fn decode(reader: &mut Reader) -> std::result::Result<Self, SectionErrorKind> {
        TypeIdx::from_reader(reader).map_err(SectionErrorKind::FunctionIndex)
    }
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
pub enum LimitReadError {
    Flag(ReadError),
    Min(ReadError),
    Max(ReadError),
}

impl std::error::Error for LimitReadError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            LimitReadError::Flag(e) => Some(e),
            LimitReadError::Min(e) => Some(e),
            LimitReadError::Max(e) => Some(e),
        }
    }
}

impl std::fmt::Display for LimitReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LimitReadError::Flag(_) => write!(f, "reading the limit flag"),
            LimitReadError::Min(_) => write!(f, "reading the limit min"),
            LimitReadError::Max(_) => write!(f, "reading the limit max"),
        }
    }
}

impl Limit {
    fn read(reader: &mut Reader) -> std::result::Result<Self, LimitReadError> {
        let flag: LimitFlag = reader.read().map_err(LimitReadError::Flag)?;
        let min: u32 = reader.read().map_err(LimitReadError::Min)?;

        match flag {
            LimitFlag::Min => Ok(Self { min, max: None }),
            LimitFlag::MinMax => Ok(Self {
                min,
                max: Some(reader.read().map_err(LimitReadError::Max)?),
            }),
        }
    }
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct MemType(pub Limit);

pub type MemorySection = Vec<MemType>;

impl SectionEntry for MemType {
    fn decode(reader: &mut Reader) -> std::result::Result<Self, SectionErrorKind> {
        Limit::read(reader)
            .map_err(SectionErrorKind::MemoryLimit)
            .map(MemType)
    }
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

impl SectionEntry for ExportEntry {
    fn decode(reader: &mut Reader) -> std::result::Result<Self, SectionErrorKind> {
        let name = reader.read_name().map_err(SectionErrorKind::ExportName)?;
        let kind = reader.read().map_err(SectionErrorKind::ExportDescKind)?;

        let index = reader
            .read_u32()
            .map_err(SectionErrorKind::ExportDescIndex)?;

        Ok(ExportEntry { name, kind, index })
    }
}

/// Start Section
#[derive(Debug)]
#[allow(dead_code)]
pub struct StartSection(pub FuncIdx);

pub fn decode_start_section(
    reader: &mut Reader,
    info: SectionInfo,
) -> std::result::Result<StartSection, SectionError> {
    let count: u32 = reader.read().map_err(|e| SectionError {
        kind: SectionErrorKind::DataCount(e),
        info,
        idx: None,
    })?;
    Ok(StartSection(count))
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

impl SectionEntry for CodeEntry {
    fn decode(reader: &mut Reader) -> std::result::Result<Self, SectionErrorKind> {
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
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct DataCountSection(pub u32);

pub fn decode_data_count_section(
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
