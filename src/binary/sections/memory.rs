use std::{error, fmt};

use crate::binary::{
    reader::{FromReader, Reader},
    sections::{SectionEntry, SectionErrorKind},
    types::{Limit, LimitReadError},
};

/// Memory Section
#[derive(Debug, Clone)]
pub struct MemType(pub Limit);

pub type MemorySection = Vec<MemType>;

impl<'a> FromReader<'a> for MemType {
    type Error = MemTypeReadError;

    fn from_reader(reader: &mut Reader<'a>) -> std::result::Result<Self, Self::Error> {
        reader.read().map_err(MemTypeReadError).map(Self)
    }
}

#[derive(Debug)]
pub struct MemTypeReadError(pub LimitReadError);

impl error::Error for MemTypeReadError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        Some(&self.0)
    }
}

impl fmt::Display for MemTypeReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "reading its limits")
    }
}

impl SectionEntry for MemType {
    fn decode(reader: &mut Reader) -> std::result::Result<Self, SectionErrorKind> {
        reader.read().map_err(SectionErrorKind::MemorySection)
    }
}
