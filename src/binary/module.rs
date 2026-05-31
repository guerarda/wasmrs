use crate::instructions::Instruction;

use super::reader::{ReadError, ReadErrorKind, Reader};
use super::sections::custom::decode_custom_section;
use super::sections::import::ImportDesc;
use super::sections::{
    CodeSection, CustomSection, DataCountSection, DataSection, ElementSection, ExportSection,
    FunctionSection, GlobalSection, ImportSection, MemorySection, SectionError, SectionErrorKind,
    SectionId, SectionInfo, StartSection, TableSection, TypeSection, decode_data_count_section,
    decode_section, decode_start_section,
};

macro_rules! impl_count_kind {
    ($name: ident, $variant: ident, $field: ident) => {
        pub(crate) fn $name(&self) -> usize {
            let imported = self.imports.as_ref().map_or(0, |imps| {
                imps.iter()
                    .filter(|i| matches!(i.desc, ImportDesc::$variant(_)))
                    .count()
            });
            let local = self.$field.as_ref().map_or(0, |f| f.len());
            imported + local
        }
    };
}

#[derive(Debug)]
#[non_exhaustive]
pub enum MalformedError {
    Read(ReadError),
    Preamble(ReadError),

    DuplicateSection {
        offset: usize,
        id: SectionId,
        other: SectionInfo,
    },
    SectionOrder {
        offset: usize,
        id: SectionId,
        other: SectionInfo,
    },
    Section(SectionError),
    SectionRequired {
        section: SectionId,
    },
    InconsistentLength {
        section: SectionId,
        other: SectionId,
    },
}

impl std::fmt::Display for MalformedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read(_) => write!(f, "Malformed module"),
            Self::Preamble(_) => write!(f, "invalid module preamble"),
            Self::DuplicateSection { offset, id, other } => {
                write!(
                    f,
                    "duplicate section: {id} section at offset {offset:#0x} ({offset}), previously seen at offset {other_offset:#0x} ({other_offset})",
                    id = id,
                    offset = offset,
                    other_offset = other.offset
                )
            }
            Self::SectionOrder { offset, id, other } => {
                write!(
                    f,
                    "section out of order: {id} section at offset {offset:#0x} ({offset}), appears after {other_id} at offset {other_offset:#0x} ({other_offset})",
                    id = id,
                    offset = offset,
                    other_id = other.id,
                    other_offset = other.offset
                )
            }
            Self::Section(_) => write!(f, "malformed section"),
            Self::SectionRequired { section } => write!(f, "section required: {section}"),
            Self::InconsistentLength { section, other } => {
                write!(f, "inconsistent section lenght, {section} and {other}")
            }
        }
    }
}

impl std::error::Error for MalformedError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read(e) => Some(e),
            Self::Preamble(e) => Some(e),
            Self::DuplicateSection { .. } => None,
            Self::SectionOrder { .. } => None,
            Self::Section(e) => Some(e),
            _ => None,
        }
    }
}

impl From<SectionError> for MalformedError {
    fn from(value: SectionError) -> Self {
        MalformedError::Section(value)
    }
}

impl From<ReadError> for MalformedError {
    fn from(value: ReadError) -> Self {
        MalformedError::Read(value)
    }
}

use std::collections::HashMap;
use std::io::{Seek, SeekFrom};

const WASM_MAGIC: [u8; 4] = *b"\0asm";
const WASM_VERSION: [u8; 4] = [0x01, 0x00, 0x00, 0x00];

#[derive(Debug)]
pub struct Module {
    bytes: Vec<u8>,
    sections: Vec<SectionInfo>,

    customs: Vec<CustomSection>,
    pub(crate) types: Option<TypeSection>,
    pub(crate) imports: Option<ImportSection>,
    pub(crate) functions: Option<FunctionSection>,
    pub(crate) tables: Option<TableSection>,
    pub(crate) memories: Option<MemorySection>,
    pub(crate) globals: Option<GlobalSection>,
    pub(crate) exports: Option<ExportSection>,
    pub(crate) elements: Option<ElementSection>,
    pub(crate) start: Option<StartSection>,
    pub(crate) data_count: Option<DataCountSection>,
    pub(crate) codes: Option<CodeSection>,
    pub(crate) data: Option<DataSection>,
}

