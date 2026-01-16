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
