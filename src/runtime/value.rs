use core::fmt;

use crate::{
    binary::types::{RefType, ValType},
    runtime::store::FuncAddr,
};

#[derive(Debug, Clone, Copy)]
pub enum Value {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    NullRef(RefType),
    FuncRef(u32),
    ExternRef(u32),
}

impl From<ValType> for Value {
    fn from(value: ValType) -> Self {
        match value {
            ValType::I32 => Value::I32(0),
            ValType::I64 => Value::I64(0),
            ValType::F32 => Value::F32(0.0),
            ValType::F64 => Value::F64(0.0),
            ValType::V128 => unimplemented!(),
            ValType::Ref(rt) => Value::NullRef(rt),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::I32(v) => write!(f, "i32({v})"),
            Self::I64(v) => write!(f, "i64({v})"),
            Self::F32(v) => write!(f, "f32({v})"),
            Self::F64(v) => write!(f, "f64({v})"),
            Self::NullRef(v) => write!(f, "{v}(null)"),
            Self::FuncRef(v) => write!(f, "funcref({v})"),
            Self::ExternRef(v) => write!(f, "externref({v})"),
        }
    }
}

#[derive(Debug)]
pub enum ExternVal {
    Func(FuncAddr),
    Table(usize),
    Mem(usize),
    Global(usize),
}
