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

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ValType {
    I32 = 0x7f,
    I64 = 0x7e,
    F32 = 0x7d,
    F64 = 0x7c,

    V128 = 0x7b,
    // Reference Types
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
            _ => Err(InvalidEnumValueError {
                value,
                enum_name: std::any::type_name::<ValType>(),
            }),
        }
    }
}

#[derive(Debug)]
pub struct FuncType {
    pub params: Vec<ValType>,
    pub results: Vec<ValType>,
}
