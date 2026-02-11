use std::{error, fmt, iter::repeat_n, result};

use crate::{
    Error, Module,
    binary::{
        module,
        sections::{
            element::{ElementSegmentItems, ElementSegmentMode},
            global::GlobalType,
            memory::MemType,
            table::TableType,
        },
        types::{ConstExpression, GlobalIdx, MemIndex},
    },
    instructions::Instruction,
    limits::MAX_WASM_32BIT_MEMORY_PAGES,
    runtime::{
        executor::ExecutionContext,
        instance::{ModuleHandle, ModuleInstance, ModuleRegistry},
        stack::Frame,
        store::{Func, FuncInstance, Store},
        value::{ExternVal, Ref, Value},
    },
};

pub mod executor;
pub mod instance;
pub mod stack;
pub mod store;
pub mod value;

const WASM_MEM_PAGE_BYTE_SIZE: usize = 1 << 16;

#[derive(Debug)]
pub struct GlobalInstance {
    #[allow(dead_code)]
    globaltype: GlobalType,
    value: Value,
}

#[derive(Debug)]
pub struct MemoryInstance {
    memtype: MemType,
    data: Vec<u8>,
}

#[derive(Debug)]
pub struct TableInstance {
    #[allow(dead_code)]
    tabletype: TableType,
    elem: Vec<Ref>,
}

#[derive(Debug, Default)]
pub struct Runtime {
    store: Store,
    globals: Vec<GlobalInstance>,
    memories: Vec<MemoryInstance>,
    tables: Vec<TableInstance>,
    module_registry: ModuleRegistry,
}

impl Runtime {
    fn instantiate_module(&mut self, module: &Module) -> ModuleHandle {
        let h = self.module_registry.reserve();
        let mut mi = ModuleInstance::default();

        // Handle optional sections - minimal modules may have none
        if let Some(typesec) = &module.types {
            mi.types = typesec.clone();
        }

        // Only process functions if we have both function and code sections
        if let (Some(funcsec), Some(codesec), Some(typesec)) =
            (&module.functions, &module.codes, &module.types)
        {
            for (idx, typeidx) in funcsec.iter().enumerate() {
                let typeidx = *typeidx;

                let locals = {
                    let mut v = vec![];
                    for l in codesec[idx].locals.as_slice() {
                        v.extend(repeat_n(l.valtype, l.count as usize));
                    }
                    v
                };
                let func = Func {
                    typeidx,
                    locals,
                    body: codesec[idx].body.clone(),
                };

                let funcinst = FuncInstance {
                    ftype: typesec[typeidx as usize].clone(),
                    module: h,
                    func,
                };

                mi.funcaddrs.push(self.store.funcs.len().into());
                self.store.funcs.push(funcinst);
            }
        }

        // Only process exports if we have them
        if let Some(exports) = &module.exports {
            for export in exports {
                let funcaddr = mi.funcaddrs[export.index as usize];
                mi.exports
                    .insert(export.name.clone(), ExternVal::Func(funcaddr));
            }
        }

        // Instantiate Globals
        if let Some(globalsec) = &module.globals {
            for global in globalsec {
                self.globals.push(GlobalInstance {
                    globaltype: global.gt.clone(),
                    value: global.gt.type_.into(),
                })
            }
        }

        // Instantiate memory instance
        if let Some(memsec) = &module.memories {
            assert!(memsec.len() <= 1);

            if let Some(memtype) = memsec.first() {
                let memsize = (memtype.0.min as usize) * WASM_MEM_PAGE_BYTE_SIZE;
                let mem = vec![0u8; memsize];
                self.memories.push(MemoryInstance {
                    memtype: memtype.clone(),
                    data: mem,
                });
            }
        }

        // Allocate table instances
        if let Some(tablesec) = &module.tables {
            for tabletype in tablesec {
                mi.tableaddrs.push(self.tables.len().into());
                self.tables.push(TableInstance {
                    tabletype: tabletype.clone(),
                    elem: vec![tabletype.elemtype.into(); tabletype.limit.min as usize],
                })
            }
        }

        // Init table instance
        if let Some(elemsec) = &module.elements {
            for elem in elemsec {
                let Some((idx, offset)) = (match &elem.mode {
                    ElementSegmentMode::Active {
                        table_index,
                        offset,
                    } => {
                        let idx = table_index.unwrap_or(0);
                        let offset = self.eval_expression(offset).unwrap();
                        Some((idx as usize, offset.as_i32().unwrap() as usize))
                    }
                    _ => None,
                }) else {
                    continue;
                };

                match &elem.items {
                    ElementSegmentItems::Functions(items) => {
                        for (i, item) in items.iter().enumerate() {
                            let func_ref = Ref::Func(mi.lookup_func(item));
                            self.tables[idx].elem[offset + i] = func_ref;
                        }
                    }
                    ElementSegmentItems::Expressions(rt, expressions) => {
                        for (i, expr) in expressions.iter().enumerate() {
                            let vref: Ref = self
                                .eval_expression(expr)
                                .and_then(|v| v.try_into())
                                .and_then(|r: Ref| {
                                    if r.is_ref_type(rt) {
                                        Ok(r)
                                    } else {
                                        Err(RuntimeError::internal("non matching ref type"))
                                    }
                                })
                                .unwrap();

                            self.tables[idx].elem[offset + i] = vref;
                        }
                    }
                }
            }
        }

        self.module_registry.register(h, mi);
        h
    }

