use std::{error, fmt, result};

use crate::{
    Error, Module,
    binary::{
        module,
        sections::{
            data::DataSegmentMode,
            element::{ElementSegmentItems, ElementSegmentMode},
            export::ExportKind,
            import::ImportDesc,
        },
        types::{
            ConstExpression, DataIdx, ElemIdx, FuncType, GlobalIdx, MemIndex, RefType, TableIdx,
        },
    },
    instructions::Instruction,
    runtime::{
        executor::ExecutionContext,
        instance::{ModuleHandle, ModuleInstance, ModuleRegistry},
        stack::Frame,
        store::{
            Data, DataInstance, ElemInstance, Elements, Globals, Memories,
            MemoryInstance, Store, TableInstance, Tables,
        },
        value::{ExternVal, Ref, Value},
    },
};

pub mod executor;
pub mod instance;
pub mod stack;
pub mod store;
pub mod value;

const WASM_MEM_PAGE_BYTE_SIZE: usize = 1 << 16;

#[derive(Debug, Default)]
pub struct Runtime {
    store: Store,
    module_registry: ModuleRegistry,
}

struct DataInit<'a> {
    offset: &'a ConstExpression,
    len: usize,
    dataidx: usize,
    memidx: MemIndex,
}

struct ElemInit<'a> {
    offset: &'a ConstExpression,
    len: usize,
    elemidx: usize,
    tableidx: TableIdx,
}

