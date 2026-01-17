use std::ops::Range;
use thiserror::Error;

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

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum CustomSectionReadError {
    #[error("reading the module name")]
    Name(#[source] ReadError),
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
