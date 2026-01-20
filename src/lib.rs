use binary::module::{self, Module};
use binary::reader::ReadError;
use binary::sections::{SectionError, SectionId, SectionInfo};

use std::result;

mod binary;
mod instructions;
mod limits;
pub mod runtime;

#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    Malformed(MalformedError),
    Invalid,
    Trap,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Malformed(e) => write!(f, "malformed module: {}", e),
            Error::Invalid => write!(f, "invalid module"),
            Error::Trap => write!(f, "trap"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Malformed(e) => Some(e),
            _ => None,
        }
    }
}

impl From<MalformedError> for Error {
    fn from(value: MalformedError) -> Self {
        Error::Malformed(value)
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum MalformedError {
    Read(ReadError),
    Preamble(ReadError),

    DuplicateSection {
        offset: usize,
        id: SectionId,
        other: SectionInfo,
    },
    SectionOrder {
        offset: usize,
        id: SectionId,
        other: SectionInfo,
    },
    Section(SectionError),
    InconsistentLength {
        section: SectionId,
        other: SectionId,
    },
}

impl std::fmt::Display for MalformedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read(_) => write!(f, "Malformed module"),
            Self::Preamble(_) => write!(f, "invalid module preamble"),
            Self::DuplicateSection { offset, id, other } => {
                write!(
                    f,
                    "duplicate section: {id} section at offset {offset:#0x} ({offset}), previously seen at offset {other_offset:#0x} ({other_offset})",
                    id = id,
                    offset = offset,
                    other_offset = other.offset
                )
            }
            Self::SectionOrder { offset, id, other } => {
                write!(
                    f,
                    "section out of order: {id} section at offset {offset:#0x} ({offset}), appears after {other_id} at offset {other_offset:#0x} ({other_offset})",
                    id = id,
                    offset = offset,
                    other_id = other.id,
                    other_offset = other.offset
                )
            }
            Self::Section(_) => write!(f, "malformed section"),
            Self::InconsistentLength { section, other } => {
                write!(f, "inconsistent section lenght, {section} and {other}")
            }
        }
    }
}

impl std::error::Error for MalformedError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read(e) => Some(e),
            Self::Preamble(e) => Some(e),
            Self::DuplicateSection { .. } => None,
            Self::SectionOrder { .. } => None,
            Self::Section(e) => Some(e),
            _ => None,
        }
    }
}

impl From<SectionError> for MalformedError {
    fn from(value: SectionError) -> Self {
        MalformedError::Section(value)
    }
}

impl From<ReadError> for MalformedError {
    fn from(value: ReadError) -> Self {
        MalformedError::Read(value)
    }
}

// Parse module
pub fn parse_module(bytes: &[u8]) -> result::Result<Module, Error> {
    module::decode_bytes(bytes.to_vec())
}

// Validate module
pub fn validate_module(_: &Module) -> result::Result<(), Error> {
    Ok(())
}
