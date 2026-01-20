use crate::{
    binary::types::{FuncType, TypeIdx, ValType},
    instructions::Instruction,
    runtime::{instance::ModuleHandle, value::ExternVal},
};

#[derive(Debug, Clone, Copy)]
pub struct FuncAddr(usize);

impl From<usize> for FuncAddr {
    fn from(value: usize) -> Self {
        FuncAddr(value)
    }
}

impl TryFrom<&ExternVal> for FuncAddr {
    type Error = anyhow::Error;

    fn try_from(value: &ExternVal) -> std::result::Result<Self, Self::Error> {
        match *value {
            ExternVal::Func(funcaddr) => Ok(funcaddr),
            _ => panic!("oops"),
        }
    }
}

#[derive(Debug)]
pub struct Func {
    #[allow(dead_code)]
    pub typeidx: TypeIdx,
    pub locals: Vec<ValType>,
    pub body: Vec<Instruction>,
}

#[derive(Debug)]
pub struct FuncInstance {
    pub ftype: FuncType,
    pub module: ModuleHandle,
    pub func: Func,
}

#[derive(Debug, Default)]
pub struct Store {
    pub funcs: Vec<FuncInstance>,
}

impl Store {
    pub fn get_func(&self, addr: FuncAddr) -> &FuncInstance {
        &self.funcs[addr.0]
    }
}