impl Runtime {
    fn instantiate_module(&mut self, module: &Module) -> result::Result<ModuleHandle, Error> {
        // Prepare Data and Element init
        let instr_d = module.data.as_ref().map_or(vec![], |datasec| {
            datasec
                .iter()
                .enumerate()
                .filter_map(|(i, d)| match &d.mode {
                    DataSegmentMode::Active { mem_index, offset } => Some(DataInit {
                        offset,
                        len: d.data.len(),
                        dataidx: i,
                        memidx: MemIndex(*mem_index),
                    }),
                    DataSegmentMode::Passive => None,
                })
                .collect()
        });

        let instr_e = module.elements.as_ref().map_or(vec![], |elemsec| {
            elemsec
                .iter()
                .enumerate()
                .filter_map(|(i, e)| match &e.mode {
                    ElementSegmentMode::Active {
                        table_index,
                        offset,
                    } => {
                        let n = match &e.items {
                            ElementSegmentItems::Functions(items) => items.len(),
                            ElementSegmentItems::Expressions(.., const_expressions) => {
                                const_expressions.len()
                            }
                        };
                        Some(ElemInit {
                            offset,
                            len: n,
                            elemidx: i,
                            tableidx: table_index.unwrap_or(0),
                        })
                    }

                    _ => None,
                })
                .collect()
        });

        // Preliminary module instance
        let h = self.module_registry.reserve();
        let mut mi = ModuleInstance::default();

        // Register imports first
        // TODO Type check import descriptor payloads against the resolved
        // ExternVal payload
        if let Some(imports) = &module.imports {
            for import in imports {
                let ev = self
                    .module_registry
                    .resolve(&import.mod_name, &import.name)
                    .ok_or(UnlinkableError::UnknownImport)?;

                match import.desc {
                    ImportDesc::Func(_) => {
                        if let ExternVal::Func(addr) = ev {
                            mi.funcs.push(addr);
                        }
                    }
                    ImportDesc::Table(_) => {
                        if let ExternVal::Table(addr) = ev {
                            mi.tables.push(addr);
                        }
                    }
                    ImportDesc::Mem(_) => {
                        if let ExternVal::Mem(addr) = ev {
                            mi.mems.push(addr);
                        }
                    }
                    ImportDesc::Global(_) => {
                        if let ExternVal::Global(addr) = ev {
                            mi.globals.push(addr);
                        }
                    }
                }
            }
        }

        // Register functions
        if let (Some(funcsec), Some(codesec), Some(typesec)) =
            (&module.functions, &module.codes, &module.types)
        {
            for (code, typeidx) in codesec.iter().zip(funcsec) {
                let ftype = typesec[*typeidx as usize].clone();
                let a = self.store.functions.alloc(h, ftype, code, *typeidx);
                mi.funcs.push(a);
            }
        }

        // Alloc and init types
        mi.types = module.types.clone().unwrap_or_default();

        // Init Globals
        if let Some(globalsec) = &module.globals {
            for g in globalsec {
                let val = Self::eval_expression(&self.store, &mi, &g.body)?;
                let a = self.store.globals.alloc(g.gt.clone(), val);
                mi.globals.push(a);
            }
        };

        // Init Tables
        if let Some(tablesec) = &module.tables {
            for t in tablesec {
                let init = match &t.expr {
                    Some(expr) => Self::eval_expression(&self.store, &mi, expr).and_then(|e| {
                        e.into_ref()
                            .ok_or(RuntimeError::internal("mismatched type"))
                    })?,
                    None => Ref::Null(t.tabletype.elemtype),
                };
                let size = t.tabletype.limit.min as usize;
                let entries = vec![init; size];
                let a = self.store.tables.alloc(t.tabletype.clone(), entries);

                mi.tables.push(a);
            }
        };

        // Init Elements
        if let Some(elementsec) = &module.elements {
            for e in elementsec {
                match &e.items {
                    ElementSegmentItems::Functions(items) => {
                        let refs = items
                            .iter()
                            .map(|it| Ref::Func(mi.funcs[it.0 as usize]))
                            .collect::<Vec<_>>();
                        let a = self.store.elements.alloc(RefType::Func, refs);

                        mi.elems.push(a);
                    }
                    ElementSegmentItems::Expressions(rt, exprs) => {
                        let refs = exprs
                            .iter()
                            .map(|e| {
                                Self::eval_expression(&self.store, &mi, e)?
                                    .into_ref_checked(rt)
                                    .ok_or(RuntimeError::internal("mismatched type"))
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        let a = self.store.elements.alloc(*rt, refs);
                        mi.elems.push(a);
                    }
                }
            }
        };

        // Allocate memories
        if let Some(memsec) = &module.memories {
            assert!(memsec.len() <= 1);

            if let Some(memtype) = memsec.first() {
                let a = self.store.memories.alloc(memtype);
                mi.mems.push(a);
            }
        }

        // Allocate exports
        if let Some(exports) = &module.exports {
            for export in exports {
                let idx = export.index as usize;
                let val = match &export.kind {
                    ExportKind::Func => ExternVal::Func(mi.funcs[idx]),
                    ExportKind::Table => ExternVal::Table(mi.tables[idx]),
                    ExportKind::Memory => ExternVal::Mem(mi.mems[idx]),
                    ExportKind::Global => ExternVal::Global(mi.globals[idx]),
                };
                mi.exports.insert(export.name.clone(), val);
            }
        }

        // Allocate data
        if let Some(datasec) = &module.data {
            for ds in datasec {
                let a = self.store.data.alloc(ds);
                mi.datas.push(a);
            }
        }

        // Execute element initialization
        for ei in instr_e {
            let src = Self::eval_expression(&self.store, &mi, ei.offset)?
                .as_i32()
                .ok_or(RuntimeError::internal("expected i32"))?;
            let ea = mi.elems[ei.elemidx];
            let elem = self.store.elements.get(ea);
            let ta = mi.tables[ei.tableidx as usize];

            self.store
                .tables
                .get_mut(ta)
                .init(src as usize, 0, ei.len, elem)?;

            self.store.elements.drop(ea);
        }

        // Execute data initialization
        for di in instr_d {
            let src = Self::eval_expression(&self.store, &mi, di.offset)?
                .as_i32()
                .ok_or(RuntimeError::internal("expected i32"))?;
            let da = mi.datas[di.dataidx];
            let data = self.store.data.get(da);
            let ma = mi.mems[di.memidx.0 as usize];

            self.store
                .memories
                .get_mut(ma)
                .init(src as usize, 0, di.len, data)?;

            self.store.data.drop(da);
        }

        // Get start function
        let start_fn = module.start.as_ref().map(|s| mi.funcs[s.0 as usize]);

        // Register module
        self.module_registry.add(h, mi);

        // Execute start function after module registration so that
        // module instnace handle is valid
        if let Some(funcidx) = start_fn {
            let mut value_stack = vec![];
            let mut call_stack = vec![];

            let mut ctx = ExecutionContext::new(
                &mut value_stack,
                &mut call_stack,
                &mut self.store,
                &self.module_registry,
            );
            ctx.call(funcidx);
            ctx.execute()?;
        }
        Ok(h)
    }

    pub(super) fn global_get(
        globals: &Globals,
        module_inst: &ModuleInstance,
        idx: GlobalIdx,
    ) -> result::Result<Value, RuntimeError> {
        let a = module_inst
            .globals
            .get(idx as usize)
            .ok_or(RuntimeError::trap("undefined global"))?;
        let g = globals.get(*a);

        Ok(g.value)
    }

    pub(super) fn global_set(
        globals: &mut Globals,
        module_inst: &ModuleInstance,
        idx: GlobalIdx,
        val: Value,
    ) -> result::Result<(), RuntimeError> {
        let a = module_inst
            .globals
            .get(idx as usize)
            .ok_or(RuntimeError::trap("undefined global"))?;
        let g = globals.get_mut(*a);

        // TODO Assert on type
        g.value = val;

        Ok(())
    }

    pub(super) fn table_get<'a>(
        tables: &'a Tables,
        module_inst: &ModuleInstance,
        idx: TableIdx,
    ) -> result::Result<&'a TableInstance, RuntimeError> {
        let a = module_inst
            .tables
            .get(idx as usize)
            .ok_or(RuntimeError::trap("undefined table"))?;
        Ok(tables.get(*a))
    }

