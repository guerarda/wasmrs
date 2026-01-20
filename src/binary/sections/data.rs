use core::{error, fmt};

use crate::binary::{
    reader::{FromReader, ReadError, ReadErrorKind, Reader, VecReadError},
    sections::{SectionEntry, SectionErrorKind},
    types::{ConstExpression, ConstExpressionReadError},
};

/// Data Section
#[derive(Debug)]
pub enum DataSegmentMode {
    Active {
        #[allow(dead_code)]
        mem_index: u32,
        #[allow(dead_code)]
        offset: ConstExpression,
    },
    Passive,
}

#[derive(Debug)]
pub enum DataSegmentModeReadError {
    Flag(ReadError),
    MemIndex(ReadError),
    Expression(ConstExpressionReadError),
}

impl std::error::Error for DataSegmentModeReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Flag(e) => Some(e),
            Self::MemIndex(e) => Some(e),
            Self::Expression(e) => Some(e),
        }
    }
}

impl std::fmt::Display for DataSegmentModeReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Flag(_) => write!(f, "reading flag"),
            Self::MemIndex(_) => write!(f, "reading mem index"),
            Self::Expression(_) => write!(f, "reading expression"),
        }
    }
}

impl<'a> FromReader<'a> for DataSegmentMode {
    type Error = DataSegmentModeReadError;

    fn from_reader(reader: &mut Reader<'a>) -> std::result::Result<Self, Self::Error> {
        let offset = reader.position() as usize;
        let flag: u32 = reader.read().map_err(Self::Error::Flag)?;

        match flag {
            0x00 => Ok(DataSegmentMode::Active {
                mem_index: 0,
                offset: reader.read().map_err(Self::Error::Expression)?,
            }),
            0x01 => Ok(DataSegmentMode::Passive),
            0x02 => Ok(DataSegmentMode::Active {
                mem_index: reader.read().map_err(Self::Error::MemIndex)?,
                offset: reader.read().map_err(Self::Error::Expression)?,
            }),
            _ => Err(Self::Error::Flag(ReadError {
                offset,
                kind: ReadErrorKind::UnexpectedValue {
                    value: flag.to_string(),
                    expected: "0, 1 or 2".to_string(),
                },
            })),
        }
    }
}

#[derive(Debug)]
pub struct DataSegment {
    #[allow(dead_code)]
    pub mode: DataSegmentMode,
    #[allow(dead_code)]
    pub data: Vec<u8>,
}

pub type DataSection = Vec<DataSegment>;

impl SectionEntry for DataSegment {
    fn decode(reader: &mut Reader) -> std::result::Result<Self, SectionErrorKind> {
        Ok(Self {
            mode: reader
                .read()
                .map_err(DataSectionReadError::DataSegmentMode)?,
            data: reader.read().map_err(DataSectionReadError::DataSegment)?,
        })
    }
}

/// Errors
#[derive(Debug)]
#[non_exhaustive]
pub enum DataSectionReadError {
    DataSegmentMode(DataSegmentModeReadError),
    DataSegment(VecReadError<ReadError>),
}

impl error::Error for DataSectionReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::DataSegmentMode(e) => Some(e),
            Self::DataSegment(e) => Some(e),
        }
    }
}

impl fmt::Display for DataSectionReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DataSegmentMode(_) => write!(f, "reading data segment mode"),
            Self::DataSegment(_) => write!(f, "reading data segment data"),
        }
    }
}

impl From<DataSectionReadError> for SectionErrorKind {
    fn from(value: DataSectionReadError) -> Self {
        SectionErrorKind::DataSection(value)
    }
}
