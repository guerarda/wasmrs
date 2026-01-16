use crate::binary::{
    reader::{FromReader, ReadError, ReadErrorKind, Reader},
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
        let _: FuncTypeMarker = reader.read().map_err(SectionErrorKind::FuncTypeMarker)?;

        Ok(FuncType {
            params: reader.read().map_err(SectionErrorKind::FuncTypeParams)?,
            results: reader.read().map_err(SectionErrorKind::FuncTypeResults)?,
        })
    }
}
