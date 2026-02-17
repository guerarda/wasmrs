use core::{error, fmt};

use crate::{
    binary::reader::{
        FromReader, InvalidEnumValueError, ReadError, ReadErrorKind, Reader, VecReadError,
    },
    instructions::{Instruction, InstructionError, decode_instruction},
};

/// Indices​
pub type TypeIdx = u32;
pub type FuncIdx = u32;
pub type TableIdx = u32;
pub type GlobalIdx = u32;
pub type ElemIdx = u32;
pub type DataIdx = u32;
// pub type LocalIdx = u32;
pub type LabelIdx = u32;

#[derive(Debug)]
pub struct FuncIndex(pub u32);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MemIndex(pub u32); // TODO Make private

impl MemIndex {
    pub const ZERO: Self = Self(0);
}

impl TryFrom<u32> for MemIndex {
    type Error = InvalidEnumValueError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(MemIndex(0)),
            _ => Err(InvalidEnumValueError {
                value: value as u8,
                enum_name: "memidx",
            }),
        }
    }
}

impl<'a> FromReader<'a> for MemIndex {
    type Error = ReadError;

    fn from_reader(reader: &mut Reader<'a>) -> std::result::Result<Self, Self::Error> {
        let pos = reader.position() as usize;
        // Wasm 2.0 enforces that MemIdx should be a single byte 0x00
        (reader.read_u8()? as u32)
            .try_into()
            .map_err(|e| ReadError::at_offset(e, pos))
    }
}

/// RefType
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RefType {
    Func = 0x70,
    Extern = 0x6f,
}

impl TryFrom<u8> for RefType {
    type Error = InvalidEnumValueError;

    fn try_from(value: u8) -> std::result::Result<Self, Self::Error> {
        match value {
            0x70 => Ok(RefType::Func),
            0x6f => Ok(RefType::Extern),
            _ => Err(InvalidEnumValueError {
                value,
                enum_name: std::any::type_name::<Self>(),
            }),
        }
    }
}

impl From<RefType> for ValType {
    fn from(value: RefType) -> Self {
        ValType::Ref(value)
    }
}

impl<'a> FromReader<'a> for RefType {
    type Error = ReadError;

    fn from_reader(reader: &mut Reader<'a>) -> std::result::Result<Self, Self::Error> {
        let pos = reader.position() as usize;
        reader
            .read_u8()?
            .try_into()
            .map_err(|e| ReadError::at_offset(e, pos))
    }
}

impl fmt::Display for RefType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Func => write!(f, "funcref"),
            Self::Extern => write!(f, "externref"),
        }
    }
}

/// ValType
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ValType {
    Ref(RefType),

    V128 = 0x7b,

    F64 = 0x7c,
    F32 = 0x7d,
    I64 = 0x7e,
    I32 = 0x7f,
}

impl ValType {
    pub fn size_bytes(&self) -> u32 {
        match self {
            Self::Ref(_) => unreachable!(),
            Self::V128 => 16,
            Self::F64 | Self::I64 => 8,
            Self::F32 | Self::I32 => 4,
        }
    }
}

impl TryFrom<u8> for ValType {
    type Error = InvalidEnumValueError;

    fn try_from(value: u8) -> std::result::Result<Self, Self::Error> {
        match value {
            0x7f => Ok(ValType::I32),
            0x7e => Ok(ValType::I64),
            0x7d => Ok(ValType::F32),
            0x7c => Ok(ValType::F64),
            0x7b => Ok(ValType::V128),
            0x70 | 0x6f => Ok(RefType::try_from(value)?.into()),
            _ => Err(InvalidEnumValueError {
                value,
                enum_name: std::any::type_name::<Self>(),
            }),
        }
    }
}

impl<'a> FromReader<'a> for ValType {
    type Error = ReadError;

    fn from_reader(reader: &mut Reader<'a>) -> std::result::Result<Self, ReadError> {
        let pos = reader.position() as usize;
        reader
            .read_u8()?
            .try_into()
            .map_err(|e| ReadError::at_offset(e, pos))
    }
}

/// Block Type
#[cfg_attr(test, derive(PartialEq))]
#[derive(Debug, Clone, Copy)]
pub enum BlockType {
    Empty,
    Value(ValType),
    Index(TypeIdx),
}

impl<'a> FromReader<'a> for BlockType {
    type Error = ReadError;

    fn from_reader(reader: &mut Reader<'a>) -> std::result::Result<Self, ReadError> {
        let pos = reader.position() as usize;

        let byte = reader.peek()?;
        if byte == 0x40 {
            let _ = reader.read_u8()?;
            return Ok(BlockType::Empty);
        }

        if let Ok(valtype) = ValType::try_from(byte) {
            // consume
            let _ = reader.read_u8()?;
            return Ok(BlockType::Value(valtype));
        }

        // If neither empty or ValType, then it's a type index encoded
        // as a signed 33 bit integer. but that must be positive.
        let idx = reader.read_i64()?;
        if idx < 0 || idx > u32::MAX as i64 {
            return Err(ReadError::at_offset(
                ReadErrorKind::UnexpectedValue {
                    value: idx.to_string(),
                    expected: "valid u32 value".to_string(),
                },
                pos,
            ));
        }
        Ok(BlockType::Index(idx as u32))
    }
}

/// Functype
#[derive(Debug, Clone, PartialEq)]
pub struct FuncType {
    pub params: Vec<ValType>,
    pub results: Vec<ValType>,
}