    pub(super) fn table_get_mut<'a>(
        tables: &'a mut Tables,
        module_inst: &ModuleInstance,
        idx: TableIdx,
    ) -> result::Result<&'a mut TableInstance, RuntimeError> {
        let a = module_inst
            .tables
            .get(idx as usize)
            .ok_or(RuntimeError::trap("undefined table"))?;
        Ok(tables.get_mut(*a))
    }

    pub(super) fn element_get<'a>(
        elements: &'a Elements,
        module_inst: &ModuleInstance,
        idx: ElemIdx,
    ) -> result::Result<&'a ElemInstance, RuntimeError> {
        let a = module_inst
            .elems
            .get(idx as usize)
            .ok_or(RuntimeError::trap("undefined element"))?;
        Ok(elements.get(*a))
    }

    pub(super) fn memory_get<'a>(
        memories: &'a Memories,
        module_inst: &ModuleInstance,
        idx: MemIndex,
    ) -> result::Result<&'a MemoryInstance, RuntimeError> {
        let a = module_inst
            .mems
            .get(idx.0 as usize)
            .ok_or(RuntimeError::trap("undefined memory"))?;
        Ok(memories.get(*a))
    }

    pub(super) fn memory_get_mut<'a>(
        memories: &'a mut Memories,
        module_inst: &ModuleInstance,
        idx: MemIndex,
    ) -> result::Result<&'a mut MemoryInstance, RuntimeError> {
        let a = module_inst
            .mems
            .get(idx.0 as usize)
            .ok_or(RuntimeError::trap("undefined memory"))?;
        Ok(memories.get_mut(*a))
    }

    pub(super) fn data_get<'a>(
        datas: &'a mut Data,
        module_inst: &ModuleInstance,
        idx: DataIdx,
    ) -> result::Result<&'a DataInstance, RuntimeError> {
        let a = module_inst
            .datas
            .get(idx as usize)
            .ok_or(RuntimeError::trap("undefined data"))?;
        Ok(datas.get(*a))
    }

    /// Evaluate a constant expression (e.g. element or data segment)
    fn eval_expression(
        store: &Store,
        module: &ModuleInstance,
        expr: &ConstExpression,
    ) -> result::Result<Value, RuntimeError> {
        let mut value_stack = vec![];

        for inst in &expr.0 {
            match inst {
                Instruction::End => break,
                Instruction::I32Const(v) => value_stack.push(Value::I32(*v)),
                Instruction::I64Const(v) => value_stack.push(Value::I64(*v)),
                Instruction::F32Const(v) => value_stack.push(Value::F32(*v)),
                Instruction::F64Const(v) => value_stack.push(Value::F64(*v)),
                Instruction::GlobalGet(idx) => {
                    let v = Runtime::global_get(&store.globals, module, *idx)?;
                    value_stack.push(v);
                }
                Instruction::RefNull(rt) => value_stack.push(Value::Ref(Ref::Null(*rt))),
                Instruction::RefFunc(fi) => {
                    value_stack.push(Value::Ref(Ref::Func(module.funcs[*fi as usize])))
                }
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
        let mi = self
            .module_registry
            .get_instance(module)
            .ok_or(RuntimeError::internal("unknown module"))?;

        let funcaddr = mi.exports.get(fn_name).unwrap().try_into().unwrap();
        let arity = self.store.functions.get(funcaddr).ftype.results.len();

        let mut value_stack = Vec::from(fn_args);
        let mut call_stack = vec![];

        let result = {
            let mut ctx = ExecutionContext::new(
                &mut value_stack,
                &mut call_stack,
                &mut self.store,
                &self.module_registry,
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
        let handle = self.instantiate_module(&module)?;
        Ok(handle)
    }

    /// Associate a name to a module, for referencing exports
    pub fn register_module(
        &mut self,
        name: String,
        handle: ModuleHandle,
    ) -> result::Result<(), Error> {
        self.module_registry.register(name, handle);
        Ok(())
    }

    pub fn register_host_fn(
        &mut self,
        mod_name: &str,
        fn_name: &str,
        ftype: FuncType,
        func: impl Fn(&[Value]) -> result::Result<Vec<Value>, RuntimeError> + 'static,
    ) -> result::Result<(), Error> {
        let (mh, host) = self.module_registry.get_or_create_host(mod_name);
        let addr = self.store.functions.alloc_host(mh, ftype, func);
        host.exports
            .insert(fn_name.to_string(), ExternVal::Func(addr));
        Ok(())
    }
}

#[cfg(test)]
impl Runtime {
    pub fn memory_pages(&self, idx: usize) -> usize {
        debug_assert!(idx == 0);
        self.store.memories.0[0].data.len() / WASM_MEM_PAGE_BYTE_SIZE
    }

    pub fn memory_data(&self, idx: usize) -> &[u8] {
        debug_assert!(idx == 0);
        &self.store.memories.0[0].data
    }
}

/// Unlinkable Error
#[derive(Debug)]
#[non_exhaustive]
pub enum UnlinkableError {
    UnknownImport,
    IncompatibleImportType,
}

impl error::Error for UnlinkableError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        None
    }
}

impl fmt::Display for UnlinkableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self {
            Self::UnknownImport => write!(f, "unknown import"),
            Self::IncompatibleImportType => write!(f, "incompatible import type"),
        }
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
