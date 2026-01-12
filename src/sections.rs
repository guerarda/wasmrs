use std::{
    error,
    fmt::{self, Display},
    result,
};

use crate::{
    instructions::{Instruction, InstructionError, decode_instruction},
    limits::MAX_WASM_FUNCTION_LOCALS,
    reader::{FromReader, InvalidEnumValueError, ReadError, ReadErrorKind, Reader, Result},
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
            Self::Custom => f.pad("Custom(0)"),
            Self::Type => f.pad("Type(1)"),
            Self::Import => f.pad("Import(2)"),
            Self::Function => f.pad("Function(3)"),
            Self::Table => f.pad("Table(4)"),
            Self::Memory => f.pad("Memory(5)"),
            Self::Global => f.pad("Global(6)"),
            Self::Export => f.pad("Export(7)"),
            Self::Start => f.pad("Start(8)"),
            Self::Element => f.pad("Element(9)"),
            Self::Code => f.pad("Code(10)"),
            Self::Data => f.pad("Data(11)"),
            Self::DataCount => f.pad("Data Count(12)"),
        }
    }
}

impl TryFrom<u8> for SectionId {
    type Error = InvalidEnumValueError;

    fn try_from(value: u8) -> result::Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Self::Custom),
            0x01 => Ok(Self::Type),
            0x02 => Ok(Self::Import),
            0x03 => Ok(Self::Function),
            0x04 => Ok(Self::Table),
            0x05 => Ok(Self::Memory),
            0x06 => Ok(Self::Global),
            0x07 => Ok(Self::Export),
            0x08 => Ok(Self::Start),
            0x09 => Ok(Self::Element),
            0x0a => Ok(Self::Code),
            0x0b => Ok(Self::Data),
            0x0c => Ok(Self::DataCount),
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
            Self::Custom => 0,
            Self::Type => 1,
            Self::Import => 2,
            Self::Function => 3,
            Self::Table => 4,
            Self::Memory => 5,
            Self::Global => 6,
            Self::Export => 7,
            Self::Start => 8,
            Self::Element => 9,
            Self::Code => 11,
            Self::Data => 12,
            Self::DataCount => 10,
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
    pub kind: Box<SectionErrorKind>,
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

    // Import Section
    ImportModuleName(ReadError),
    ImportEntityName(ReadError),
    ImportDescType(ReadError),
    ImportDescFunc(ReadError),
    ImportDescTable(TableTypeReadError),
    ImportDescMem(MemTypeReadError),
    ImportDescGlobal(GlobalTypeReadError),

    // Function Section
    FunctionIndex(ReadError),

    // Table Section
    Table(TableTypeReadError),

    // Memory Section
    Memory(MemTypeReadError),

    // Global Section
    GlobalType(GlobalTypeReadError),
    GlobalExpression(ConstExpressionReadError),

    // Export Section
    ExportName(ReadError),
    ExportDescKind(ReadError),
    ExportDescIndex(ReadError),

    // Code Section
    CodeFuncLocal(ReadError),
    CodeFuncTooManyLocals,
    CodeFuncBody(InstructionError),

    // Data Section
    DataSegmentMode(DataSegmentModeReadError),
    DataSegment(ReadError),

    // DataCount Section
    DataCount(ReadError),
}

