use crate::binary::{
    reader::Reader,
    sections::{SectionError, SectionErrorKind, SectionInfo},
    types::FuncIdx,
};

/// Start Section
#[derive(Debug)]
#[allow(dead_code)]
pub struct StartSection(pub FuncIdx);

pub fn decode_start_section(
    reader: &mut Reader,
    info: SectionInfo,
) -> std::result::Result<StartSection, SectionError> {
    let count: u32 = reader.read().map_err(|e| SectionError {
        kind: Box::new(SectionErrorKind::DataCountSection(e)),
        info,
        idx: None,
    })?;
    Ok(StartSection(count))
}