/// Const Expression
#[derive(Debug)]
pub struct ConstExpression(pub Vec<Instruction>);

#[derive(Debug)]
#[non_exhaustive]
pub enum ConstExpressionReadError {
    Instruction(InstructionError),
    MissingEnd,
}

impl std::error::Error for ConstExpressionReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Instruction(e) => Some(e),
            _ => None,
        }
    }
}

impl std::fmt::Display for ConstExpressionReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Instruction(_) => write!(f, "reading instruction"),
            Self::MissingEnd => write!(f, "missing end instruction"),
        }
    }
}

impl<'a> FromReader<'a> for ConstExpression {
    type Error = ConstExpressionReadError;

    fn from_reader(reader: &mut Reader<'a>) -> std::result::Result<Self, Self::Error> {
        let mut expr = Vec::<Instruction>::new();

        while !reader.is_exhausted() {
            let instr = decode_instruction(reader).map_err(Self::Error::Instruction)?;
            match instr {
                Instruction::End => {
                    expr.push(instr);
                    return Ok(Self(expr));
                }
                _ => {
                    expr.push(instr);
                }
            }
        }
        Err(Self::Error::MissingEnd)
    }
}

// Limit
#[repr(u8)]
#[derive(Debug)]
pub enum LimitFlag {
    Min = 0x00,
    MinMax = 0x01,
}

impl TryFrom<u8> for LimitFlag {
    type Error = InvalidEnumValueError;

    fn try_from(value: u8) -> std::result::Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Self::Min),
            0x01 => Ok(Self::MinMax),
            _ => Err(InvalidEnumValueError {
                value,
                enum_name: std::any::type_name::<Self>(),
            }),
        }
    }
}

impl<'a> FromReader<'a> for LimitFlag {
    type Error = ReadError;

    fn from_reader(reader: &mut Reader<'a>) -> std::result::Result<Self, Self::Error> {
        let pos = reader.position() as usize;
        reader
            .read_u8()?
            .try_into()
            .map_err(|e| ReadError::at_offset(e, pos))
    }
}

#[derive(Debug, Clone)]
pub struct Limit {
    #[allow(dead_code)]
    pub min: u32,
    #[allow(dead_code)]
    pub max: Option<u32>,
}

#[derive(Debug)]
pub enum LimitReadError {
    Flag(ReadError),
    Min(ReadError),
    Max(ReadError),
}

impl std::error::Error for LimitReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Flag(e) => Some(e),
            Self::Min(e) => Some(e),
            Self::Max(e) => Some(e),
        }
    }
}

impl std::fmt::Display for LimitReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Flag(_) => write!(f, "reading the limit flag"),
            Self::Min(_) => write!(f, "reading the limit min"),
            Self::Max(_) => write!(f, "reading the limit max"),
        }
    }
}

impl<'a> FromReader<'a> for Limit {
    type Error = LimitReadError;

    fn from_reader(reader: &mut Reader<'a>) -> std::result::Result<Self, Self::Error> {
        let flag: LimitFlag = reader.read().map_err(Self::Error::Flag)?;
        let min: u32 = reader.read().map_err(Self::Error::Min)?;

        match flag {
            LimitFlag::Min => Ok(Self { min, max: None }),
            LimitFlag::MinMax => Ok(Self {
                min,
                max: Some(reader.read().map_err(Self::Error::Max)?),
            }),
        }
    }
}

// Branch table indices
#[cfg_attr(test, derive(PartialEq))]
#[derive(Debug, Clone)]
pub struct BranchTableIdx {
    pub labels: Vec<LabelIdx>,
    pub default: LabelIdx,
}

#[derive(Debug)]
#[non_exhaustive]
pub enum BranchTableIdxReadError {
    Labels(VecReadError<ReadError>),
    Default(ReadError),
}

impl error::Error for BranchTableIdxReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Labels(e) => Some(e),
            Self::Default(e) => Some(e),
        }
    }
}

impl fmt::Display for BranchTableIdxReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Labels(_) => write!(f, "reading labels"),
            Self::Default(_) => write!(f, "reading default lael"),
        }
    }
}

impl<'a> FromReader<'a> for BranchTableIdx {
    type Error = BranchTableIdxReadError;

    fn from_reader(reader: &mut Reader<'a>) -> std::result::Result<Self, Self::Error> {
        let labels = reader.read().map_err(Self::Error::Labels)?;
        let default = reader.read().map_err(Self::Error::Default)?;

        Ok(Self { labels, default })
    }
}

/// Memory
#[derive(Debug, Clone)]
#[cfg_attr(test, derive(PartialEq))]
pub struct MemArg {
    pub align: u32,
    pub offset: u32,
}

#[derive(Debug)]
#[non_exhaustive]
pub enum MemArgReadError {
    Align(ReadError),
    Offset(ReadError),
}

impl error::Error for MemArgReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Align(e) => Some(e),
            Self::Offset(e) => Some(e),
        }
    }
}

impl fmt::Display for MemArgReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Align(_) => write!(f, "reading align"),
            Self::Offset(_) => write!(f, "reading offset"),
        }
    }
}

impl<'a> FromReader<'a> for MemArg {
    type Error = MemArgReadError;

    fn from_reader(reader: &mut Reader<'a>) -> std::result::Result<Self, Self::Error> {
        let align: u32 = reader.read().map_err(Self::Error::Align)?;
        let offset: u32 = reader.read().map_err(Self::Error::Offset)?;

        Ok(MemArg { align, offset })
    }
}
