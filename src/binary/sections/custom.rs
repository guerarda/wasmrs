use core::{error, fmt};
use std::ops::Range;

use crate::binary::{
    reader::{ReadError, Reader},
    sections::{SectionError, SectionErrorKind, SectionInfo},
};

/// Custom Section
#[derive(Debug)]
pub struct CustomSection {
    #[allow(dead_code)]
    pub name: String,
    #[allow(dead_code)]
    pub data_span: Range<usize>,
}

pub fn decode_custom_section(
    reader: &mut Reader,
    info: SectionInfo,
) -> std::result::Result<CustomSection, SectionError> {
    let name = reader
        .read()
        .map_err(CustomSectionReadError::Name)
        .map_err(SectionErrorKind::CustomSection)
        .map_err(|e| SectionError {
            kind: Box::new(e),
            info,
            idx: None,
        })?;

    let data_span = reader.position() as usize..info.end as usize;
    Ok(CustomSection { name, data_span })
}

/// Errors
#[derive(Debug)]
#[non_exhaustive]
pub enum CustomSectionReadError {
    Name(ReadError),
}

impl error::Error for CustomSectionReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Name(e) => Some(e),
        }
    }
}

impl fmt::Display for CustomSectionReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Name(_) => write!(f, "reading the name"),
        }
    }
}
