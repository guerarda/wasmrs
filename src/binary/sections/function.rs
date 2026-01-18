use crate::binary::{
    reader::{FromReader, Reader},
    sections::{SectionEntry, SectionErrorKind},
    types::TypeIdx,
};

/// Function Section
pub type FunctionSection = Vec<TypeIdx>;

impl SectionEntry for TypeIdx {
    fn decode(reader: &mut Reader) -> std::result::Result<Self, SectionErrorKind> {
        TypeIdx::from_reader(reader).map_err(SectionErrorKind::FunctionSection)
    }
}
