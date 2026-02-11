use core::fmt;

use crate::{
    binary::types::{FuncType, TypeIdx, ValType},
    instructions::Instruction,
    runtime::{instance::ModuleHandle, value::ExternVal},
};

/// Func Addr
#[derive(Debug, Clone, Copy)]
pub struct FuncAddr(pub usize);

impl From<usize> for FuncAddr {
    fn from(value: usize) -> Self {
        FuncAddr(value)
    }
}

impl From<u32> for FuncAddr {
    fn from(value: u32) -> Self {
        FuncAddr(value as usize)
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

impl fmt::Display for FuncAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Table Addr
#[derive(Debug, Clone, Copy)]
pub struct TableAddr(pub usize);

impl From<usize> for TableAddr {
    fn from(value: usize) -> Self {
        TableAddr(value)
    }
}

impl From<u32> for TableAddr {
    fn from(value: u32) -> Self {
        TableAddr(value as usize)
    }
}

/// Func Instance
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