impl Module {
    impl_count_kind!(func_count, Func, functions);
    impl_count_kind!(memory_count, Mem, memories);
    impl_count_kind!(table_count, Table, tables);
    impl_count_kind!(global_count, Global, globals);

    pub(crate) fn imported_global_count(&self) -> usize {
        self.imports.as_ref().map_or(0, |imps| {
            imps.iter()
                .filter(|i| matches!(i.desc, ImportDesc::Global(_)))
                .count()
        })
    }

    fn from_bytes(bytes: Vec<u8>) -> Self {
        Module {
            bytes,
            sections: Vec::new(),

            customs: vec![],
            types: None,
            imports: None,
            functions: None,
            tables: None,
            memories: None,
            globals: None,
            exports: None,
            elements: None,
            start: None,
            data_count: None,
            codes: None,
            data: None,
        }
    }
}

struct ModuleReader<'a> {
    reader: Reader<'a>,
}

impl<'a> ModuleReader<'a> {
    fn from_module(module: &'a Module) -> Self {
        ModuleReader {
            reader: Reader::from_bytes(&module.bytes, 0),
        }
    }

    fn read_preamble(&mut self) -> crate::binary::reader::Result<()> {
        let mut buf = [0u8; 4];

        self.reader.read_exact(&mut buf)?;
        if buf != WASM_MAGIC {
            return Err(ReadError::at_offset(ReadErrorKind::BadMagic, 0));
        }

        self.reader.read_exact(&mut buf)?;
        if buf != WASM_VERSION {
            return Err(ReadError::at_offset(ReadErrorKind::BadVersion, 4));
        }
        Ok(())
    }

    fn read_toc(&mut self) -> std::result::Result<Vec<SectionInfo>, MalformedError> {
        let mut v: Vec<SectionInfo> = Vec::new();
        let mut seen: HashMap<SectionId, SectionInfo> = HashMap::new();

        while !self.reader.is_exhausted() {
            let offset = self.reader.position() as usize;
            let id: SectionId = self.reader.read_u8()?.try_into().map_err(|e| ReadError {
                kind: ReadErrorKind::InvalidEnumValue(e),
                offset,
            })?;

            if id != SectionId::Custom
                && let Some(other) = seen.get(&id)
            {
                return Err(MalformedError::DuplicateSection {
                    offset,
                    id,
                    other: *other,
                });
            }

            let size = self.reader.read_u32()?;

            let info = SectionInfo {
                offset,
                id,
                start: self.reader.position(),
                end: self
                    .reader
                    .cursor
                    .seek(SeekFrom::Current(size as i64))
                    .unwrap(),
                size,
            };
            if let Some(prev) = v.last()
                && prev.id != SectionId::Custom
                && info.id != SectionId::Custom
                && info.id.order() < prev.id.order()
            {
                return Err(MalformedError::SectionOrder {
                    offset,
                    id,
                    other: *prev,
                });
            }
            v.push(info);
            seen.insert(id, info);
        }
        Ok(v)
    }
}

