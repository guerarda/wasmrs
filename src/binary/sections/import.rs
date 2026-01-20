use core::{error, fmt};

use crate::binary::{
    reader::{FromReader, InvalidEnumValueError, ReadError, Reader},
    sections::{
        GlobalTypeReadError, MemTypeReadError, SectionEntry, SectionErrorKind, TableTypeReadError,
        global::GlobalType, memory::MemType, table::TableType,
    },
    types::TypeIdx,
};

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

    fn try_from(value: u8) -> std::result::Result<Self, Self::Error> {
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

    fn from_reader(reader: &mut Reader<'a>) -> std::result::Result<Self, Self::Error> {
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
        let mod_name: String = reader.read().map_err(ImportSectionReadError::ModuleName)?;
        let name: String = reader.read().map_err(ImportSectionReadError::EntityName)?;

        let itype: ImportDescType = reader.read().map_err(ImportSectionReadError::DescType)?;

        let desc = match itype {
            ImportDescType::Func => reader
                .read()
                .map_err(ImportSectionReadError::DescFunc)
                .map(ImportDesc::Func),

            ImportDescType::Table => reader
                .read()
                .map_err(ImportSectionReadError::DescTable)
                .map(ImportDesc::Table),

            ImportDescType::Mem => reader
                .read()
                .map_err(ImportSectionReadError::DescMem)
                .map(ImportDesc::Mem),

            ImportDescType::Global => reader
                .read()
                .map_err(ImportSectionReadError::DescGlobal)
                .map(ImportDesc::Global),
        }?;

        Ok(ImportEntry {
            mod_name,
            name,
            desc,
        })
    }
}

/// Errors
#[derive(Debug)]
#[non_exhaustive]
pub enum ImportSectionReadError {
    ModuleName(ReadError),
    EntityName(ReadError),
    DescType(ReadError),
    DescFunc(ReadError),
    DescTable(TableTypeReadError),
    DescMem(MemTypeReadError),
    DescGlobal(GlobalTypeReadError),
}

impl error::Error for ImportSectionReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ModuleName(e) => Some(e),
            Self::EntityName(e) => Some(e),
            Self::DescType(e) => Some(e),
            Self::DescFunc(e) => Some(e),
            Self::DescTable(e) => Some(e),
            Self::DescMem(e) => Some(e),
            Self::DescGlobal(e) => Some(e),
        }
    }
}

impl fmt::Display for ImportSectionReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ModuleName(_) => write!(f, "reading import module name"),
            Self::EntityName(_) => write!(f, "reading import entity name"),
            Self::DescType(_) => write!(f, "reading import descriptor type"),
            Self::DescFunc(_) => write!(f, "reading import descriptor: func"),
            Self::DescTable(_) => {
                write!(f, "reading import descriptor: table")
            }
            Self::DescMem(_) => write!(f, "reading import descriptor: mem"),
            Self::DescGlobal(_) => {
                write!(f, "reading import descriptor: global")
            }
        }
    }
}

impl From<ImportSectionReadError> for SectionErrorKind {
    fn from(value: ImportSectionReadError) -> Self {
        SectionErrorKind::ImportSection(value)
    }
}
