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
        let mod_name: String = reader
            .read()
            .map_err(ImportSectionReadError::ImportModuleName)?;
        let name: String = reader
            .read()
            .map_err(ImportSectionReadError::ImportEntityName)?;

        let itype: ImportDescType = reader
            .read()
            .map_err(ImportSectionReadError::ImportDescType)?;

        let desc = match itype {
            ImportDescType::Func => reader
                .read()
                .map_err(ImportSectionReadError::ImportDescFunc)
                .map(ImportDesc::Func),

            ImportDescType::Table => reader
                .read()
                .map_err(ImportSectionReadError::ImportDescTable)
                .map(ImportDesc::Table),

            ImportDescType::Mem => reader
                .read()
                .map_err(ImportSectionReadError::ImportDescMem)
                .map(ImportDesc::Mem),

            ImportDescType::Global => reader
                .read()
                .map_err(ImportSectionReadError::ImportDescGlobal)
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
    ImportModuleName(ReadError),
    ImportEntityName(ReadError),
    ImportDescType(ReadError),
    ImportDescFunc(ReadError),
    ImportDescTable(TableTypeReadError),
    ImportDescMem(MemTypeReadError),
    ImportDescGlobal(GlobalTypeReadError),
}

impl error::Error for ImportSectionReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ImportModuleName(e) => Some(e),
            Self::ImportEntityName(e) => Some(e),
            Self::ImportDescType(e) => Some(e),
            Self::ImportDescFunc(e) => Some(e),
            Self::ImportDescTable(e) => Some(e),
            Self::ImportDescMem(e) => Some(e),
            Self::ImportDescGlobal(e) => Some(e),
        }
    }
}

impl fmt::Display for ImportSectionReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
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
        }
    }
}

impl From<ImportSectionReadError> for SectionErrorKind {
    fn from(value: ImportSectionReadError) -> Self {
        SectionErrorKind::ImportSection(value)
    }
}