impl Display for SectionErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EntryCount(_) => write!(f, "reading the entry count"),
            Self::EntrySize(_) => write!(f, "reading this entry size"),

            Self::FuncTypeMarker(_) => write!(f, "reading the functype marker"),
            Self::FuncTypeParams(_) => write!(f, "reading the function param types"),
            Self::FuncTypeResults(_) => write!(f, "reading the function result types"),
            Self::ImportModuleName(_) => write!(f, "reading import module name"),
            Self::ImportEntityName(_) => write!(f, "reading import entity name"),
            Self::ImportDescType(_) => write!(f, "reading import descriptor type"),
            Self::ImportDescFunc(_) => write!(f, "reading import descriptor: func"),
            Self::ImportDescTable(_) => {
                write!(f, "reading import descriptor: table")
            }
            Self::ImportDescMem(_) => write!(f, "reading import descriptor: mem"),
            Self::ImportDescGlobal(_) => {
                write!(f, "reading import descriptor: global")
            }

            Self::FunctionIndex(_) => write!(f, "reading the function type index"),

            Self::Table(_) => write!(f, "reading the table type"),

            Self::Memory(_) => write!(f, "reading the memory type"),

            Self::GlobalType(_) => write!(f, "reading the global type"),
            Self::GlobalExpression(_) => write!(f, "reading the global expression"),

            Self::ExportName(_) => write!(f, "reading the export name"),
            Self::ExportDescKind(_) => write!(f, "reading the export kind"),
            Self::ExportDescIndex(_) => write!(f, "reading the export index"),

            Self::CodeFuncLocal(_) => write!(f, "reading function local"),
            Self::CodeFuncTooManyLocals => write!(f, "checking function locals count"),
            Self::CodeFuncBody(_) => write!(f, "reading function body"),

            Self::DataSegmentMode(_) => write!(f, "reading data segment mode"),
            Self::DataSegment(_) => write!(f, "reading data segment data"),

            Self::DataCount(_) => write!(f, "reading data count"),
        }
    }
}

impl error::Error for SectionErrorKind {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::EntryCount(e) => Some(e),
            Self::EntrySize(e) => Some(e),

            Self::FuncTypeMarker(e) => Some(e),
            Self::FuncTypeParams(e) => Some(e),
            Self::FuncTypeResults(e) => Some(e),

            Self::ImportModuleName(e) => Some(e),
            Self::ImportEntityName(e) => Some(e),
            Self::ImportDescType(e) => Some(e),
            Self::ImportDescFunc(e) => Some(e),
            Self::ImportDescTable(e) => Some(e),
            Self::ImportDescMem(e) => Some(e),
            Self::ImportDescGlobal(e) => Some(e),

            Self::FunctionIndex(e) => Some(e),

            Self::Memory(e) => Some(e),

            Self::Table(e) => Some(e),

            Self::GlobalType(e) => Some(e),
            Self::GlobalExpression(e) => Some(e),

            Self::ExportName(e) => Some(e),
            Self::ExportDescKind(e) => Some(e),
            Self::ExportDescIndex(e) => Some(e),

            Self::CodeFuncBody(e) => Some(e),
            Self::CodeFuncTooManyLocals => None,
            Self::CodeFuncLocal(e) => Some(e),

            Self::DataSegmentMode(e) => Some(e),
            Self::DataSegment(e) => Some(e),

            Self::DataCount(e) => Some(e),
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
        kind: Box::new(SectionErrorKind::EntryCount(e)),
        info,
        idx: None,
    })?;

    (0..len)
        .map(|idx| {
            T::decode(reader).map_err(|kind| SectionError {
                kind: Box::new(kind),
                info,
                idx: Some(idx as usize),
            })
        })
        .collect()
}

/// Type Section
pub type TypeSection = Vec<FuncType>;
struct FuncTypeMarker();

impl<'a> FromReader<'a> for FuncTypeMarker {
    type Error = ReadError;

    fn from_reader(reader: &mut Reader<'a>) -> std::result::Result<Self, Self::Error> {
        let offset = reader.position() as usize;
        let v = reader.read_u8()?;

        if v == 0x60 {
            Ok(FuncTypeMarker())
        } else {
            Err(ReadError {
                offset,
                kind: ReadErrorKind::UnexpectedValue {
                    value: v.to_string(),
                    expected: 0x60.to_string(),
                },
            })
        }
    }
}

impl SectionEntry for FuncType {
    fn decode(reader: &mut Reader) -> std::result::Result<Self, SectionErrorKind> {
        let _: FuncTypeMarker = reader.read().map_err(SectionErrorKind::FuncTypeMarker)?;

        Ok(FuncType {
            params: reader.read().map_err(SectionErrorKind::FuncTypeParams)?,
            results: reader.read().map_err(SectionErrorKind::FuncTypeResults)?,
        })
    }
}

/// Import Section
pub type ImportSection = Vec<ImportEntry>;

#[derive(Debug)]
pub enum ImportDescType {
    Func = 0x00,
    Table = 0x01,
    Mem = 0x02,
    Global = 0x03,
}

impl TryFrom<u8> for ImportDescType {
    type Error = InvalidEnumValueError;

