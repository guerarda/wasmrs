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

#[derive(Debug)]
pub enum ExternVal {
    Func(FuncAddr),
    Table(usize),
    Mem(usize),
    Global(usize),
}
