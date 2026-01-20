use std::{error, fmt};

use crate::binary::{
    reader::{FromReader, InvalidEnumValueError, ReadError, Reader},
    sections::{SectionEntry, SectionErrorKind},
};

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

impl<'a> FromReader<'a> for ExportKind {
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
pub struct ExportEntry {
    pub name: String,
    #[allow(dead_code)]
    pub kind: ExportKind,
    pub index: u32,
}

pub type ExportSection = Vec<ExportEntry>;

impl SectionEntry for ExportEntry {
    fn decode(reader: &mut Reader) -> std::result::Result<Self, SectionErrorKind> {
        let name = reader.read_name().map_err(ExportSectionReadError::Name)?;
        let kind = reader.read().map_err(ExportSectionReadError::DescKind)?;

        let index = reader
            .read_u32()
            .map_err(ExportSectionReadError::DescIndex)?;

        Ok(ExportEntry { name, kind, index })
    }
}

/// Errors
#[derive(Debug)]
#[non_exhaustive]
pub enum ExportSectionReadError {
    Name(ReadError),
    DescKind(ReadError),
    DescIndex(ReadError),
}

impl error::Error for ExportSectionReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Name(e) => Some(e),
            Self::DescKind(e) => Some(e),
            Self::DescIndex(e) => Some(e),
        }
    }
}

impl fmt::Display for ExportSectionReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Name(_) => write!(f, "reading the export name"),
            Self::DescKind(_) => write!(f, "reading the export kind"),
            Self::DescIndex(_) => write!(f, "reading the export index"),
        }
    }
}

impl From<ExportSectionReadError> for SectionErrorKind {
    fn from(value: ExportSectionReadError) -> Self {
        SectionErrorKind::ExportSection(value)
    }
}
