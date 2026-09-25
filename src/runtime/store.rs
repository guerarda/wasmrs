use core::fmt;
use std::{iter::repeat_n, result};

use crate::{
    binary::{
        sections::{
            code::CodeEntry, data::DataSegment, global::GlobalType, memory::MemType,
            table::TableType,
        },
        types::{FuncType, RefType, TypeIdx, ValType},
    },
    instructions::Instruction,
    limits::{MAX_WASM_32BIT_MEMORY_PAGES, MAX_WASM_TABLE_LEN},
    runtime::{
        RuntimeError, TrapErrorKind, WASM_MEM_PAGE_BYTE_SIZE,
        instance::ModuleHandle,
        value::{Ref, Value},
    },
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

impl fmt::Display for FuncAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Func Body
pub enum FuncBody {
    Wasm(WasmFn),
    Host(HostFn),
}

impl fmt::Debug for FuncBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Wasm(w) => write!(f, "Wasm Function({:?})", w),
            Self::Host(_) => write!(f, "Host Function"),
        }
    }
}

#[derive(Debug)]
pub struct FuncInstance {
    pub ftype: FuncType,
    pub module: ModuleHandle,
    pub body: FuncBody,
}

/// Host Fn
pub type HostFn = Box<dyn Fn(&[Value]) -> result::Result<Vec<Value>, RuntimeError>>;

/// Wasm Function
#[derive(Debug, Clone)]
pub struct WasmFn {
    pub typeidx: TypeIdx,
    pub locals: Vec<ValType>,
    pub expr: Vec<Instruction>,
}

#[derive(Debug)]
pub struct MemoryInstance {
    pub memtype: MemType,
    pub data: Vec<u8>,
}
impl MemoryInstance {
    pub(super) fn init(
        &mut self,
        dst: usize,
        src: usize,
        len: usize,
        data_inst: &DataInstance,
    ) -> result::Result<(), RuntimeError> {
        let dst_end = dst
            .checked_add(len)
            .filter(|&end| end <= self.data.len())
            .ok_or(TrapErrorKind::OutOfBoundsMemoryAccess)?;

        let src_end = src
            .checked_add(len)
            .filter(|&end| end <= data_inst.data.len())
            .ok_or(TrapErrorKind::OutOfBoundsMemoryAccess)?;

        if len == 0 {
            return Ok(());
        }

        self.data[dst..dst_end].copy_from_slice(&data_inst.data[src..src_end]);
        Ok(())
    }

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

    pub(super) fn slice(
        &self,
        base: i32,
        offset: u32,
        len: usize,
    ) -> result::Result<&[u8], RuntimeError> {
        let ea = (base as u32)
            .checked_add(offset)
            .ok_or(TrapErrorKind::OutOfBoundsMemoryAccess)? as usize;

        let end = ea
            .checked_add(len)
            .ok_or(TrapErrorKind::OutOfBoundsMemoryAccess)?;

        if end > self.data.len() {
            return Err(TrapErrorKind::OutOfBoundsMemoryAccess.into());
        }

        Ok(&self.data[ea..end])
    }