/// Decode a module from bytes (parsing only, no instantiation)
pub fn decode_bytes(bytes: Vec<u8>) -> std::result::Result<Module, MalformedError> {
    let mut m = Module::from_bytes(bytes);

    let mut r = ModuleReader::from_module(&m);
    r.read_preamble().map_err(MalformedError::Preamble)?;
    m.sections = r.read_toc()?;

    for item in m.sections.iter() {
        let start = item.start as usize;
        let end = item.end as usize;

        let mut reader = Reader::from_bytes_range(&m.bytes, start, end)
            .map_err(SectionErrorKind::SectionSize)
            .map_err(|kind| SectionError {
                kind: Box::new(kind),
                info: *item,
                idx: None,
            })
            .map_err(MalformedError::Section)?;

        match item.id {
            SectionId::Custom => {
                m.customs.push(
                    decode_custom_section(&mut reader, *item).map_err(MalformedError::Section)?,
                );
            }
            SectionId::Type => {
                m.types =
                    Some(decode_section(&mut reader, *item).map_err(MalformedError::Section)?);
            }
            SectionId::Import => {
                m.imports =
                    Some(decode_section(&mut reader, *item).map_err(MalformedError::Section)?);
            }
            SectionId::Function => {
                m.functions =
                    Some(decode_section(&mut reader, *item).map_err(MalformedError::Section)?);
            }
            SectionId::Table => {
                m.tables =
                    Some(decode_section(&mut reader, *item).map_err(MalformedError::Section)?);
            }
            SectionId::Memory => {
                m.memories =
                    Some(decode_section(&mut reader, *item).map_err(MalformedError::Section)?);
            }
            SectionId::Global => {
                m.globals =
                    Some(decode_section(&mut reader, *item).map_err(MalformedError::Section)?);
            }
            SectionId::Export => {
                m.exports =
                    Some(decode_section(&mut reader, *item).map_err(MalformedError::Section)?);
            }
            SectionId::Start => {
                m.start = Some(
                    decode_start_section(&mut reader, *item).map_err(MalformedError::Section)?,
                );
            }
            SectionId::Element => {
                m.elements =
                    Some(decode_section(&mut reader, *item).map_err(MalformedError::Section)?);
            }
            SectionId::Code => {
                m.codes =
                    Some(decode_section(&mut reader, *item).map_err(MalformedError::Section)?);
            }
            SectionId::Data => {
                let data = decode_section(&mut reader, *item).map_err(MalformedError::Section)?;
                // The number of data entry should match the data count section if present
                if let Some(ref dc) = m.data_count
                    && data.len() != dc.0 as usize
                {
                    return Err(MalformedError::InconsistentLength {
                        section: item.id,
                        other: SectionId::DataCount,
                    });
                }
                m.data = Some(data);
            }
            SectionId::DataCount => {
                m.data_count = Some(
                    decode_data_count_section(&mut reader, *item)
                        .map_err(MalformedError::Section)?,
                )
            }
        };
    }

    // Verify that Function & Code have consistent length
    let fs_len = m.functions.as_ref().map_or(0, |fs| fs.len());
    let cs_len = m.codes.as_ref().map_or(0, |fs| fs.len());

    if fs_len != cs_len {
        return Err(MalformedError::InconsistentLength {
            section: SectionId::Code,
            other: SectionId::Function,
        });
    }

    // Verify that Data & Data Count have consistent length
    if let Some(dc) = &m.data_count {
        let ds_len = m.data.as_ref().map_or(0, |fs| fs.len());

        if ds_len != dc.0 as usize {
            return Err(MalformedError::InconsistentLength {
                section: SectionId::Data,
                other: SectionId::DataCount,
            });
        }
    }

    // Verify that data count is present if memory.init or data.drop
    // is present
    if m.data_count.is_none()
        && m.codes.as_ref().is_some_and(|codes| {
            codes
                .iter()
                .flat_map(|c| &c.body)
                .any(|x| matches!(x, Instruction::MemoryInit(_) | Instruction::DataDrop(_)))
        })
    {
        return Err(MalformedError::SectionRequired {
            section: SectionId::DataCount,
        });
    }

    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_minimal_module() -> anyhow::Result<()> {
        let bytes = b"\0asm\x01\x00\x00\x00".to_vec();

        let _ = decode_bytes(bytes)?;

        Ok(())
    }

    #[test]
    fn test_invalid_section_id() -> anyhow::Result<()> {
        let bytes = [
            b"\0asm\x01\x00\x00\x00" as &[u8],
            b"\x0f\x06",
            b"\x01\x60\x01\x7f\x01\x7f",
        ]
        .concat();

        let m = decode_bytes(bytes);
        assert!(m.is_err());

        Ok(())
    }

    #[test]
    fn test_decode_type_section() -> anyhow::Result<()> {
        let bytes = [
            b"\0asm\x01\x00\x00\x00" as &[u8],
            b"\x01\x06",                 // Type section(1), 6 bytes
            b"\x01\x60\x01\x7f\x01\x7f", // 1 function, (i32) -> i32
        ]
        .concat();

        let m = decode_bytes(bytes)?;
        assert!(m.types.is_some());
        assert_eq!(m.types.unwrap().len(), 1);

        Ok(())
    }

    #[test]
    fn test_decode_table_section() -> anyhow::Result<()> {
        // Func Ref, Limit Min only
        let bytes = [
            b"\0asm\x01\x00\x00\x00" as &[u8],
            b"\x04\x04\x01", // Table Section(4), 6 bytes, 1 entry
            b"\x70\x00\x01", // fucn ref, limit min only
        ]
        .concat();

        let m = decode_bytes(bytes)?;
        assert!(m.tables.is_some());
        assert_eq!(m.tables.unwrap().len(), 1);

        // Extern Ref, Limit Min Max
        let bytes = [
            b"\0asm\x01\x00\x00\x00" as &[u8],
            b"\x04\x05\x01",     // Table Section(4), 6 bytes, 1 entry
            b"\x6f\x01\x01\x02", // extern ref, limit min max
        ]
        .concat();

        let m = decode_bytes(bytes)?;
        assert!(m.tables.is_some());
        assert_eq!(m.tables.unwrap().len(), 1);

        Ok(())
    }

    #[test]
    fn test_decode_memory_section() -> anyhow::Result<()> {
        // One MemType, Min Only
        let bytes = [
            b"\0asm\x01\x00\x00\x00" as &[u8],
            b"\x05\x03\x01", // Memory Section(5), one entry
            b"\x00\x01",     // Min
        ]
        .concat();

        let m = decode_bytes(bytes)?;
        assert!(m.memories.is_some());
        assert_eq!(m.memories.unwrap().len(), 1);

        // One MemType, Min Max
        let bytes = [
            b"\0asm\x01\x00\x00\x00" as &[u8],
            b"\x05\x04\x01", // Memory Section(5), one entry
            b"\x01\x01\x02", // Min Max
        ]
        .concat();

        let m = decode_bytes(bytes)?;
        assert!(m.memories.is_some());
        assert_eq!(m.memories.unwrap().len(), 1);

        Ok(())
    }

    #[test]
    fn test_invalid_memory_section() -> anyhow::Result<()> {
        let bytes = [
            b"\0asm\x01\x00\x00\x00" as &[u8],
            b"\x05\x03\x01", // Memory Section(5), one entry
            b"\x02\x01",     // Invalid Flag
        ]
        .concat();

        let m = decode_bytes(bytes);
        assert!(m.is_err());

        Ok(())
    }

    #[test]
    fn test_decode_global_section() -> anyhow::Result<()> {
        let bytes = [
            b"\0asm\x01\x00\x00\x00" as &[u8],
            b"\x06\x06\x01", // Global Section(6), one entry
            b"\x7f\x00\x41\x00\x0b",
        ]
        .concat();
        let m = decode_bytes(bytes)?;
        assert!(m.globals.is_some());
        assert_eq!(m.globals.unwrap().len(), 1);

        Ok(())
    }

    #[test]
    fn test_decode_start_section() -> anyhow::Result<()> {
        let bytes = [
            b"\0asm\x01\x00\x00\x00" as &[u8],
            b"\x08\x01\x00", // Start Section(8), index 0
        ]
        .concat();

        let m = decode_bytes(bytes)?;
        assert!(m.start.is_some());
        assert_eq!(m.start.unwrap().0, 0);

        Ok(())
    }

    #[test]
    fn test_decode_data_count_section() -> anyhow::Result<()> {
        let bytes = [
            b"\0asm\x01\x00\x00\x00" as &[u8],
            b"\x0c\x01\x02", // Data Count section(12), u32(1)
            b"\x0b\x05\x02", // Data Section(11), 2 entries
            b"\x01\x00",
            b"\x01\x00",
        ]
        .concat();

        let m = decode_bytes(bytes)?;
        assert!(m.data_count.is_some());
        assert_eq!(m.data_count.unwrap().0, 2);

        Ok(())
    }

    #[test]
    fn test_decode_data_section() -> anyhow::Result<()> {
        // Test passive data segments (flag 0x01)
        let bytes = [
            b"\0asm\x01\x00\x00\x00" as &[u8],
            b"\x0b\x05\x02", // Data section(11), two entries
            b"\x01\x00",     // passive, empty data
            b"\x01\x00",     // passive, empty data
        ]
        .concat();

        let m = decode_bytes(bytes)?;
        assert!(m.data.is_some());
        assert_eq!(m.data.unwrap().len(), 2);

        // Test active data segment with implicit mem_index 0 (flag 0x00)
        let bytes = [
            b"\0asm\x01\x00\x00\x00" as &[u8],
            b"\x0b\x09\x01",     // Data section(11), size 9, one entry
            b"\x00",             // flag 0x00: active, implicit mem_index 0
            b"\x41\x00\x0b",     // offset expr: i32.const 0, end
            b"\x03\x01\x02\x03", // data: length 3, bytes [1, 2, 3]
        ]
        .concat();

        let m = decode_bytes(bytes)?;
        assert!(m.data.is_some());
        let data = m.data.unwrap();
        assert_eq!(data.len(), 1);
        assert_eq!(data[0].data, vec![1, 2, 3]);

        // Test active data segment with explicit mem_index (flag 0x02)
        let bytes = [
            b"\0asm\x01\x00\x00\x00" as &[u8],
            b"\x0b\x09\x01", // Data section(11), size 9, one entry
            b"\x02",         // flag 0x02: active, explicit mem_index
            b"\x01",         // mem_index: 1
            b"\x41\x10\x0b", // offset expr: i32.const 16, end
            b"\x02\xaa\xbb", // data: length 2, bytes [0xAA, 0xBB]
        ]
        .concat();

        let m = decode_bytes(bytes)?;
        assert!(m.data.is_some());
        let data = m.data.unwrap();
        assert_eq!(data.len(), 1);
        assert_eq!(data[0].data, vec![0xaa, 0xbb]);

        Ok(())
    }

    #[test]
    fn test_invalid_leb128_encoding() -> anyhow::Result<()> {
        let bytes = [
            b"\0asm\x01\x00\x00\x00" as &[u8],
            b"\x05\x0d\x01", // Memory Section(5), one entry
            b"\x00\x82\x80\x80\x80\x80\x80\x80\x80\x80\x80\x00", // Minimum 2, too many bytes
        ]
        .concat();

        let m = decode_bytes(bytes);
        assert!(m.is_err());

        Ok(())
    }

    #[test]
    fn test_duplicate_section() -> anyhow::Result<()> {
        let bytes = [
            b"\0asm\x01\x00\x00\x00" as &[u8],
            b"\x01\x01\x00",
            b"\x01\x01\x00", // Duplicate Type section
        ]
        .concat();

        let m = decode_bytes(bytes);
        assert!(m.is_err());

        Ok(())
    }

    #[test]
    fn test_out_of_order_section() -> anyhow::Result<()> {
        let bytes = [
            b"\0asm\x01\x00\x00\x00" as &[u8],
            b"\x02\x01\x00", // Import Section
            b"\x01\x01\x00", // Type Section should be first
        ]
        .concat();

        let m = decode_bytes(bytes);
        assert!(m.is_err());

        Ok(())
    }

    #[test]
    fn test_global_section2() -> anyhow::Result<()> {
        let bytes = [
            b"\0asm\x01\x00\x00\x00" as &[u8],
            b"\x06\x07\x01",
            b"\x7e\x00",
            b"\x42\xff\x7f\x0b",
        ]
        .concat();

        let _ = decode_bytes(bytes)?;

        Ok(())
    }
}
