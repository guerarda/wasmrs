use core::fmt;

use crate::{
    binary::types::{RefType, ValType},
    runtime::{RuntimeError, store::FuncAddr},
};

#[derive(Debug, Clone, Copy)]
pub enum Ref {
    Null(RefType),
    Func(FuncAddr),
    Extern(u32),
}

impl Ref {
    pub fn is_ref_type(&self, ref_type: &RefType) -> bool {
        match self {
            Self::Null(rt) => rt == ref_type,
            Self::Func(_) => ref_type == &RefType::Func,
            Self::Extern(_) => ref_type == &RefType::Extern,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Ref::Null(_))
    }

    pub fn as_funcref(&self) -> Option<FuncAddr> {
        match self {
            Self::Func(addr) => Some(*addr),
            _ => None,
        }
    }
}

impl TryFrom<Value> for Ref {
    type Error = RuntimeError;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::Ref(r) => Ok(r),
            _ => Err(RuntimeError::internal("expected value ref")),
        }
    }
}

impl fmt::Display for Ref {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Null(v) => write!(f, "{v}(null)"),
            Self::Func(v) => write!(f, "funcref({v})"),
            Self::Extern(v) => write!(f, "externref({v})"),
        }
    }
}

impl From<RefType> for Ref {
    fn from(value: RefType) -> Self {
        Ref::Null(value)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Value {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    Ref(Ref),
}

impl From<ValType> for Value {
    fn from(value: ValType) -> Self {
        match value {
            ValType::I32 => Value::I32(0),
            ValType::I64 => Value::I64(0),
            ValType::F32 => Value::F32(0.0),
            ValType::F64 => Value::F64(0.0),
            ValType::V128 => unimplemented!(),
            ValType::Ref(rt) => Value::Ref(Ref::Null(rt)),
        }
    }
}

impl Value {
    pub fn into_ref(self) -> Option<Ref> {
        match self {
            Value::Ref(r) => Some(r),
            _ => None,
        }
    }

    pub fn into_ref_checked(self, rt: &RefType) -> Option<Ref> {
        self.into_ref().filter(|r| r.is_ref_type(rt))
    }

    pub fn as_bool(self) -> Option<bool> {
        match self {
            Value::I32(v) => Some(v != 0),
            _ => None,
        }
    }

    pub fn as_i32(self) -> Option<i32> {
        match self {
            Value::I32(v) => Some(v),
            _ => None,
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
            Self::Ref(v) => write!(f, "{}", v),
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