    fn try_from(value: u8) -> result::Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Self::Func),
            0x01 => Ok(Self::Table),
            0x03 => Ok(Self::Mem),
            0x04 => Ok(Self::Global),
            _ => Err(InvalidEnumValueError {
                value,
                enum_name: std::any::type_name::<Self>(),
            }),
        }
    }
}

impl<'a> FromReader<'a> for ImportDescType {
    type Error = ReadError;

    fn from_reader(reader: &mut Reader<'a>) -> Result<Self> {
        let pos = reader.position() as usize;
        reader
            .read_u8()?
            .try_into()
            .map_err(|e| ReadError::at_offset(e, pos))
    }
}

#[derive(Debug)]
pub enum ImportDesc {
    #[allow(dead_code)]
    Func(TypeIdx),
    #[allow(dead_code)]
    Table(TableType),
    #[allow(dead_code)]
    Mem(MemType),
    #[allow(dead_code)]
    Global(GlobalType),
}

#[derive(Debug)]
pub struct ImportEntry {
    #[allow(dead_code)]
    pub mod_name: String,
    #[allow(dead_code)]
    pub name: String,
    #[allow(dead_code)]
    pub desc: ImportDesc,
}

impl SectionEntry for ImportEntry {
    fn decode(reader: &mut Reader) -> std::result::Result<Self, SectionErrorKind> {
        let mod_name: String = reader.read().map_err(SectionErrorKind::ImportModuleName)?;
        let name: String = reader.read().map_err(SectionErrorKind::ImportEntityName)?;

        let itype: ImportDescType = reader.read().map_err(SectionErrorKind::ImportDescType)?;

        let desc = match itype {
            ImportDescType::Func => reader
                .read()
                .map_err(SectionErrorKind::ImportDescFunc)
                .map(ImportDesc::Func),

            ImportDescType::Table => reader
                .read()
                .map_err(SectionErrorKind::ImportDescTable)
                .map(ImportDesc::Table),

            ImportDescType::Mem => reader
                .read()
                .map_err(SectionErrorKind::ImportDescMem)
                .map(ImportDesc::Mem),

            ImportDescType::Global => reader
                .read()
                .map_err(SectionErrorKind::ImportDescGlobal)
                .map(ImportDesc::Global),
        }?;

        Ok(ImportEntry {
            mod_name,
            name,
            desc,
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

#[derive(Debug)]
pub enum TableTypeReadError {
    RefType(ReadError),
    Limit(LimitReadError),
}

impl std::error::Error for TableTypeReadError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::RefType(e) => Some(e),
            Self::Limit(e) => Some(e),
        }
    }
}

impl std::fmt::Display for TableTypeReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RefType(_) => write!(f, "reading its element reference type"),
            Self::Limit(_) => write!(f, "reading its limits"),
        }
    }
}

impl<'a> FromReader<'a> for RefType {
    type Error = ReadError;

    fn from_reader(reader: &mut Reader<'a>) -> Result<Self> {
        let pos = reader.position() as usize;
        reader
            .read_u8()?
            .try_into()
            .map_err(|e| ReadError::at_offset(e, pos))
    }
}

impl<'a> FromReader<'a> for TableType {
    type Error = TableTypeReadError;

    fn from_reader(reader: &mut Reader<'a>) -> std::result::Result<Self, Self::Error> {
        let etype = reader.read().map_err(Self::Error::RefType)?;
        let limit = reader.read().map_err(Self::Error::Limit)?;

        Ok(TableType { etype, limit })
    }
}

impl SectionEntry for TableType {
    fn decode(reader: &mut Reader) -> std::result::Result<Self, SectionErrorKind> {
        reader.read().map_err(SectionErrorKind::Table)
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
    type Error = ReadError;

    fn from_reader(reader: &mut Reader<'a>) -> std::result::Result<Self, Self::Error> {
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
            Self::Flag(e) => Some(e),
            Self::Min(e) => Some(e),
            Self::Max(e) => Some(e),
        }
    }
}

impl std::fmt::Display for LimitReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Flag(_) => write!(f, "reading the limit flag"),
            Self::Min(_) => write!(f, "reading the limit min"),
            Self::Max(_) => write!(f, "reading the limit max"),
        }
    }
}

impl<'a> FromReader<'a> for Limit {
    type Error = LimitReadError;

