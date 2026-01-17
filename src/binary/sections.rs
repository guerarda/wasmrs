use std::{
    error,
    fmt::{self, Display},
    result,
};

pub mod code;
pub use code::CodeSection;

pub mod custom;
pub use custom::CustomSection;

pub mod data;
pub use data::DataSection;

pub mod data_count;
pub use data_count::{DataCountSection, decode_data_count_section};

pub mod element;
pub use element::ElementSection;

pub mod export;
pub use export::ExportSection;

pub mod function;
pub use function::FunctionSection;

pub mod global;
pub use global::{GlobalSection, GlobalTypeReadError};

pub mod import;
pub use import::ImportSection;

pub mod memory;
pub use memory::{MemTypeReadError, MemorySection};

pub mod start;
pub use start::{StartSection, decode_start_section};

pub mod table;
pub use table::{TableSection, TableTypeReadError};

pub mod type_;
pub use type_::TypeSection;

use crate::{
    binary::{
        reader::{InvalidEnumValueError, ReadError, Reader, VecReadError},
        sections::{
            custom::CustomSectionReadError, data::DataSegmentModeReadError,
            element::ElementSectionReadError,
        },
        types::ConstExpressionReadError,
    },
    instructions::InstructionError,
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
    SectionSize(ReadError),
    SectionSizeMismatch { end: usize, expected: usize },
    EntryCount(ReadError),
    EntrySize(ReadError),

    // Custom Section
    CustomSection(CustomSectionReadError),

    // Type Section
    FuncTypeMarker(ReadError),
    FuncTypeParams(VecReadError<ReadError>),
    FuncTypeResults(VecReadError<ReadError>),

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
    CodeFuncLocal(VecReadError<ReadError>),
    CodeFuncTooManyLocals,
    CodeFuncBody(InstructionError),

    // Element Section
    ElementSection(ElementSectionReadError),

    // Data Section
    DataSegmentMode(DataSegmentModeReadError),
    DataSegment(VecReadError<ReadError>),

    // DataCount Section
    DataCount(ReadError),
}

impl Display for SectionErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SectionSize(_) => write!(f, "section size out of range"),
            Self::SectionSizeMismatch { end, expected } => write!(
                f,
                "section size mismatch, content ended at {a:#0x} ({a}), expected end: {b:#0x} ({b})",
                a = end,
                b = expected
            ),
            Self::EntryCount(_) => write!(f, "reading the entry count"),
            Self::EntrySize(_) => write!(f, "reading this entry size"),

            Self::CustomSection(_) => write!(f, "reading custom section"),

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

            Self::ElementSection(_) => write!(f, "reading the element section"),

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
            Self::SectionSize(e) => Some(e),
            Self::SectionSizeMismatch { .. } => None,
            Self::EntryCount(e) => Some(e),
            Self::EntrySize(e) => Some(e),

            Self::CustomSection(e) => Some(e),

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

            Self::ElementSection(e) => Some(e),

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
    let mut reader = reader.scoped(info.size as u32).map_err(|e| SectionError {
        kind: Box::new(SectionErrorKind::SectionSize(e)),
        info,
        idx: None,
    })?;

    let len: u32 = reader.read().map_err(|e| SectionError {
        kind: Box::new(SectionErrorKind::EntryCount(e)),
        info,
        idx: None,
    })?;

    let entries: std::result::Result<Vec<T>, SectionError> = (0..len)
        .map(|idx| {
            T::decode(&mut reader).map_err(|kind| SectionError {
                kind: Box::new(kind),
                info,
                idx: Some(idx as usize),
            })
        })
        .collect();

    if entries.is_ok() && !reader.is_exhausted() {
        Err(SectionError {
            kind: Box::new(SectionErrorKind::SectionSizeMismatch {
                end: reader.position() as usize,
                expected: info.end as usize,
            }),
            info,
            idx: None,
        })
    } else {
        entries
    }
}
