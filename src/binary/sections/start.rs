use crate::binary::reader::Reader;
use crate::binary::sections::{SectionError, SectionErrorKind, SectionInfo};
use crate::binary::types::FuncIndex;

/// Start Section
#[derive(Debug)]
pub struct StartSection(pub FuncIndex);

pub fn decode_start_section(
    reader: &mut Reader,
    info: SectionInfo,
) -> std::result::Result<StartSection, SectionError> {
    let count: u32 = reader.read().map_err(|e| SectionError {
        kind: Box::new(SectionErrorKind::DataCountSection(e)),
        info,
        idx: None,
    })?;
    Ok(StartSection(FuncIndex(count)))
}
