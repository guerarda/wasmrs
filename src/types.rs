use crate::reader::InvalidEnumValueError;

/// Indices​

pub type TypeIdx = u32;
// pub type FuncIdx = u32;
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

/// Functype
#[derive(Debug, Clone)]
pub struct FuncType {
    pub params: Vec<ValType>,
    pub results: Vec<ValType>,
}