    fn from_reader(reader: &mut Reader<'a>) -> std::result::Result<Self, Self::Error> {
        let flag: LimitFlag = reader.read().map_err(Self::Error::Flag)?;
        let min: u32 = reader.read().map_err(Self::Error::Min)?;

        match flag {
            LimitFlag::Min => Ok(Self { min, max: None }),
            LimitFlag::MinMax => Ok(Self {
                min,
                max: Some(reader.read().map_err(Self::Error::Max)?),
            }),
        }
    }
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct MemType(pub Limit);

pub type MemorySection = Vec<MemType>;

impl<'a> FromReader<'a> for MemType {
    type Error = MemTypeReadError;

    fn from_reader(reader: &mut Reader<'a>) -> std::result::Result<Self, Self::Error> {
        reader.read().map_err(MemTypeReadError).map(Self)
    }
}

#[derive(Debug)]
pub struct MemTypeReadError(LimitReadError);

impl std::error::Error for MemTypeReadError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        Some(&self.0)
    }
}

impl std::fmt::Display for MemTypeReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "reading its limits")
    }
}

impl SectionEntry for MemType {
    fn decode(reader: &mut Reader) -> std::result::Result<Self, SectionErrorKind> {
        reader.read().map_err(SectionErrorKind::Memory)
    }
}

/// Global Section
pub type GlobalSection = Vec<GlobalEntry>;

#[derive(Debug)]
pub enum MutabilityFlag {
    Const = 0x00,
    Var = 0x01,
}

impl TryFrom<u8> for MutabilityFlag {
    type Error = InvalidEnumValueError;

    fn try_from(value: u8) -> result::Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Self::Const),
            0x01 => Ok(Self::Var),
            _ => Err(InvalidEnumValueError {
                value,
                enum_name: std::any::type_name::<Self>(),
            }),
        }
    }
}

impl<'a> FromReader<'a> for MutabilityFlag {
    type Error = ReadError;

    fn from_reader(reader: &mut Reader<'a>) -> Result<Self> {
        let pos = reader.position() as usize;
        reader
            .read_u8()?
            .try_into()
            .map_err(|e| ReadError::at_offset(e, pos))
    }
}

#[derive(Debug)]
pub struct GlobalType {
    #[allow(dead_code)]
    type_: ValType,
    #[allow(dead_code)]
    mutflag: MutabilityFlag,
}

#[derive(Debug)]
#[non_exhaustive]
pub enum GlobalTypeReadError {
    Type(ReadError),
    MutabilityFlag(ReadError),
}

impl std::error::Error for GlobalTypeReadError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Type(e) => Some(e),
            Self::MutabilityFlag(e) => Some(e),
        }
    }
}

impl std::fmt::Display for GlobalTypeReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Type(_) => write!(f, "reading its value type"),
            Self::MutabilityFlag(_) => write!(f, "reading its mutability"),
        }
    }
}

impl<'a> FromReader<'a> for GlobalType {
    type Error = GlobalTypeReadError;

    fn from_reader(reader: &mut Reader<'a>) -> std::result::Result<Self, Self::Error> {
        let type_ = reader.read().map_err(Self::Error::Type)?;
        let mutflag = reader.read().map_err(Self::Error::MutabilityFlag)?;

        Ok(GlobalType { type_, mutflag })
    }
}

#[derive(Debug)]
pub struct GlobalEntry {
    #[allow(dead_code)]
    gt: GlobalType,
    #[allow(dead_code)]
    body: ConstExpression,
}

impl SectionEntry for GlobalEntry {
    fn decode(reader: &mut Reader) -> std::result::Result<Self, SectionErrorKind> {
        Ok(GlobalEntry {
            gt: reader.read().map_err(SectionErrorKind::GlobalType)?,
            body: reader.read().map_err(SectionErrorKind::GlobalExpression)?,
        })
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
            0x00 => Ok(Self::Func),
            0x01 => Ok(Self::Table),
            0x02 => Ok(Self::Memory),
            0x03 => Ok(Self::Global),
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
    type Error = ReadError;

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
        kind: Box::new(SectionErrorKind::DataCount(e)),
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

/// Data Section
#[derive(Debug)]
pub enum DataSegmentMode {
    Active {
        #[allow(dead_code)]
        mem_index: u32,
        #[allow(dead_code)]
        offset: ConstExpression,
    },
    Passive,
}

#[derive(Debug)]
pub enum DataSegmentModeReadError {
    Flag(ReadError),
    MemIndex(ReadError),
    Expression(ConstExpressionReadError),
}

impl std::error::Error for DataSegmentModeReadError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Flag(e) => Some(e),
            Self::MemIndex(e) => Some(e),
            Self::Expression(e) => Some(e),
        }
    }
}

