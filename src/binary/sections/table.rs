use crate::binary::{
    reader::{FromReader, ReadError, Reader},
    sections::{SectionEntry, SectionErrorKind},
    types::{Limit, LimitReadError, RefType},
};

/// Table Section
pub type TableSection = Vec<TableType>;

#[derive(Debug, Clone)]
pub struct TableType {
    #[allow(dead_code)]
    pub elemtype: RefType,
    #[allow(dead_code)]
    pub limit: Limit,
}

#[derive(Debug)]
pub enum TableTypeReadError {
    RefType(ReadError),
    Limit(LimitReadError),
}

impl std::error::Error for TableTypeReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::RefType(e) => Some(e),
            Self::Limit(e) => Some(e),
        }
    }
}

impl std::fmt::Display for TableTypeReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RefType(_) => write!(f, "reading its element reference type"),
            Self::Limit(_) => write!(f, "reading its limits"),
        }
    }
}

impl<'a> FromReader<'a> for TableType {
    type Error = TableTypeReadError;

    fn from_reader(reader: &mut Reader<'a>) -> std::result::Result<Self, Self::Error> {
        let elemtype = reader.read().map_err(Self::Error::RefType)?;
        let limit = reader.read().map_err(Self::Error::Limit)?;

        Ok(TableType { elemtype, limit })
    }
}

impl SectionEntry for TableType {
    fn decode(reader: &mut Reader) -> std::result::Result<Self, SectionErrorKind> {
        reader.read().map_err(SectionErrorKind::TableSection)
    }
}