    pub(super) fn slice_mut(
        &mut self,
        base: i32,
        offset: u32,
        len: usize,
    ) -> result::Result<&mut [u8], RuntimeError> {
        // Calculate effective address
        let ea = (base as u32)
            .checked_add(offset)
            .ok_or(TrapErrorKind::OutOfBoundsMemoryAccess)? as usize;

        let end = ea
            .checked_add(len)
            .ok_or(TrapErrorKind::OutOfBoundsMemoryAccess)?;

        if end > self.data.len() {
            return Err(TrapErrorKind::OutOfBoundsMemoryAccess.into());
        }

        Ok(&mut self.data[ea..end])
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MemAddr(usize);

#[derive(Debug, Default)]
pub struct Memories(pub(crate) Vec<MemoryInstance>);

impl Memories {
    pub(super) fn alloc(&mut self, memtype: &MemType) -> MemAddr {
        let memsize = (memtype.0.min as usize) * WASM_MEM_PAGE_BYTE_SIZE;
        let mem = vec![0u8; memsize];
        self.0.push(MemoryInstance {
            memtype: memtype.clone(),
            data: mem,
        });
        MemAddr(self.0.len() - 1)
    }

    pub(super) fn get(&self, idx: MemAddr) -> &MemoryInstance {
        self.0.get(idx.0).unwrap()
    }

    pub(super) fn get_mut(&mut self, idx: MemAddr) -> &mut MemoryInstance {
        self.0.get_mut(idx.0).unwrap()
    }
}

/// Globals
#[derive(Debug, Clone, Copy)]
pub struct GlobalAddr(pub usize);

#[derive(Debug, Default)]
pub struct Globals(pub Vec<GlobalInstance>);

#[derive(Debug)]
pub struct GlobalInstance {
    pub globaltype: GlobalType,
    pub value: Value,
}

impl GlobalInstance {
    pub fn new(t: GlobalType, value: Value) -> Self {
        Self {
            globaltype: t,
            value,
        }
    }
}

impl Globals {
    pub fn alloc(&mut self, t: GlobalType, v: Value) -> GlobalAddr {
        self.0.push(GlobalInstance::new(t, v));
        GlobalAddr(self.0.len() - 1)
    }

    pub fn get(&self, addr: GlobalAddr) -> &GlobalInstance {
        self.0.get(addr.0).unwrap()
    }

    pub fn get_mut(&mut self, addr: GlobalAddr) -> &mut GlobalInstance {
        self.0.get_mut(addr.0).unwrap()
    }
}

/// Tables
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

#[derive(Debug, Default)]
pub struct Tables(pub Vec<TableInstance>);

#[derive(Debug)]
pub struct TableInstance {
    pub tabletype: TableType,
    pub refs: Vec<Ref>,
}

impl TableInstance {
    pub fn new(tabletype: TableType, refs: Vec<Ref>) -> Self {
        Self { tabletype, refs }
    }

    pub fn init(
        &mut self,
        dst: usize,
        src: usize,
        len: usize,
        elem_inst: &ElemInstance,
    ) -> result::Result<(), RuntimeError> {
        let dst_end = dst
            .checked_add(len)
            .filter(|&end| end <= self.refs.len())
            .ok_or(TrapErrorKind::OutOfBoundsTableAccess)?;

        let src_end = src
            .checked_add(len)
            .filter(|&end| end <= elem_inst.refs.len())
            .ok_or(TrapErrorKind::OutOfBoundsTableAccess)?;

        if len == 0 {
            return Ok(());
        }

        self.refs[dst..dst_end].copy_from_slice(&elem_inst.refs[src..src_end]);
        Ok(())
    }

    pub(super) fn grow(&mut self, inc: u32, val: Ref) -> result::Result<Option<u32>, RuntimeError> {
        let len = self.refs.len();
        let new_len = match len.checked_add(inc as usize) {
            Some(n) => n,
            _ => return Ok(None),
        };

        if new_len > MAX_WASM_TABLE_LEN {
            return Ok(None);
        }

        if let Some(max_sz) = self.tabletype.limit.max
            && new_len > (max_sz as usize)
        {
            return Ok(None);
        }

        self.refs.resize(new_len, val);

        Ok(Some(len as u32))
    }

    pub(super) fn slice(&self, base: i32, len: usize) -> result::Result<&[Ref], RuntimeError> {
        let base = base as usize;
        let end = base
            .checked_add(len)
            .ok_or(TrapErrorKind::OutOfBoundsTableAccess)?;

        if end > self.refs.len() {
            return Err(TrapErrorKind::OutOfBoundsTableAccess.into());
        }

        Ok(&self.refs[base..end])
    }

    pub(super) fn slice_mut(
        &mut self,
        base: i32,
        len: usize,
    ) -> result::Result<&mut [Ref], RuntimeError> {
        let base = base as usize;

        let end = base
            .checked_add(len)
            .ok_or(TrapErrorKind::OutOfBoundsTableAccess)?;

        if end > self.refs.len() {
            return Err(TrapErrorKind::OutOfBoundsTableAccess.into());
        }

        Ok(&mut self.refs[base..end])
    }
}

impl Tables {
    pub fn alloc(&mut self, t: TableType, refs: Vec<Ref>) -> TableAddr {
        self.0.push(TableInstance::new(t, refs));
        TableAddr(self.0.len() - 1)
    }

    pub fn get(&self, addr: TableAddr) -> &TableInstance {
        self.0
            .get(addr.0)
            .expect("table address must refer to an allocated table")
    }

    pub fn get_mut(&mut self, addr: TableAddr) -> &mut TableInstance {
        self.0
            .get_mut(addr.0)
            .expect("table adress must refer to an allocated table")
    }
}

/// Elements
#[derive(Debug, Clone, Copy)]
pub struct ElemAddr(pub usize);

#[derive(Debug, Default)]
pub struct Elements(pub Vec<ElemInstance>);

#[derive(Debug)]
pub struct ElemInstance {
    #[allow(dead_code)]
    elemtype: RefType,
    refs: Vec<Ref>,
    dropped: bool,
}

impl ElemInstance {
    pub fn new(elemtype: RefType, refs: Vec<Ref>) -> Self {
        Self {
            elemtype,
            refs,
            dropped: false,
        }
    }
}

impl Elements {
    pub fn alloc(&mut self, t: RefType, refs: Vec<Ref>) -> ElemAddr {
        self.0.push(ElemInstance::new(t, refs));
        ElemAddr(self.0.len() - 1)
    }

    pub fn get(&self, addr: ElemAddr) -> &ElemInstance {
        (self.0.get(addr.0).unwrap()) as _
    }

    pub fn get_mut(&mut self, addr: ElemAddr) -> &mut ElemInstance {
        (self.0.get_mut(addr.0).unwrap()) as _
    }

    pub fn drop(&mut self, addr: ElemAddr) {
        self.0[addr.0].refs.clear();
        self.0[addr.0].dropped = true;
    }
}

// Data
#[derive(Debug, Clone, Copy)]
pub struct DataAddr(pub usize);

#[derive(Debug, Default)]
pub struct Data(pub Vec<DataInstance>);

#[derive(Debug)]
pub struct DataInstance {
    data: Vec<u8>,
    dropped: bool,
}

impl DataInstance {
    pub fn new(data: Vec<u8>) -> Self {
        Self {
            data,
            dropped: false,
        }
    }
}

impl Data {
    pub fn alloc(&mut self, ds: &DataSegment) -> DataAddr {
        self.0.push(DataInstance::new(ds.data.clone()));
        DataAddr(self.0.len() - 1)
    }

    pub fn get(&self, addr: DataAddr) -> &DataInstance {
        self.0
            .get(addr.0)
            .expect("data address must refer to an allocated data segment")
    }

    pub fn drop(&mut self, addr: DataAddr) {
        self.0[addr.0].data.clear();
        self.0[addr.0].dropped = true;
    }
}

// Functions
#[derive(Debug, Default)]
pub struct Functions(Vec<FuncInstance>);

impl Functions {
    pub fn alloc(
        &mut self,
        module: ModuleHandle,
        ftype: FuncType,
        code: &CodeEntry,
        typeidx: TypeIdx,
    ) -> FuncAddr {
        let locals = {
            let mut v = vec![];
            for l in code.locals.as_slice() {
                v.extend(repeat_n(l.valtype, l.count as usize));
            }
            v
        };
        let func = WasmFn {
            typeidx,
            locals,
            expr: code.body.clone(),
        };

        let funcinst = FuncInstance {
            ftype,
            module,
            body: FuncBody::Wasm(func),
        };

        self.0.push(funcinst);
        FuncAddr(self.0.len() - 1)
    }

    pub fn get(&self, addr: FuncAddr) -> &FuncInstance {
        self.0.get(addr.0).unwrap()
    }

    pub fn alloc_host(
        &mut self,
        module: ModuleHandle,
        ftype: FuncType,
        func: impl Fn(&[Value]) -> result::Result<Vec<Value>, RuntimeError> + 'static,
    ) -> FuncAddr {
        self.0.push(FuncInstance {
            ftype,
            module,
            body: FuncBody::Host(Box::new(func)),
        });
        FuncAddr(self.0.len() - 1)
    }
}

// Store
#[derive(Debug, Default)]
pub struct Store {
    pub functions: Functions,
    pub globals: Globals,
    pub tables: Tables,
    pub elements: Elements,
    pub memories: Memories,
    pub data: Data,
}