    pub(super) fn global_get(
        globals: &[GlobalInstance],
        idx: GlobalIdx,
    ) -> result::Result<Value, RuntimeError> {
        globals
            .get(idx as usize)
            .map(|g| g.value)
            .ok_or_else(|| RuntimeError::internal("global index out of bounds"))
    }

    pub(super) fn global_set(
        globals: &mut [GlobalInstance],
        idx: GlobalIdx,
        val: Value,
    ) -> result::Result<(), RuntimeError> {
        let g = globals
            .get_mut(idx as usize)
            .ok_or_else(|| RuntimeError::internal("global index out of bounds"))?;

        // TODO assert on value type
        g.value = val;

        Ok(())
    }

    pub(super) fn memory_grow(
        mem_instances: &mut [MemoryInstance],
        idx: MemIndex,
        n_pages: u32,
    ) -> result::Result<Option<u32>, RuntimeError> {
        let sz = Self::memory_size(mem_instances, idx)?;
        let new_sz = match sz.checked_add(n_pages) {
            Some(n) => n,
            _ => return Ok(None),
        };

        if new_sz > MAX_WASM_32BIT_MEMORY_PAGES {
            return Ok(None);
        }

        if let Some(max_sz) = mem_instances[0].memtype.0.max
            && new_sz > max_sz
        {
            return Ok(None);
        }

        let len = new_sz as usize * WASM_MEM_PAGE_BYTE_SIZE;
        mem_instances[0].data.resize(len, 0);

        Ok(Some(sz))
    }

    pub(super) fn memory_size(
        mem_instances: &[MemoryInstance],
        idx: MemIndex,
    ) -> result::Result<u32, RuntimeError> {
        debug_assert!(idx == MemIndex::ZERO);

        mem_instances
            .first()
            .map(|mem| (mem.data.len() / WASM_MEM_PAGE_BYTE_SIZE) as u32)
            .ok_or_else(|| RuntimeError::internal("assert: mems[0] does not exist"))
    }

    pub(super) fn memory_slice(
        mem_instances: &[MemoryInstance],
        idx: MemIndex,
        offset: usize,
        len: usize,
    ) -> result::Result<&[u8], RuntimeError> {
        debug_assert!(idx == MemIndex::ZERO);

        let mem_inst = &mem_instances[0];
        let end = offset
            .checked_add(len)
            .ok_or_else(|| RuntimeError::trap("out-of-bound memory access"))?;

        if end > mem_inst.data.len() {
            return Err(RuntimeError::trap("out-of-bound memory access"));
        }

        Ok(&mem_inst.data[offset..end])
    }

    pub(super) fn memory_slice_mut(
        mem_instances: &mut [MemoryInstance],
        idx: MemIndex,
        offset: usize,
        len: usize,
    ) -> result::Result<&mut [u8], RuntimeError> {
        debug_assert!(idx == MemIndex::ZERO);

        let mem_inst = &mut mem_instances[0];
        let end = offset
            .checked_add(len)
            .ok_or_else(|| RuntimeError::trap("out-of-bound memory access"))?;

        if end > mem_inst.data.len() {
            return Err(RuntimeError::trap("out-of-bound memory access"));
        }

        Ok(&mut mem_inst.data[offset..end])
    }

    /// Evaluate a constant expression (e.g. element or data segment)
    fn eval_expression(&mut self, expr: &ConstExpression) -> result::Result<Value, RuntimeError> {
        let mut value_stack = vec![];

        for inst in &expr.0 {
            match inst {
                Instruction::End => break,
                Instruction::I32Const(v) => value_stack.push(Value::I32(*v)),
                Instruction::I64Const(v) => value_stack.push(Value::I64(*v)),
                Instruction::F32Const(v) => value_stack.push(Value::F32(*v)),
                Instruction::F64Const(v) => value_stack.push(Value::F64(*v)),
                Instruction::GlobalGet(_) => todo!(),
                Instruction::RefNull(rt) => value_stack.push(Value::Ref(Ref::Null(*rt))),
                Instruction::RefFunc(fi) => value_stack.push(Value::Ref(Ref::Func((*fi).into()))),
                _ => return Err(RuntimeError::trap("invalid const expression")),
            }
        }
        value_stack
            .pop()
            .ok_or_else(|| RuntimeError::trap("const expression produced no value"))
    }

