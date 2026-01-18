use core::{error, fmt};

use crate::binary::{
    reader::{FromReader, ReadError, ReadErrorKind, Reader, VecReadError},
    sections::{SectionEntry, SectionErrorKind},
    types::FuncType,
};

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
        let _: FuncTypeMarker = reader
            .read()
            .map_err(TypeSectionReadError::FuncTypeMarker)?;

        Ok(FuncType {
            params: reader
                .read()
                .map_err(TypeSectionReadError::FuncTypeParams)?,
            results: reader
                .read()
                .map_err(TypeSectionReadError::FuncTypeResults)?,
        })
    }
}

/// Errors
#[derive(Debug)]
#[non_exhaustive]
pub enum TypeSectionReadError {
    FuncTypeMarker(ReadError),
    FuncTypeParams(VecReadError<ReadError>),
    FuncTypeResults(VecReadError<ReadError>),
}

impl error::Error for TypeSectionReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::FuncTypeMarker(e) => Some(e),
            Self::FuncTypeParams(e) => Some(e),
            Self::FuncTypeResults(e) => Some(e),
        }
    }
}

impl fmt::Display for TypeSectionReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FuncTypeMarker(_) => write!(f, "reading the functype marker"),
            Self::FuncTypeParams(_) => write!(f, "reading the function param types"),
            Self::FuncTypeResults(_) => write!(f, "reading the function result types"),
        }
    }
}

impl From<TypeSectionReadError> for SectionErrorKind {
    fn from(value: TypeSectionReadError) -> Self {
        SectionErrorKind::TypeSection(value)
    }
}
