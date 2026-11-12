use crate::binary::reader::{FromReader, Reader};
use crate::binary::sections::{SectionEntry, SectionErrorKind};
use crate::binary::types::TypeIndex;

/// Function Section
pub type FunctionSection = Vec<TypeIndex>;

impl SectionEntry for TypeIndex {
    fn decode(reader: &mut Reader) -> std::result::Result<Self, SectionErrorKind> {
        TypeIndex::from_reader(reader).map_err(SectionErrorKind::FunctionSection)
    }
}
