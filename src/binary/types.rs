use crate::{
    binary::reader::{FromReader, InvalidEnumValueError, ReadError, Reader},
    instructions::{Instruction, InstructionError, decode_instruction},
};

/// Indices​
pub type TypeIdx = u32;
pub type FuncIdx = u32;
// pub type TableIdx = u32;
// pub type MemIdx = u32;
// pub type GlobalIdx = u32;
// pub type ElemIdx = u32;
// pub type DataIdx = u32;
// pub type LocalIdx = u32;
// pub type LabelIdx = u32;

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

/// Functype
#[derive(Debug, Clone)]
pub struct FuncType {
    pub params: Vec<ValType>,
    pub results: Vec<ValType>,
}

/// Const Expression
#[derive(Debug)]
#[allow(dead_code)]
pub struct ConstExpression(Vec<Instruction>);

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
            if instr != Instruction::End {
                expr.push(instr);
            } else {
                expr.push(instr);
                return Ok(Self(expr));
            }
        }
        Err(Self::Error::MissingEnd)
    }
}

// Limit
#[repr(u8)]
#[derive(Debug, Eq, PartialEq)]
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

#[derive(Debug)]
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
