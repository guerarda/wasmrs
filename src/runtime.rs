use std::{error, fmt, result};

use crate::{
    Error, Limit, Module,
    binary::{
        module,
        sections::{
            data::DataSegmentMode,
            element::{ElementSegmentItems, ElementSegmentMode},
            export::ExportKind,
            global::GlobalType,
            import::ImportDesc,
            memory::MemType,
            table::TableType,
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
            Data, DataInstance, ElemInstance, Elements, FuncAddr, Globals, Memories,
            MemoryInstance, Store, TableInstance, Tables,
        },
        value::{ExternVal, Ref, Value},
    },
    validation,
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
        if let Some(imports) = &module.imports {
            for import in imports {
                let externval = self
                    .module_registry
                    .resolve(&import.mod_name, &import.name)
                    .ok_or(UnlinkableError::UnknownImport)?;

                match (&import.desc, &externval) {
                    (ImportDesc::Func(idx), ExternVal::Func(addr)) => {
                        let func = self.store.functions.get(*addr);
                        let expected = module
                            .types
                            .as_ref()
                            .and_then(|t| t.get(*idx as usize))
                            .expect("type index within bound");

                        if func.ftype != *expected {
                            return Err(UnlinkableError::IncompatibleImportType.into());
                        }
                        mi.funcs.push(*addr);
                    }
                    (ImportDesc::Table(expected), ExternVal::Table(addr)) => {
                        let ti = self.store.tables.get(*addr);
                        // Use the limit from the current state, after table.grow
                        // the min changes, not the original limit
                        let current = Limit {
                            min: ti
                                .refs
                                .len()
                                .try_into()
                                .expect("table length within declared max"),
                            max: ti.tabletype.limit.max,
                        };

                        if ti.tabletype.elemtype != expected.elemtype
                            || !current.matches(&expected.limit)
                        {
                            return Err(UnlinkableError::IncompatibleImportType.into());
                        }
                        mi.tables.push(*addr);
                    }
                    (ImportDesc::Mem(expected), ExternVal::Mem(addr)) => {
                        let mem = self.store.memories.get(*addr);
                        // Use the limit from the current state, after mem.grow
                        // the min changes, not the original limit
                        let current = Limit {
                            min: mem.size(),
                            max: mem.memtype.0.max,
                        };
                        if !current.matches(&expected.0) {
                            return Err(UnlinkableError::IncompatibleImportType.into());
                        }
                        mi.mems.push(*addr);
                    }
                    (ImportDesc::Global(expected), ExternVal::Global(addr)) => {
                        let global = self.store.globals.get(*addr);
                        if global.globaltype != *expected {
                            return Err(UnlinkableError::IncompatibleImportType.into());
                        }
                        mi.globals.push(*addr);
                    }
                    _ => return Err(UnlinkableError::IncompatibleImportType.into()),
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
                // Declarative are immediately dropped
                if matches!(e.mode, ElementSegmentMode::Declarative) {
                    let a = self.store.elements.alloc(e.reftype(), vec![]);
                    self.store.elements.drop(a);
                    mi.elems.push(a);
                    continue;
                }

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

        // Register module
        // The module needs to be fully allocated before the element
        // and data init loop runs
        self.module_registry.add(h, mi);

        // Execute element initialization
        let mi = self.module_registry.get_instance(h).unwrap();
        for ei in instr_e {
            let src = Self::eval_expression(&self.store, mi, ei.offset)?
                .as_i32()
                .ok_or(RuntimeError::internal("expected i32"))?;
            let ea = mi.elems[ei.elemidx];
            let ta = mi.tables[ei.tableidx as usize];
            let elem = self.store.elements.get(ea);

            self.store
                .tables
                .get_mut(ta)
                .init(src as usize, 0, ei.len, elem)?;

            self.store.elements.drop(ea);
        }

        // Execute data initialization
        let mi = self.module_registry.get_instance(h).unwrap();
        for di in instr_d {
            let src = Self::eval_expression(&self.store, mi, di.offset)?
                .as_i32()
                .ok_or(RuntimeError::internal("expected i32"))?;
            let da = mi.datas[di.dataidx];
            let ma = mi.mems[di.memidx.0 as usize];
            let data = self.store.data.get(da);

            self.store
                .memories
                .get_mut(ma)
                .init(src as usize, 0, di.len, data)?;

            self.store.data.drop(da);
        }

        // Get start function
        let mi = self.module_registry.get_instance(h).unwrap();
        let start_fn = module.start.as_ref().map(|s| mi.funcs[s.0 as usize]);

        // Execute start function after module registration so that
        // module instnace handle is valid
        if let Some(funcidx) = start_fn {
            let mut call_stack = vec![];

            let mut ctx =
                ExecutionContext::new(&[], &mut call_stack, &mut self.store, &self.module_registry);
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
            .ok_or(RuntimeError::internal("undefined global"))?;
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
            .ok_or(RuntimeError::internal("undefined global"))?;
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
            .ok_or(RuntimeError::internal("undefined table"))?;
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
            .ok_or(RuntimeError::internal("undefined table"))?;
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
            .ok_or(RuntimeError::internal("undefined element"))?;
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
            .ok_or(RuntimeError::internal("undefined memory"))?;
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
            .ok_or(RuntimeError::internal("undefined memory"))?;
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
            .ok_or(RuntimeError::internal("undefined data"))?;
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
                _ => return Err(RuntimeError::internal("invalid const expression")),
            }
        }
        value_stack
            .pop()
            .ok_or_else(|| RuntimeError::internal("const expression produced no value"))
    }

    /// Invoke a function from a given module
    pub fn invoke(
        &mut self,
        module: ModuleHandle,
        fn_name: &str,
        fn_args: &[Value],
    ) -> result::Result<Vec<Value>, Error> {
        let ev = self
            .module_registry
            .resolve_export(module, fn_name)
            .ok_or(RuntimeError::internal("unknown export"))?;
        let funcaddr: FuncAddr = ev
            .as_func()
            .ok_or(RuntimeError::internal("export is not a function"))?;
        let arity = self.store.functions.get(funcaddr).ftype.results.len();

        let mut value_stack;
        let mut call_stack = vec![];

        let result = {
            let mut ctx = ExecutionContext::new(
                &fn_args,
                &mut call_stack,
                &mut self.store,
                &self.module_registry,
            );

            ctx.call(funcaddr);
            let result = ctx.execute();
            value_stack = ctx.into_value_stack();
            result
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
        validation::validate_module(&module)?;
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
    ) -> result::Result<ModuleHandle, Error> {
        let (mh, host) = self.module_registry.get_or_create_host(mod_name);
        let addr = self.store.functions.alloc_host(mh, ftype, func);
        host.exports
            .insert(fn_name.to_string(), ExternVal::Func(addr));
        Ok(mh)
    }

    pub fn register_host_global(
        &mut self,
        mod_name: &str,
        name: &str,
        gtype: GlobalType,
        value: Value,
    ) -> result::Result<ModuleHandle, Error> {
        let (mh, host) = self.module_registry.get_or_create_host(mod_name);
        let addr = self.store.globals.alloc(gtype, value);
        host.exports
            .insert(name.to_string(), ExternVal::Global(addr));
        Ok(mh)
    }

    pub fn register_host_table(
        &mut self,
        mod_name: &str,
        name: &str,
        ttype: TableType,
        init_ref: Ref,
    ) -> result::Result<ModuleHandle, Error> {
        let (mh, host) = self.module_registry.get_or_create_host(mod_name);
        let size = ttype.limit.min as usize;

        let addr = self.store.tables.alloc(ttype, vec![init_ref; size]);
        host.exports
            .insert(name.to_string(), ExternVal::Table(addr));
        Ok(mh)
    }

    pub fn register_host_memory(
        &mut self,
        mod_name: &str,
        name: &str,
        mtype: MemType,
    ) -> result::Result<ModuleHandle, Error> {
        let (mh, host) = self.module_registry.get_or_create_host(mod_name);
        let addr = self.store.memories.alloc(&mtype);
        host.exports.insert(name.to_string(), ExternVal::Mem(addr));
        Ok(mh)
    }

    pub fn get_global_value(&self, mh: ModuleHandle, name: &str) -> Option<Value> {
        match self.module_registry.resolve_export(mh, name)? {
            ExternVal::Global(addr) => Some(self.store.globals.get(addr).value),
            _ => None,
        }
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

impl From<UnlinkableError> for Error {
    fn from(value: UnlinkableError) -> Self {
        Error::Unlinkable(value)
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

impl From<RuntimeErrorKind> for RuntimeError {
    fn from(value: RuntimeErrorKind) -> Self {
        Self {
            kind: value,
            pc: -1,
            instruction: "",
            call_stack: vec![],
            value_stack: vec![],
            fn_name: None,
        }
    }
}

impl From<TrapErrorKind> for RuntimeError {
    fn from(value: TrapErrorKind) -> Self {
        RuntimeErrorKind::from(value).into()
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum RuntimeErrorKind {
    Trap(TrapErrorKind),
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
            Self::Trap(kind) => write!(f, "trap: {kind}"),
            Self::Internal(msg) => write!(f, "internal: {msg}"),
        }
    }
}

impl From<TrapErrorKind> for RuntimeErrorKind {
    fn from(value: TrapErrorKind) -> Self {
        RuntimeErrorKind::Trap(value)
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum TrapErrorKind {
    Unreachable,
    DivisionByZero,
    CallStackExhausted,
    OutOfBoundsMemoryAccess,
    OutOfBoundsTableAccess,
    IndirectCallTypeMismatch,
    UninitializedElement,
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
            Self::CallStackExhausted => write!(f, "call stack exhausted"),
            Self::OutOfBoundsMemoryAccess => write!(f, "out of bounds memory access"),
            Self::OutOfBoundsTableAccess => write!(f, "out of bounds table access"),
            Self::IndirectCallTypeMismatch => write!(f, "indirect call type mismatch"),
            Self::UninitializedElement => write!(f, "uninitialized element"),
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

#[cfg(test)]
mod tests {
    use crate::{
        Error, Limit,
        binary::{
            sections::{
                global::{GlobalType, MutabilityFlag},
                memory::MemType,
                table::TableType,
            },
            types::{FuncType, RefType, ValType},
        },
        runtime::{
            Runtime, UnlinkableError,
            value::{Ref, Value},
        },
    };

    #[test]
    fn test_host_fn_direct_call() -> anyhow::Result<()> {
        let mut runtime = Runtime::default();

        let mh = runtime.register_host_fn(
            "env",
            "add",
            FuncType {
                params: vec![ValType::I32, ValType::I32],
                results: vec![ValType::I32],
            },
            |args| {
                let a = args[0].as_i32().unwrap();
                let b = args[1].as_i32().unwrap();
                Ok(vec![Value::I32(a + b)])
            },
        )?;

        let cases = [(3, 4, 7), (0, 0, 0), (-1, 1, 0)];

        for (a, b, expected) in cases {
            let r = runtime.invoke(mh, "add", &[Value::I32(a), Value::I32(b)])?;
            assert!(
                matches!(r.as_slice(), [Value::I32(v)] if *v == expected),
                "add({a}, {b}): expected {expected}, got {r:?}",
            );
        }

        Ok(())
    }

    #[test]
    fn test_host_fn_import() -> anyhow::Result<()> {
        // (module
        //   (import "env" "add" (func $add (param i32 i32) (result i32)))
        //   (func (export "call_add") (param i32 i32) (result i32)
        //     local.get 0 local.get 1 call 0))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            // type section: 1 type, (i32 i32) -> (i32)
            b"\x01\x07\x01\x60\x02\x7f\x7f\x01\x7f",
            // import section: "env"."add" -> func type 0
            b"\x02\x0b\x01\x03\x65\x6e\x76\x03\x61\x64\x64\x00\x00",
            // function section: 1 func, type 0
            b"\x03\x02\x01\x00",
            // export section: "call_add" -> func 1
            b"\x07\x0c\x01\x08\x63\x61\x6c\x6c\x5f\x61\x64\x64\x00\x01",
            // code section: local.get 0, local.get 1, call 0, end
            b"\x0a\x0a\x01\x08\x00\x20\x00\x20\x01\x10\x00\x0b",
        ]
        .concat();

        let mut runtime = Runtime::default();

        runtime.register_host_fn(
            "env",
            "add",
            FuncType {
                params: vec![ValType::I32, ValType::I32],
                results: vec![ValType::I32],
            },
            |args| {
                let a = args[0].as_i32().unwrap();
                let b = args[1].as_i32().unwrap();
                Ok(vec![Value::I32(a + b)])
            },
        )?;

        let mh = runtime.load_module(&bytes)?;

        let cases = [(10, 20, 30), (0, 0, 0), (-5, 3, -2)];

        for (a, b, expected) in cases {
            let r = runtime.invoke(mh, "call_add", &[Value::I32(a), Value::I32(b)])?;
            assert!(
                matches!(r.as_slice(), [Value::I32(v)] if *v == expected),
                "call_add({a}, {b}): expected {expected}, got {r:?}",
            );
        }

        Ok(())
    }

    #[test]
    fn test_host_fn_multiple_imports() -> anyhow::Result<()> {
        // (module
        //   (import "math" "double" (func $double (param i32) (result i32)))
        //   (import "math" "negate" (func $negate (param i32) (result i32)))
        //   (func (export "double_negate") (param i32) (result i32)
        //     local.get 0 call 0 call 1))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            // type section: 1 type, (i32) -> (i32)
            b"\x01\x06\x01\x60\x01\x7f\x01\x7f",
            // import section: "math"."double" type 0, "math"."negate" type 0
            b"\x02\x1d\x02\x04\x6d\x61\x74\x68\x06\x64\x6f\x75\x62\x6c\x65\x00\x00\x04\x6d\x61\x74\x68\x06\x6e\x65\x67\x61\x74\x65\x00\x00",
            // function section: 1 func, type 0
            b"\x03\x02\x01\x00",
            // export section: "double_negate" -> func 2
            b"\x07\x11\x01\x0d\x64\x6f\x75\x62\x6c\x65\x5f\x6e\x65\x67\x61\x74\x65\x00\x02",
            // code section: local.get 0, call 0, call 1, end
            b"\x0a\x0a\x01\x08\x00\x20\x00\x10\x00\x10\x01\x0b",
        ]
        .concat();

        let mut runtime = Runtime::default();

        runtime.register_host_fn(
            "math",
            "double",
            FuncType {
                params: vec![ValType::I32],
                results: vec![ValType::I32],
            },
            |args| {
                let x = args[0].as_i32().unwrap();
                Ok(vec![Value::I32(x * 2)])
            },
        )?;

        runtime.register_host_fn(
            "math",
            "negate",
            FuncType {
                params: vec![ValType::I32],
                results: vec![ValType::I32],
            },
            |args| {
                let x = args[0].as_i32().unwrap();
                Ok(vec![Value::I32(-x)])
            },
        )?;

        let mh = runtime.load_module(&bytes)?;

        let cases = [(5, -10), (0, 0), (-3, 6)];

        for (input, expected) in cases {
            let r = runtime.invoke(mh, "double_negate", &[Value::I32(input)])?;
            assert!(
                matches!(r.as_slice(), [Value::I32(v)] if *v == expected),
                "double_negate({input}): expected {expected}, got {r:?}",
            );
        }

        Ok(())
    }

    #[test]
    fn test_import_func_type_mismatch_params() -> anyhow::Result<()> {
        // (module (import "env" "f" (func (param i32) (result i32))))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            // type section: 1 type, (i32) -> (i32)
            b"\x01\x06\x01\x60\x01\x7f\x01\x7f",
            // import section: "env"."f" -> func type 0
            b"\x02\x09\x01\x03\x65\x6e\x76\x01\x66\x00\x00",
        ]
        .concat();

        let mut runtime = Runtime::default();
        runtime.register_host_fn(
            "env",
            "f",
            FuncType {
                params: vec![ValType::I64],
                results: vec![ValType::I32],
            },
            |_| Ok(vec![Value::I32(0)]),
        )?;

        let r = runtime.load_module(&bytes);
        assert!(
            matches!(
                r,
                Err(Error::Unlinkable(UnlinkableError::IncompatibleImportType))
            ),
            "expected IncompatibleImportType, got {r:?}",
        );
        Ok(())
    }

    #[test]
    fn test_import_func_type_mismatch_results() -> anyhow::Result<()> {
        // (module (import "env" "f" (func (param i32) (result i32))))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            // type section: 1 type, (i32) -> (i32)
            b"\x01\x06\x01\x60\x01\x7f\x01\x7f",
            // import section: "env"."f" -> func type 0
            b"\x02\x09\x01\x03\x65\x6e\x76\x01\x66\x00\x00",
        ]
        .concat();

        let mut runtime = Runtime::default();
        runtime.register_host_fn(
            "env",
            "f",
            FuncType {
                params: vec![ValType::I32],
                results: vec![ValType::I64],
            },
            |_| Ok(vec![Value::I64(0)]),
        )?;

        let r = runtime.load_module(&bytes);
        assert!(
            matches!(
                r,
                Err(Error::Unlinkable(UnlinkableError::IncompatibleImportType))
            ),
            "expected IncompatibleImportType, got {r:?}",
        );
        Ok(())
    }

    #[test]
    fn test_import_func_kind_mismatch() -> anyhow::Result<()> {
        // (module (import "env" "x" (func (result i32))))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            // type section: 1 type, () -> (i32)
            b"\x01\x05\x01\x60\x00\x01\x7f",
            // import section: "env"."x" -> func type 0
            b"\x02\x09\x01\x03\x65\x6e\x76\x01\x78\x00\x00",
        ]
        .concat();

        let mut runtime = Runtime::default();
        runtime.register_host_global(
            "env",
            "x",
            GlobalType {
                type_: ValType::I32,
                mutflag: MutabilityFlag::Const,
            },
            Value::I32(0),
        )?;

        let r = runtime.load_module(&bytes);
        assert!(
            matches!(
                r,
                Err(Error::Unlinkable(UnlinkableError::IncompatibleImportType))
            ),
            "expected IncompatibleImportType, got {r:?}",
        );
        Ok(())
    }

    #[test]
    fn test_import_table_elemtype_mismatch() -> anyhow::Result<()> {
        // (module (import "env" "t" (table 1 funcref)))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            // import section: "env"."t" -> table funcref, {min: 1}
            b"\x02\x0b\x01\x03\x65\x6e\x76\x01\x74\x01\x70\x00\x01",
        ]
        .concat();

        let mut runtime = Runtime::default();
        runtime.register_host_table(
            "env",
            "t",
            TableType {
                elemtype: RefType::Extern,
                limit: Limit { min: 1, max: None },
            },
            Ref::Null(RefType::Extern),
        )?;

        let r = runtime.load_module(&bytes);
        assert!(
            matches!(
                r,
                Err(Error::Unlinkable(UnlinkableError::IncompatibleImportType))
            ),
            "expected IncompatibleImportType, got {r:?}",
        );
        Ok(())
    }

    #[test]
    fn test_import_table_min_too_small() -> anyhow::Result<()> {
        // (module (import "env" "t" (table 5 funcref)))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            // import section: "env"."t" -> table funcref, {min: 5}
            b"\x02\x0b\x01\x03\x65\x6e\x76\x01\x74\x01\x70\x00\x05",
        ]
        .concat();

        let mut runtime = Runtime::default();
        runtime.register_host_table(
            "env",
            "t",
            TableType {
                elemtype: RefType::Func,
                limit: Limit { min: 2, max: None },
            },
            Ref::Null(RefType::Func),
        )?;

        let r = runtime.load_module(&bytes);
        assert!(
            matches!(
                r,
                Err(Error::Unlinkable(UnlinkableError::IncompatibleImportType))
            ),
            "expected IncompatibleImportType, got {r:?}",
        );
        Ok(())
    }

    #[test]
    fn test_import_table_max_required_but_absent() -> anyhow::Result<()> {
        // (module (import "env" "t" (table 1 10 funcref)))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            // import section: "env"."t" -> table funcref, {min: 1, max: 10}
            b"\x02\x0c\x01\x03\x65\x6e\x76\x01\x74\x01\x70\x01\x01\x0a",
        ]
        .concat();

        let mut runtime = Runtime::default();
        runtime.register_host_table(
            "env",
            "t",
            TableType {
                elemtype: RefType::Func,
                limit: Limit { min: 1, max: None },
            },
            Ref::Null(RefType::Func),
        )?;

        let r = runtime.load_module(&bytes);
        assert!(
            matches!(
                r,
                Err(Error::Unlinkable(UnlinkableError::IncompatibleImportType))
            ),
            "expected IncompatibleImportType, got {r:?}",
        );
        Ok(())
    }

    #[test]
    fn test_import_table_max_too_large() -> anyhow::Result<()> {
        // (module (import "env" "t" (table 1 5 funcref)))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            // import section: "env"."t" -> table funcref, {min: 1, max: 5}
            b"\x02\x0c\x01\x03\x65\x6e\x76\x01\x74\x01\x70\x01\x01\x05",
        ]
        .concat();

        let mut runtime = Runtime::default();
        runtime.register_host_table(
            "env",
            "t",
            TableType {
                elemtype: RefType::Func,
                limit: Limit {
                    min: 1,
                    max: Some(10),
                },
            },
            Ref::Null(RefType::Func),
        )?;

        let r = runtime.load_module(&bytes);
        assert!(
            matches!(
                r,
                Err(Error::Unlinkable(UnlinkableError::IncompatibleImportType))
            ),
            "expected IncompatibleImportType, got {r:?}",
        );
        Ok(())
    }

    #[test]
    fn test_import_memory_min_too_small() -> anyhow::Result<()> {
        // (module (import "env" "m" (memory 4)))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            // import section: "env"."m" -> memory {min: 4}
            b"\x02\x0a\x01\x03\x65\x6e\x76\x01\x6d\x02\x00\x04",
        ]
        .concat();

        let mut runtime = Runtime::default();
        runtime.register_host_memory("env", "m", MemType(Limit { min: 1, max: None }))?;

        let r = runtime.load_module(&bytes);
        assert!(
            matches!(
                r,
                Err(Error::Unlinkable(UnlinkableError::IncompatibleImportType))
            ),
            "expected IncompatibleImportType, got {r:?}",
        );
        Ok(())
    }

    #[test]
    fn test_import_memory_max_required_but_absent() -> anyhow::Result<()> {
        // (module (import "env" "m" (memory 1 8)))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            // import section: "env"."m" -> memory {min: 1, max: 8}
            b"\x02\x0b\x01\x03\x65\x6e\x76\x01\x6d\x02\x01\x01\x08",
        ]
        .concat();

        let mut runtime = Runtime::default();
        runtime.register_host_memory("env", "m", MemType(Limit { min: 1, max: None }))?;

        let r = runtime.load_module(&bytes);
        assert!(
            matches!(
                r,
                Err(Error::Unlinkable(UnlinkableError::IncompatibleImportType))
            ),
            "expected IncompatibleImportType, got {r:?}",
        );
        Ok(())
    }

    #[test]
    fn test_import_memory_max_too_large() -> anyhow::Result<()> {
        // (module (import "env" "m" (memory 1 4)))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            // import section: "env"."m" -> memory {min: 1, max: 4}
            b"\x02\x0b\x01\x03\x65\x6e\x76\x01\x6d\x02\x01\x01\x04",
        ]
        .concat();

        let mut runtime = Runtime::default();
        runtime.register_host_memory(
            "env",
            "m",
            MemType(Limit {
                min: 1,
                max: Some(8),
            }),
        )?;

        let r = runtime.load_module(&bytes);
        assert!(
            matches!(
                r,
                Err(Error::Unlinkable(UnlinkableError::IncompatibleImportType))
            ),
            "expected IncompatibleImportType, got {r:?}",
        );
        Ok(())
    }

    #[test]
    fn test_import_global_type_mismatch() -> anyhow::Result<()> {
        // (module (import "env" "g" (global i32)))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            // import section: "env"."g" -> global i32 const
            b"\x02\x0a\x01\x03\x65\x6e\x76\x01\x67\x03\x7f\x00",
        ]
        .concat();

        let mut runtime = Runtime::default();
        runtime.register_host_global(
            "env",
            "g",
            GlobalType {
                type_: ValType::I64,
                mutflag: MutabilityFlag::Const,
            },
            Value::I64(0),
        )?;

        let r = runtime.load_module(&bytes);
        assert!(
            matches!(
                r,
                Err(Error::Unlinkable(UnlinkableError::IncompatibleImportType))
            ),
            "expected IncompatibleImportType, got {r:?}",
        );
        Ok(())
    }

    #[test]
    fn test_import_global_mut_mismatch() -> anyhow::Result<()> {
        // (module (import "env" "g" (global (mut i32))))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            // import section: "env"."g" -> global i32 var
            b"\x02\x0a\x01\x03\x65\x6e\x76\x01\x67\x03\x7f\x01",
        ]
        .concat();

        let mut runtime = Runtime::default();
        runtime.register_host_global(
            "env",
            "g",
            GlobalType {
                type_: ValType::I32,
                mutflag: MutabilityFlag::Const,
            },
            Value::I32(0),
        )?;

        let r = runtime.load_module(&bytes);
        assert!(
            matches!(
                r,
                Err(Error::Unlinkable(UnlinkableError::IncompatibleImportType))
            ),
            "expected IncompatibleImportType, got {r:?}",
        );
        Ok(())
    }

    #[test]
    fn test_import_table_wider_min_ok() -> anyhow::Result<()> {
        // (module (import "env" "t" (table 1 funcref)))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            // import section: "env"."t" -> table funcref, {min: 1}
            b"\x02\x0b\x01\x03\x65\x6e\x76\x01\x74\x01\x70\x00\x01",
        ]
        .concat();

        let mut runtime = Runtime::default();
        runtime.register_host_table(
            "env",
            "t",
            TableType {
                elemtype: RefType::Func,
                limit: Limit { min: 4, max: None },
            },
            Ref::Null(RefType::Func),
        )?;
        runtime.load_module(&bytes)?;
        Ok(())
    }
}
