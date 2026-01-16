use crate::binary::{
    reader::Reader,
    sections::{SectionError, SectionErrorKind, SectionInfo},
};

/// Data Count Section
#[derive(Debug)]
#[allow(dead_code)]
pub struct DataCountSection(pub u32);

pub fn decode_data_count_section(
    reader: &mut Reader,
    info: SectionInfo,
) -> std::result::Result<DataCountSection, SectionError> {
    let count: u32 = reader.read().map_err(|e| SectionError {
        kind: Box::new(SectionErrorKind::DataCount(e)),
        info,
        idx: None,
    })?;
    Ok(DataCountSection(count))
}