    /// Invoke a function from a given module
    pub fn invoke(
        &mut self,
        module: ModuleHandle,
        fn_name: &str,
        fn_args: &[Value],
    ) -> result::Result<Vec<Value>, Error> {
        let mi = self.module_registry.get_instance(module);
        let funcaddr = mi.exports.get(fn_name).unwrap().try_into().unwrap();
        let arity = self.store.get_func(funcaddr).ftype.results.len();

        let mut value_stack = Vec::from(fn_args);
        let mut call_stack = vec![];

        let result = {
            let mut ctx = ExecutionContext::new(
                &mut value_stack,
                &mut call_stack,
                &mut self.store,
                &self.module_registry,
                &mut self.globals,
                &mut self.memories,
                &mut self.tables,
            );

            ctx.call(funcaddr);
            ctx.execute()
        };
        result.map_err(|e| e.with_stacks(call_stack.clone(), value_stack.clone()))?;

        let stack_len = value_stack.len();
        if stack_len < arity {
            return Err(RuntimeError::internal("invalid stack len after function invocation: len={stack_len}, function arity={arity}").into());
        }

        Ok(value_stack.split_off(stack_len - arity))
    }

    /// Decode and instantiate a module from bytes
    pub fn load_module(&mut self, bytes: &[u8]) -> std::result::Result<ModuleHandle, Error> {
        let module = module::decode_bytes(bytes.to_vec())?;
        //validation::validate_module(&module)?;
        let handle = self.instantiate_module(&module);
        Ok(handle)
    }
}

#[cfg(test)]
impl Runtime {
    pub fn memory_pages(&self, idx: usize) -> usize {
        self.memories[idx].data.len() / WASM_MEM_PAGE_BYTE_SIZE
    }

    pub fn memory_data(&self, idx: usize) -> &[u8] {
        &self.memories[idx].data
    }
}

/// Trap Error
#[derive(Debug)]
pub struct RuntimeError {
    pub kind: RuntimeErrorKind,
    pub pc: isize,
    pub instruction: &'static str,
    pub call_stack: Vec<Frame>,
    pub value_stack: Vec<Value>,
    pub fn_name: Option<String>,
}

impl error::Error for RuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.kind)
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{} error", self.kind)?;
        writeln!(f, "{}", self.instruction)?;

        // Print value stack, topmosts only : ... | valtype(val) | ... |
        write!(f, "value stack: ")?;
        let n = 8;
        if self.value_stack.len() > n {
            write!(f, "... ")?;
        }
        for v in self.value_stack.iter().rev().take(n).rev() {
            write!(f, "| {v} ")?;
        }
        writeln!(f, "|")?;

        // Print call stack
        writeln!(f, "call stack:")?;
        for (i, frame) in self.call_stack.iter().rev().enumerate() {
            writeln!(
                f,
                "  #{i}: func[{}] pc={} sp={} arity={} locals={}",
                frame.funcaddr,
                frame.pc,
                frame.sp,
                frame.arity,
                frame.locals.len()
            )?;
        }
        Ok(())
    }
}

impl RuntimeError {
    fn trap(msg: &'static str) -> Self {
        Self {
            kind: RuntimeErrorKind::Trap(msg),
            pc: -1,
            instruction: "",
            call_stack: vec![],
            value_stack: vec![],
            fn_name: None,
        }
    }

    fn internal(msg: &'static str) -> Self {
        Self {
            kind: RuntimeErrorKind::Internal(msg),
            pc: -1,
            instruction: "",
            call_stack: vec![],
            value_stack: vec![],
            fn_name: None,
        }
    }

    #[allow(dead_code)]
    fn with_stacks(self, call_stack: Vec<Frame>, value_stack: Vec<Value>) -> Self {
        debug_assert!(
            self.call_stack.is_empty(),
            "with_stacks() called on already-populated trap error"
        );
        debug_assert!(
            self.call_stack.is_empty(),
            "with_stacks() called on already-populated trap error"
        );
        Self {
            kind: self.kind,
            pc: self.pc,
            instruction: self.instruction,
            call_stack,
            value_stack,
            fn_name: self.fn_name,
        }
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum RuntimeErrorKind {
    Trap(&'static str),
    Internal(&'static str),
}

impl error::Error for RuntimeErrorKind {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}

impl fmt::Display for RuntimeErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Trap(msg) => write!(f, "trap: {msg}"),
            Self::Internal(msg) => write!(f, "internal: {msg}"),
        }
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum TrapErrorKind {
    Unreachable,
    DivisionByZero,
}

impl error::Error for TrapErrorKind {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}

impl fmt::Display for TrapErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable => write!(f, "unreachable"),
            Self::DivisionByZero => write!(f, "division by zero"),
        }
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum InternalErrorKind {
    ValueStackUnderflow,
    CallStackUnderflow,
}

impl error::Error for InternalErrorKind {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}

impl fmt::Display for InternalErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ValueStackUnderflow => write!(f, "value stack underflow"),
            Self::CallStackUnderflow => write!(f, "call stack underflow"),
        }
    }
}
