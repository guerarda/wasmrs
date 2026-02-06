use core::{error, fmt};
use std::{iter::repeat_n, result};

use crate::{
    Error, Module,
    binary::module,
    runtime::{
        instance::{ModuleHandle, ModuleInstance, ModuleRegistry},
        stack::Frame,
        store::{Func, FuncAddr, FuncInstance, Store},
        value::{ExternVal, Value},
    },
};

pub mod executor;
pub mod instance;
pub mod stack;
pub mod store;
pub mod value;

#[derive(Debug, Default)]
pub struct Runtime {
    call_stack: Vec<Frame>,
    value_stack: Vec<Value>,

    store: Store,
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

        self.module_registry.register(h, mi);
        h
    }

    pub fn invoke(
        &mut self,
        module: ModuleHandle,
        fn_name: &str,
        fn_args: &[Value],
    ) -> result::Result<Vec<Value>, Error> {
        let mi = self.module_registry.get_instance(module);
        let funcaddr = mi.exports.get(fn_name).unwrap().try_into().unwrap();
        let arity = self.store.get_func(funcaddr).ftype.results.len();

        let base = self.value_stack.len();
        self.value_stack.extend_from_slice(fn_args);

        self.call(funcaddr);
        self.execute()
            .map_err(|e| e.with_stacks(self.call_stack.clone(), self.value_stack.clone()))?;

        if self.value_stack.len() < arity {
            return Err(RuntimeError::internal().into());
        }
        let idx = self.value_stack.len() - arity;
        let results = self.value_stack.split_off(idx);
        debug_assert!(
            self.value_stack.len() == base,
            "value stack not fully unwound after function invocation"
        );
        Ok(results)
    }

    fn call(&mut self, funcaddr: FuncAddr) {
        let func_instance = self.store.get_func(funcaddr);
        let n_args = func_instance.ftype.params.len();
        let arity = func_instance.ftype.results.len() as u32;
        let sp = self.value_stack.len() - n_args;

        let mut locals: Vec<Value> = self.value_stack.split_off(sp);
        locals.extend(
            func_instance
                .func
                .locals
                .clone()
                .into_iter()
                .map(Into::<Value>::into),
        );

        self.call_stack
            .push(Frame::new(arity, funcaddr, locals, sp));
    }

    /// Decode and instantiate a module from bytes
    pub fn load_module(&mut self, bytes: &[u8]) -> std::result::Result<ModuleHandle, Error> {
        let module = module::decode_bytes(bytes.to_vec())?;
        //validation::validate_module(&module)?;
        let handle = self.instantiate_module(&module);
        Ok(handle)
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
        write!(f, "trap error:\n {self:?}")
    }
}

impl RuntimeError {
    fn trap() -> Self {
        Self {
            kind: RuntimeErrorKind::Trap,
            pc: -1,
            instruction: "",
            call_stack: vec![],
            value_stack: vec![],
            fn_name: None,
        }
    }

    fn internal() -> Self {
        Self {
            kind: RuntimeErrorKind::Internal,
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
    Trap,
    Internal,
}

impl error::Error for RuntimeErrorKind {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}

impl fmt::Display for RuntimeErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Trap => write!(f, "trap"),
            Self::Internal => write!(f, "internal"),
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