impl std::fmt::Display for DataSegmentModeReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Flag(_) => write!(f, "reading flag"),
            Self::MemIndex(_) => write!(f, "reading mem index"),
            Self::Expression(_) => write!(f, "reading expression"),
        }
    }
}

impl<'a> FromReader<'a> for DataSegmentMode {
    type Error = DataSegmentModeReadError;

    fn from_reader(reader: &mut Reader<'a>) -> std::result::Result<Self, Self::Error> {
        let offset = reader.position() as usize;
        let flag: u32 = reader.read().map_err(Self::Error::Flag)?;

        match flag {
            0x00 => Ok(DataSegmentMode::Active {
                mem_index: 0,
                offset: reader.read().map_err(Self::Error::Expression)?,
            }),
            0x01 => Ok(DataSegmentMode::Passive),
            0x02 => Ok(DataSegmentMode::Active {
                mem_index: reader.read().map_err(Self::Error::MemIndex)?,
                offset: reader.read().map_err(Self::Error::Expression)?,
            }),
            _ => Err(ReadError {
                offset,
                kind: ReadErrorKind::UnexpectedValue {
                    value: flag.to_string(),
                    expected: "0, 1 or 2".to_string(),
                },
            })
            .map_err(Self::Error::Flag),
        }
    }
}

#[derive(Debug)]
pub struct DataSegment {
    #[allow(dead_code)]
    pub mode: DataSegmentMode,
    #[allow(dead_code)]
    pub data: Vec<u8>,
}

pub type DataSection = Vec<DataSegment>;

impl SectionEntry for DataSegment {
    fn decode(reader: &mut Reader) -> std::result::Result<Self, SectionErrorKind> {
        Ok(Self {
            mode: reader.read().map_err(SectionErrorKind::DataSegmentMode)?,
            data: reader.read().map_err(SectionErrorKind::DataSegment)?,
        })
    }
}

/// Data Count Section
#[derive(Debug)]
#[allow(dead_code)]
pub struct DataCountSection(pub u32);

pub fn decode_data_count_section(
    reader: &mut Reader,
    info: SectionInfo,
) -> std::result::Result<DataCountSection, SectionError> {
    let count: u32 = reader.read().map_err(|e| SectionError {
        kind: Box::new(SectionErrorKind::DataCount(e)),
        info,
        idx: None,
    })?;
    Ok(DataCountSection(count))
}

impl<'a> FromReader<'a> for FuncLocal {
    type Error = ReadError;

    fn from_reader(reader: &mut Reader<'a>) -> Result<Self> {
        Ok(FuncLocal {
            count: reader.read()?,
            valtype: reader.read()?,
        })
    }
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct ConstExpression(Vec<Instruction>);

#[derive(Debug)]
#[non_exhaustive]
pub enum ConstExpressionReadError {
    Instruction(InstructionError),
    MissingEnd,
}

impl std::error::Error for ConstExpressionReadError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Instruction(e) => Some(e),
            _ => None,
        }
    }
}

impl std::fmt::Display for ConstExpressionReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Instruction(_) => write!(f, "reading instruction"),
            Self::MissingEnd => write!(f, "missing end instruction"),
        }
    }
}

impl<'a> FromReader<'a> for ConstExpression {
    type Error = ConstExpressionReadError;

    fn from_reader(reader: &mut Reader<'a>) -> std::result::Result<Self, Self::Error> {
        let mut expr = Vec::<Instruction>::new();

        while !reader.is_exhausted() {
            let instr = decode_instruction(reader).map_err(Self::Error::Instruction)?;
            if instr != Instruction::End {
                expr.push(instr);
            } else {
                expr.push(instr);
                return Ok(Self(expr));
            }
        }
        Err(Self::Error::MissingEnd)
    }
}
