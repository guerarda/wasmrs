use core::fmt;
use std::result;

use crate::{
    binary::{
        sections::memory::MemType,
        types::{FuncType, MemArg, MemIndex, TypeIdx, ValType},
    },
    instructions::Instruction,
    limits::MAX_WASM_32BIT_MEMORY_PAGES,
    runtime::{RuntimeError, WASM_MEM_PAGE_BYTE_SIZE, instance::ModuleHandle, value::ExternVal},
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

#[derive(Debug)]
pub struct MemoryInstance {
    pub memtype: MemType,
    pub data: Vec<u8>,
}
impl MemoryInstance {
    pub(super) fn size(&self) -> u32 {
        (self.data.len() / WASM_MEM_PAGE_BYTE_SIZE) as u32
    }

    pub(super) fn grow(&mut self, n_pages: u32) -> result::Result<Option<u32>, RuntimeError> {
        let sz = self.size();
        let new_sz = match sz.checked_add(n_pages) {
            Some(n) => n,
            _ => return Ok(None),
        };

        if new_sz > MAX_WASM_32BIT_MEMORY_PAGES {
            return Ok(None);
        }

        if let Some(max_sz) = self.memtype.0.max
            && new_sz > max_sz
        {
            return Ok(None);
        }

        let len = new_sz as usize * WASM_MEM_PAGE_BYTE_SIZE;
        self.data.resize(len, 0);

        Ok(Some(sz))
    }

    pub(super) fn slice<'a>(
        &'a self,
        base: i32,
        memarg: &'a MemArg,
        len: usize,
    ) -> result::Result<&'a [u8], RuntimeError> {
        let ea = (base as u32)
            .checked_add(memarg.offset)
            .ok_or_else(|| RuntimeError::trap("out-of-bound memory access"))?
            as usize;

        let end = ea
            .checked_add(len)
            .ok_or_else(|| RuntimeError::trap("out-of-bound memory access"))?;

        if end > self.data.len() {
            return Err(RuntimeError::trap("out-of-bound memory access"));
        }

        Ok(&self.data[ea..end])
    }

    pub(super) fn slice_mut<'a>(
        &'a mut self,
        base: i32,
        memarg: &MemArg,
        len: usize,
    ) -> result::Result<&'a mut [u8], RuntimeError> {
        // Calculate effective address
        let ea = (base as u32)
            .checked_add(memarg.offset)
            .ok_or_else(|| RuntimeError::trap("out-of-bound memory access"))?
            as usize;

        let end = ea
            .checked_add(len)
            .ok_or_else(|| RuntimeError::trap("out-of-bound memory access"))?;

        if end > self.data.len() {
            return Err(RuntimeError::trap("out-of-bound memory access"));
        }

        Ok(&mut self.data[ea..end])
    }
}

#[derive(Debug, Default)]
pub struct Memories(pub(crate) Vec<MemoryInstance>);

impl Memories {
    pub(super) fn push(&mut self, mem: MemoryInstance) {
        self.0.push(mem)
    }

    pub(super) fn get(&mut self, idx: MemIndex) -> &mut MemoryInstance {
        debug_assert!(idx == MemIndex::ZERO);
        &mut self.0[0]
    }
}

#[derive(Debug, Default)]
pub struct Store {
    pub funcs: Vec<FuncInstance>,
    pub memories: Memories,
}

impl Store {
    pub fn func(funcs: &[FuncInstance], addr: FuncAddr) -> &FuncInstance {
        &funcs[addr.0]
    }
}
