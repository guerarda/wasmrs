use std::{error, fmt, result};

use crate::binary::{
    reader::{FromReader, InvalidEnumValueError, ReadError, Reader, Result},
    sections::{SectionEntry, SectionErrorKind},
    types::{ConstExpression, ConstExpressionReadError, ValType},
};

/// Global Section
pub type GlobalSection = Vec<GlobalEntry>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalType {
    pub type_: ValType,
    pub mutflag: MutabilityFlag,
}

#[derive(Debug)]
#[non_exhaustive]
pub enum GlobalTypeReadError {
    Type(ReadError),
    MutabilityFlag(ReadError),
}

impl error::Error for GlobalTypeReadError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Type(e) => Some(e),
            Self::MutabilityFlag(e) => Some(e),
        }
    }
}

impl fmt::Display for GlobalTypeReadError {
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
    pub gt: GlobalType,
    pub body: ConstExpression,
}

impl SectionEntry for GlobalEntry {
    fn decode(reader: &mut Reader) -> std::result::Result<Self, SectionErrorKind> {
        Ok(GlobalEntry {
            gt: reader.read().map_err(GlobalSectionReadError::GlobalType)?,
            body: reader
                .read()
                .map_err(GlobalSectionReadError::GlobalExpression)?,
        })
    }
}

/// Errors
#[derive(Debug)]
#[non_exhaustive]
pub enum GlobalSectionReadError {
    GlobalType(GlobalTypeReadError),
    GlobalExpression(ConstExpressionReadError),
}

impl error::Error for GlobalSectionReadError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::GlobalType(e) => Some(e),
            Self::GlobalExpression(e) => Some(e),
        }
    }
}

impl fmt::Display for GlobalSectionReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GlobalType(_) => write!(f, "reading the global type"),
            Self::GlobalExpression(_) => write!(f, "reading the global expression"),
        }
    }
}

impl From<GlobalSectionReadError> for SectionErrorKind {
    fn from(value: GlobalSectionReadError) -> Self {
        SectionErrorKind::GlobalSection(value)
    }
}
