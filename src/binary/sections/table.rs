use crate::binary::{
    reader::{FromReader, ReadError, ReadErrorKind, Reader},
    sections::{SectionEntry, SectionErrorKind},
    types::{ConstExpression, ConstExpressionReadError, Limit, LimitReadError, RefType},
};

/// Table Section
pub type TableSection = Vec<TableEntry>;

#[derive(Debug)]
pub struct TableEntry {
    pub tabletype: TableType,
    pub expr: Option<ConstExpression>,
}

#[derive(Debug, Clone)]
pub struct TableType {
    pub elemtype: RefType,
    pub limit: Limit,
}

#[derive(Debug)]
pub enum TableReadError {
    Prefix(ReadError),
    RefType(ReadError),
    Limit(LimitReadError),
    Expression(ConstExpressionReadError),
}

impl std::error::Error for TableReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Prefix(e) => Some(e),
            Self::RefType(e) => Some(e),
            Self::Limit(e) => Some(e),
            Self::Expression(e) => Some(e),
        }
    }
}

impl std::fmt::Display for TableReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Prefix(_) => write!(f, "reading its prefix"),
            Self::RefType(_) => write!(f, "reading its element reference type"),
            Self::Limit(_) => write!(f, "reading its limits"),
            Self::Expression(_) => write!(f, "reading its expr"),
        }
    }
}

impl<'a> FromReader<'a> for TableType {
    type Error = TableReadError;

    fn from_reader(reader: &mut Reader<'a>) -> std::result::Result<Self, Self::Error> {
        Ok(TableType {
            elemtype: reader.read().map_err(Self::Error::RefType)?,
            limit: reader.read().map_err(Self::Error::Limit)?,
        })
    }
}

impl<'a> FromReader<'a> for TableEntry {
    type Error = TableReadError;

    fn from_reader(reader: &mut Reader<'a>) -> std::result::Result<Self, Self::Error> {
        let pos = reader.position();
        let b = reader.peek().map_err(Self::Error::Prefix)?;
        if b == 0x40 {
            let _ = reader.read_u8().map_err(Self::Error::Prefix)?;
            let zero = reader.read_u8().map_err(Self::Error::Prefix)?;

            if zero != 0x00 {
                return Err(Self::Error::Prefix(ReadError::at_offset(
                    ReadErrorKind::UnexpectedValue {
                        value: zero.to_string(),
                        expected: "0x00".to_string(),
                    },
                    pos as usize,
                )));
            }
            Ok(TableEntry {
                tabletype: reader.read()?,
                expr: Some(reader.read().map_err(Self::Error::Expression)?),
            })
        } else {
            Ok(TableEntry {
                tabletype: reader.read()?,
                expr: None,
            })
        }
    }
}

impl SectionEntry for TableEntry {
    fn decode(reader: &mut Reader) -> std::result::Result<Self, SectionErrorKind> {
        reader.read().map_err(SectionErrorKind::TableSection)
    }
}
