use std::{error, fmt, result};

use const_expr::ConstExpressionValidationError;
use sections::global::GlobalValidationError;
use sections::import::ImportValidationError;
use sections::memory::MemoryValidationError;
use sections::table::TableValidationError;

use crate::binary::module::Module;
use crate::binary::sections::code::{CodeEntry, FuncLocal};
use crate::binary::sections::element::ElementSegmentItems;
use crate::binary::sections::export::ExportDesc;
use crate::binary::sections::global::{GlobalType, MutabilityFlag};
use crate::binary::sections::import::ImportDesc;
use crate::binary::sections::memory::MemType;
use crate::binary::sections::table::TableType;
use crate::binary::types::{
    BlockType, DataIdx, FuncIndex, FuncType, GlobalIndex, MemArg, MemIndex, RefType, TableIndex,
    TypeIdx, ValType,
};
use crate::instructions::Instruction;
use crate::limits::MAX_WASM_32BIT_MEMORY_PAGES;
use crate::validation::sections::element::ElementValidationError;
use crate::validation::sections::export::ExportValidationError;

mod const_expr;
mod sections;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ValueType {
    I32,
    I64,
    F32,
    F64,
    V128,
    Ref(RefType),
    Unknown,
}

impl ValueType {
    fn is_num(&self) -> bool {
        matches!(self, Self::I32 | Self::I64 | Self::F32 | Self::F64)
    }

    fn is_vec(&self) -> bool {
        matches!(self, Self::V128)
    }

    fn is_ref(&self) -> bool {
        matches!(self, Self::Ref(_))
    }
}

impl From<&ValType> for ValueType {
    fn from(value: &ValType) -> Self {
        match value {
            ValType::Ref(rt) => Self::Ref(*rt),
            ValType::V128 => Self::V128,
            ValType::F64 => Self::F64,
            ValType::F32 => Self::F32,
            ValType::I64 => Self::I64,
            ValType::I32 => Self::I32,
        }
    }
}

impl From<ValType> for ValueType {
    fn from(value: ValType) -> Self {
        Self::from(&value)
    }
}

impl From<RefType> for ValueType {
    fn from(value: RefType) -> Self {
        Self::from(ValType::Ref(value))
    }
}

impl From<&GlobalType> for ValueType {
    fn from(value: &GlobalType) -> Self {
        Self::from(value.type_)
    }
}

impl fmt::Display for ValueType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::I32 => write!(f, "i32"),
            Self::I64 => write!(f, "i64"),
            Self::F32 => write!(f, "f32"),
            Self::F64 => write!(f, "f64"),
            Self::V128 => write!(f, "v128"),
            Self::Ref(RefType::Func) => write!(f, "ref.func"),
            Self::Ref(RefType::Extern) => write!(f, "ref.extern"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

#[derive(Debug)]
pub struct CtrlFrame {
    opcode: Instruction,
    start_types: Vec<ValueType>,
    end_types: Vec<ValueType>,
    height: usize,
    unreachable: bool,
}

#[derive(Debug, Default)]
struct Validator {
    vals: Vec<ValueType>,
    ctrls: Vec<CtrlFrame>,
}

#[derive(Debug)]
#[non_exhaustive]
pub enum ValidationError {
    Module(ValidationErrorKind),
    Table {
        index: usize,
        source: TableValidationError,
    },
    Memory {
        index: usize,
        source: MemoryValidationError,
    },
    Import {
        index: usize,
        source: ImportValidationError,
    },
    Global {
        index: usize,
        source: GlobalValidationError,
    },
    Function {
        index: usize,
        source: FunctionValidationError,
    },
    Element {
        index: usize,
        source: ElementValidationError,
    },
    Export {
        index: usize,
        source: ExportValidationError,
    },
}

impl error::Error for ValidationError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match &self {
            Self::Module(e) => Some(e),
            Self::Function { index: _, source } => Some(source),
            Self::Table { index: _, source } => Some(source),
            Self::Memory { index: _, source } => Some(source),
            Self::Global { index: _, source } => Some(source),
            Self::Import { index: _, source } => Some(source),
            Self::Element { index: _, source } => Some(source),
            Self::Export { index: _, source } => Some(source),
        }
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self {
            Self::Module(_) => write!(f, "invalid module"),
            Self::Function { index, .. } => write!(f, "validating function at index {index}"),
            Self::Table { index, .. } => write!(f, "validating table at index {index}"),
            Self::Global { index, .. } => write!(f, "validating global at index {index}"),
            Self::Memory { index, .. } => write!(f, "validating memory at index {index}"),
            Self::Import { index, .. } => write!(f, "validating import at index {index}"),
            Self::Element { index, .. } => {
                write!(f, "validating elements segment at index {index}")
            }

            Self::Export { index, .. } => write!(f, "validating export at index {index}"),
        }
    }
}

impl From<ValidationErrorKind> for ValidationError {
    fn from(value: ValidationErrorKind) -> Self {
        ValidationError::Module(value)
    }
}

/// Function Validation Error
#[derive(Debug)]
#[non_exhaustive]
pub enum FunctionValidationError {
    Instruction {
        index: usize,
        name: &'static str,
        kind: ValidationErrorKind,
    },
    UnknownType(IndexValidationError),
}

impl error::Error for FunctionValidationError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match &self {
            Self::Instruction {
                index: _,
                name: _,
                kind,
            } => Some(kind),
            Self::UnknownType(e) => Some(e),
        }
    }
}

impl fmt::Display for FunctionValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self {
            Self::Instruction { index, name, .. } => {
                write!(f, "validating instruction {index} ({name})")
            }
            Self::UnknownType(_) => write!(f, "function type refers to an invalid type index"),
        }
    }
}

/// Index Validation Error
#[derive(Debug)]
pub enum IndexSpace {
    Type,
    Function,
    Table,
    Memory,
    Global,
    Data,
    Element,
    Local,
}

impl fmt::Display for IndexSpace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Type => write!(f, "type"),
            Self::Function => write!(f, "function"),
            Self::Table => write!(f, "table"),
            Self::Memory => write!(f, "memory"),
            Self::Global => write!(f, "global"),
            Self::Data => write!(f, "data"),
            Self::Element => write!(f, "element"),
            Self::Local => write!(f, "local"),
        }
    }
}

#[derive(Debug)]
pub struct IndexValidationError {
    space: IndexSpace,
    index: usize,
    count: usize,
}

impl IndexValidationError {
    fn mint(space: IndexSpace, index: usize, count: usize) -> Self {
        #[cfg(feature = "debug-errors")]
        dbg_breakpoint::breakpoint_if_debugging();
        Self {
            space,
            index,
            count,
        }
    }

    fn data(index: DataIdx, count: usize) -> Self {
        Self::mint(IndexSpace::Data, index as usize, count)
    }

    fn element(index: u32, count: usize) -> Self {
        Self::mint(IndexSpace::Element, index as usize, count)
    }

    fn function(index: FuncIndex, count: usize) -> Self {
        Self::mint(IndexSpace::Function, index.as_usize(), count)
    }

    fn global(index: GlobalIndex, count: usize) -> Self {
        Self::mint(IndexSpace::Global, index.0 as usize, count)
    }

    fn local(index: u32, count: usize) -> Self {
        Self::mint(IndexSpace::Local, index as usize, count)
    }

    fn memory(index: MemIndex, count: usize) -> Self {
        Self::mint(IndexSpace::Memory, index.0 as usize, count)
    }

    fn table(index: TableIndex, count: usize) -> Self {
        Self::mint(IndexSpace::Table, index.as_usize(), count)
    }

    fn ty(index: TypeIdx, count: usize) -> Self {
        Self::mint(IndexSpace::Type, index as usize, count)
    }
}

impl error::Error for IndexValidationError {}
impl fmt::Display for IndexValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} index {} out of range ({} available)",
            self.space, self.index, self.count
        )
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum LimitValidationError {
    MinExceedsMax(u32, u32),
    MinExceedsRange { min: u32, limit: u32 },
    MaxExceedsRange { max: u32, limit: u32 },
}

impl error::Error for LimitValidationError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        None
    }
}

impl fmt::Display for LimitValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MinExceedsMax(min, max) => write!(f, "limit min ({min}) exceeds max ({max})"),
            Self::MinExceedsRange { min, limit } => {
                write!(f, "limit min ({min}) exceeds allowed max ({limit})")
            }
            Self::MaxExceedsRange { max, limit } => {
                write!(f, "limit max ({max}) exceeds allowed max ({limit})")
            }
        }
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum ValidationErrorKind {
    ControlStackUnderflow,
    ValueStackUnderflow,
    TypeMismatch,
    TableTypeMismatch,
    ElseWithoutMatchingIf,
    UndeclaredFunctionReference,
    ImmutableGlobal,
    InvalidSelectTypes,
    InvalidMemAlignment,
    InvalidElement,
    ConstantExpressionRequired,
    DuplicatExportName,
    FunctionCodeMismatch,
    StartFunction,
    ConstExprError {
        source: Box<ConstExpressionValidationError>,
    },
    InvalidIndex(IndexValidationError),
}

impl error::Error for ValidationErrorKind {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::ConstExprError { source } => Some(source),
            Self::InvalidIndex(e) => Some(e),
            _ => None,
        }
    }
}

impl fmt::Display for ValidationErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ControlStackUnderflow => write!(f, "control stack underflow"),
            Self::ValueStackUnderflow => write!(f, "value stack underflow"),
            Self::TypeMismatch => write!(f, "type mismatch"),
            Self::TableTypeMismatch => write!(f, "table type mismatch"),
            Self::ElseWithoutMatchingIf => write!(f, "else without matching if"),
            Self::UndeclaredFunctionReference => write!(f, "undeclared function reference"),
            Self::ImmutableGlobal => write!(f, "immutable global"),
            Self::InvalidSelectTypes => write!(f, "select types must have exactly one entry"),
            Self::InvalidMemAlignment => write!(f, "invalid mem alignment"),
            Self::InvalidElement => write!(f, "invalid element"),
            Self::ConstantExpressionRequired => write!(f, "constant expression required"),
            Self::DuplicatExportName => write!(f, "duplicate export name"),
            Self::FunctionCodeMismatch => write!(f, "function and code section mismatch"),
            Self::StartFunction => write!(f, "start function type"),
            Self::ConstExprError { .. } => write!(f, "CONST EXPR ERROR PLACEHOLDER"),
            Self::InvalidIndex(_) => write!(f, "invalid index"),
        }
    }
}

impl From<ConstExpressionValidationError> for ValidationErrorKind {
    fn from(value: ConstExpressionValidationError) -> Self {
        Self::ConstExprError {
            source: Box::new(value),
        }
    }
}

impl From<IndexValidationError> for ValidationErrorKind {
    fn from(value: IndexValidationError) -> Self {
        ValidationErrorKind::InvalidIndex(value)
    }
}

impl ValidationErrorKind {
    fn mint(e: ValidationErrorKind) -> ValidationErrorKind {
        #[cfg(feature = "debug-errors")]
        dbg_breakpoint::breakpoint_if_debugging();
        e
    }

    fn type_mismatch() -> Self {
        Self::mint(ValidationErrorKind::TypeMismatch)
    }

    fn value_stack_underflow() -> Self {
        Self::mint(ValidationErrorKind::ValueStackUnderflow)
    }

    fn control_stack_underflow() -> Self {
        Self::mint(ValidationErrorKind::ControlStackUnderflow)
    }

    fn table_type_mismatch() -> Self {
        Self::mint(ValidationErrorKind::TableTypeMismatch)
    }

    fn else_without_matching_if() -> Self {
        Self::mint(ValidationErrorKind::ElseWithoutMatchingIf)
    }

    fn undeclared_function_reference() -> Self {
        Self::mint(ValidationErrorKind::UndeclaredFunctionReference)
    }

    fn immutable_global() -> Self {
        Self::mint(ValidationErrorKind::ImmutableGlobal)
    }

    fn invalid_select_types() -> Self {
        Self::mint(ValidationErrorKind::InvalidSelectTypes)
    }

    fn invalid_mem_alignment() -> Self {
        Self::mint(ValidationErrorKind::InvalidMemAlignment)
    }

    fn function_code_mismatch() -> Self {
        Self::mint(ValidationErrorKind::FunctionCodeMismatch)
    }

    fn start_function() -> Self {
        Self::mint(ValidationErrorKind::StartFunction)
    }
}

impl Validator {
    fn push_val(&mut self, val: ValueType) {
        self.vals.push(val)
    }

    fn push_vals(&mut self, vals: &[ValueType]) {
        self.vals.extend_from_slice(vals)
    }

    fn pop_vals_expect(
        &mut self,
        expected: &[ValueType],
    ) -> result::Result<Vec<ValueType>, ValidationErrorKind> {
        let mut res = vec![];
        for t in expected.iter().rev() {
            res.push(self.pop_val_expect(*t)?);
        }

        Ok(res)
    }

    fn pop_val(&mut self) -> result::Result<ValueType, ValidationErrorKind> {
        let last = self.ctrls.last().expect("unexpected empty control stack");
        if self.vals.len() == last.height && last.unreachable {
            return Ok(ValueType::Unknown);
        }

        if self.vals.len() == last.height {
            return Err(ValidationErrorKind::value_stack_underflow());
        }

        Ok(self.vals.pop().expect("unexpected empty value stack"))
    }

    fn pop_val_expect(
        &mut self,
        expected: ValueType,
    ) -> result::Result<ValueType, ValidationErrorKind> {
        let actual = self.pop_val()?;
        if actual != expected && actual != ValueType::Unknown && expected != ValueType::Unknown {
            return Err(ValidationErrorKind::type_mismatch());
        }
        Ok(actual)
    }

    fn pop_ref(&mut self) -> result::Result<ValueType, ValidationErrorKind> {
        let actual = self.pop_val()?;
        if actual != ValueType::Unknown && !actual.is_ref() {
            return Err(ValidationErrorKind::type_mismatch());
        }
        Ok(actual)
    }

    fn push_ctrl(&mut self, opcode: Instruction, start: Vec<ValueType>, end: Vec<ValueType>) {
        let height = self.vals.len();
        self.push_vals(&start);
        self.ctrls.push(CtrlFrame {
            opcode,
            start_types: start,
            end_types: end,
            height,
            unreachable: false,
        });
    }

    fn pop_ctrl(&mut self) -> result::Result<CtrlFrame, ValidationErrorKind> {
        let frame = self
            .ctrls
            .last()
            .ok_or_else(ValidationErrorKind::control_stack_underflow)?;
        let height = frame.height;

        // need to clone here because pop_vals_expect below
        // needs a mutable ref
        let end_types = frame.end_types.clone();

        let _ = self.pop_vals_expect(&end_types)?;
        if self.vals.len() != height {
            return Err(ValidationErrorKind::type_mismatch());
        }
        Ok(self.ctrls.pop().expect("unexpected empty control stack"))
    }

    /// Marks the current frame as unreachable
    fn unreachable(&mut self) {
        let last = self
            .ctrls
            .last_mut()
            .expect("unexpected empty control stack");
        self.vals.resize(last.height, ValueType::Unknown);
        last.unreachable = true;
    }

    /// Converts a Block Type into a ([t1*], [t2*])
    fn block_type(
        bt: &BlockType,
        module: &Module,
    ) -> result::Result<(Vec<ValueType>, Vec<ValueType>), ValidationErrorKind> {
        Ok(match bt {
            BlockType::Empty => (vec![], vec![]),
            BlockType::Value(vt) => (vec![], vec![vt.into()]),
            BlockType::Index(idx) => {
                let t = &module
                    .types
                    .as_ref()
                    .and_then(|t| t.get(*idx as usize))
                    .ok_or_else(|| IndexValidationError::ty(*idx, module.table_count()))?;

                (
                    t.params.iter().copied().map(|v| (&v).into()).collect(),
                    t.results.iter().copied().map(|v| (&v).into()).collect(),
                )
            }
        })
    }

    /// Returns the function type at the given type index (not function index)
    fn type_at(module: &Module, type_idx: u32) -> result::Result<&FuncType, IndexValidationError> {
        module
            .types
            .as_ref()
            .and_then(|types| types.get(type_idx as usize))
            .ok_or_else(|| IndexValidationError::ty(type_idx, module.type_count()))
    }

    /// Returns the type for an idx in the Function Section.
    // TODO cache the function index array with imports at the module level
    fn func_type_at(
        module: &Module,
        func_idx: FuncIndex,
    ) -> result::Result<&FuncType, IndexValidationError> {
        let imported = module
            .imports
            .iter()
            .flatten()
            .filter_map(|i| match i.desc {
                ImportDesc::Func(idx) => Some(idx),
                _ => None,
            });
        let defined = module.functions.iter().flatten().copied();

        let type_idx = imported
            .chain(defined)
            .nth(func_idx.as_usize())
            .ok_or_else(|| IndexValidationError::function(func_idx, module.func_count()))?;

        Self::type_at(module, type_idx)
    }

    /// Returns the table type at the given index
    // TODO cache the function index array with imports at the module level
    fn table_type_at(
        module: &Module,
        idx: TableIndex,
    ) -> result::Result<&TableType, IndexValidationError> {
        let imported = module
            .imports
            .iter()
            .flatten()
            .filter_map(|i| match &i.desc {
                ImportDesc::Table(table_type) => Some(table_type),
                _ => None,
            });
        let defined = module.tables.iter().flatten().map(|t| &t.tabletype);

        imported
            .chain(defined)
            .nth(idx.as_usize())
            .ok_or_else(|| IndexValidationError::table(idx, module.table_count()))
    }

    /// Returns the global type at the given index
    // TODO cache the function index array with imports at the module level
    fn global_type_at(
        module: &Module,
        idx: GlobalIndex,
    ) -> result::Result<&GlobalType, IndexValidationError> {
        let imported = module
            .imports
            .iter()
            .flatten()
            .filter_map(|i| match &i.desc {
                ImportDesc::Global(global_type) => Some(global_type),
                _ => None,
            });

        let defined = module.globals.iter().flatten().map(|g| &g.gt);

        imported
            .chain(defined)
            .nth(idx.0 as usize)
            .ok_or_else(|| IndexValidationError::global(idx, module.global_count()))
    }

    fn mem_type_at(
        module: &Module,
        mem_idx: MemIndex,
    ) -> result::Result<&MemType, IndexValidationError> {
        let imported = module
            .imports
            .iter()
            .flatten()
            .filter_map(|i| match &i.desc {
                ImportDesc::Mem(mem_type) => Some(mem_type),
                _ => None,
            });
        let defined = module.memories.iter().flatten();

        imported
            .chain(defined)
            .nth(mem_idx.0 as usize)
            .ok_or_else(|| IndexValidationError::memory(mem_idx, module.memory_count()))
    }

    /// Return the type of the function local at the given index
    fn local_type(
        functype: &FuncType,
        locals: &[FuncLocal],
        idx: u32,
    ) -> result::Result<ValueType, IndexValidationError> {
        if let Some(param) = functype.params.get(idx as usize) {
            return Ok(param.into());
        }
        let local_idx = idx - functype.params.len() as u32;
        let mut count: u32 = 0;

        for local in locals {
            if local_idx < count + local.count {
                return Ok(local.valtype.into());
            }
            count += local.count;
        }
        Err(IndexValidationError::local(
            idx,
            count as usize + functype.params.len(),
        ))
    }

    /// Returns the label_types for the nth frame from the top
    fn label_types(n: usize, frames: &[CtrlFrame]) -> &[ValueType] {
        let frame = &frames[frames.len() - n - 1];
        if matches!(frame.opcode, Instruction::Loop(_)) {
            &frame.start_types
        } else {
            &frame.end_types
        }
    }

    /// Validate that the memory alignment does not exceed the natural
    /// alignment for the given byte size
    fn validate_mem_alignment(
        memarg: &MemArg,
        byte_size: u32,
    ) -> result::Result<(), ValidationErrorKind> {
        if (1 << memarg.align) > byte_size {
            Err(ValidationErrorKind::invalid_mem_alignment())
        } else {
            Ok(())
        }
    }

    fn validate_binop(&mut self, valtype: ValType) -> result::Result<(), ValidationErrorKind> {
        // [t t] -> [t]
        let valtype = valtype.into();
        self.pop_val_expect(valtype)?;
        self.pop_val_expect(valtype)?;
        self.push_val(valtype);
        Ok(())
    }

    fn validate_unop(&mut self, valtype: ValType) -> result::Result<(), ValidationErrorKind> {
        // [t] -> [t]
        let valtype = valtype.into();
        self.pop_val_expect(valtype)?;
        self.push_val(valtype);
        Ok(())
    }

    fn validate_convop(
        &mut self,
        from_valtype: ValType,
        to_valtype: ValType,
    ) -> result::Result<(), ValidationErrorKind> {
        // [t1] -> [t2]
        let from_valtype = from_valtype.into();
        let to_valtype = to_valtype.into();
        self.pop_val_expect(from_valtype)?;
        self.push_val(to_valtype);
        Ok(())
    }

    fn validate_compop(&mut self, valtype: ValType) -> result::Result<(), ValidationErrorKind> {
        // [t t] -> [i32]
        let valtype = valtype.into();
        self.pop_val_expect(valtype)?;
        self.pop_val_expect(valtype)?;
        self.push_val(ValueType::I32);
        Ok(())
    }

    fn validate_mem_load(
        &mut self,
        module: &Module,
        memarg: &MemArg,
        byte_size: u32,
        valuetype: ValType,
    ) -> result::Result<(), ValidationErrorKind> {
        // mems[0] is defined in the context
        let _ = Self::mem_type_at(module, MemIndex::ZERO)?;

        // alignment not larger than bit width
        Self::validate_mem_alignment(memarg, byte_size)?;

        // [i32] -> [t]
        self.pop_val_expect(ValueType::I32)?;
        self.push_val(valuetype.into());
        Ok(())
    }

    fn validate_mem_store(
        &mut self,
        module: &Module,
        memarg: &MemArg,
        byte_size: u32,
        valuetype: ValType,
    ) -> result::Result<(), ValidationErrorKind> {
        // mems[0] is defined in the context
        let _ = Self::mem_type_at(module, MemIndex::ZERO)?;

        // alignment not larger than bit width
        Self::validate_mem_alignment(memarg, byte_size)?;

        // [i32 t] -> []
        self.pop_val_expect(valuetype.into())?;
        self.pop_val_expect(ValueType::I32)?;
        Ok(())
    }

    fn validate_memory_limit(memtype: &MemType) -> result::Result<(), LimitValidationError> {
        let l = &memtype.0;
        if l.min > MAX_WASM_32BIT_MEMORY_PAGES {
            return Err(LimitValidationError::MinExceedsRange {
                min: l.min,
                limit: MAX_WASM_32BIT_MEMORY_PAGES,
            });
        }
        if let Some(max) = l.max {
            if max > MAX_WASM_32BIT_MEMORY_PAGES {
                return Err(LimitValidationError::MaxExceedsRange {
                    max,
                    limit: MAX_WASM_32BIT_MEMORY_PAGES,
                });
            }
            if l.min > max {
                return Err(LimitValidationError::MinExceedsMax(l.min, max));
            }
        }
        Ok(())
    }

    fn validate_function(
        &mut self,
        functype: &FuncType,
        entry: &CodeEntry,
        module: &Module,
        declared_refs: &[FuncIndex],
    ) -> result::Result<(), FunctionValidationError> {
        let end_types: Vec<ValueType> = functype.results.iter().map(ValueType::from).collect();
        self.push_ctrl(Instruction::Call(FuncIndex(0)), vec![], end_types);

        for (idx, inst) in entry.body.iter().enumerate() {
            self.validate_instruction(inst, functype, entry, module, declared_refs)
                .map_err(|e| FunctionValidationError::Instruction {
                    index: idx,
                    name: inst.name(),
                    kind: e,
                })?;
        }
        Ok(())
    }

    fn validate_instruction(
        &mut self,
        instruction: &Instruction,
        functype: &FuncType,
        entry: &CodeEntry,
        module: &Module,
        declared_refs: &[FuncIndex],
    ) -> result::Result<(), ValidationErrorKind> {
        match instruction {
            Instruction::Unreachable => self.unreachable(),
            Instruction::Nop => (),
            Instruction::Block(bt) => {
                let (t1, t2) = Self::block_type(bt, module)?;
                self.pop_vals_expect(&t1)?;
                self.push_ctrl(Instruction::Block(*bt), t1, t2);
            }
            Instruction::Loop(bt) => {
                let (t1, t2) = Self::block_type(bt, module)?;
                self.pop_vals_expect(&t1)?;
                self.push_ctrl(Instruction::Loop(*bt), t1, t2);
            }
            Instruction::If(bt) => {
                // [t1* i32] -> [t2*]
                let (t1, t2) = Self::block_type(bt, module)?;

                self.pop_val_expect(ValueType::I32)?;
                self.pop_vals_expect(&t1)?;

                self.push_ctrl(Instruction::If(*bt), t1, t2);
            }
            Instruction::Else => {
                let frame = self.pop_ctrl()?;
                if !matches!(frame.opcode, Instruction::If(_)) {
                    return Err(ValidationErrorKind::else_without_matching_if());
                }
                self.push_ctrl(Instruction::Else, frame.start_types, frame.end_types)
            }
            Instruction::End => {
                let frame = self.pop_ctrl()?;
                if matches!(frame.opcode, Instruction::If(_))
                    && frame.start_types != frame.end_types
                {
                    return Err(ValidationErrorKind::type_mismatch());
                }
                self.push_vals(&frame.end_types);
            }
            Instruction::Br(n) => {
                let n = *n as usize;
                if self.ctrls.len() <= n {
                    return Err(ValidationErrorKind::control_stack_underflow());
                }
                // FIXME Avoid Copy. pop_vals must be in ValueStack impl
                let lt = Self::label_types(n, &self.ctrls).to_vec();
                self.pop_vals_expect(&lt)?;
                self.unreachable();
            }
            Instruction::BrIf(n) => {
                let n = *n as usize;
                if self.ctrls.len() <= n {
                    return Err(ValidationErrorKind::control_stack_underflow());
                }
                self.pop_val_expect(ValueType::I32)?;

                // FIXME Avoid Copy. pop_vals must be in ValueStack impl
                let lt = Self::label_types(n, &self.ctrls).to_vec();
                self.pop_vals_expect(&lt)?;
                self.push_vals(&lt);
            }
            Instruction::BrTable(idx) => {
                self.pop_val_expect(ValueType::I32)?;

                // Verify the default target exists and get its arity
                let m = idx.default as usize;
                if self.ctrls.len() <= m {
                    return Err(ValidationErrorKind::control_stack_underflow());
                }
                let m_types = Self::label_types(m, &self.ctrls).to_vec();
                let arity = m_types.len();

                for n in &idx.labels {
                    // For each possible target, verify the arity matches
                    // the default target's. And that its types are
                    // compatible with what's currently on the stack.
                    let n = *n as usize;
                    if self.ctrls.len() <= n {
                        return Err(ValidationErrorKind::control_stack_underflow());
                    }
                    let n_types = Self::label_types(n, &self.ctrls).to_vec();
                    if n_types.len() != arity {
                        return Err(ValidationErrorKind::type_mismatch());
                    }
                    let mut actual = self.pop_vals_expect(&n_types)?;
                    actual.reverse();
                    self.push_vals(&actual);
                }

                self.pop_vals_expect(&m_types)?;
                self.unreachable();
            }
            Instruction::Return => {
                let results = self
                    .ctrls
                    .first()
                    .expect("unexpected empty control stack")
                    .end_types
                    .clone();

                self.pop_vals_expect(&results)?;
                self.unreachable();
            }
            Instruction::Call(idx) => {
                // [t1*] -> [t2*]
                let func_type = Self::func_type_at(module, *idx)?;
                let params: Vec<ValueType> =
                    func_type.params.iter().copied().map(|v| v.into()).collect();
                self.pop_vals_expect(&params)?;

                let results: Vec<ValueType> = func_type
                    .results
                    .iter()
                    .copied()
                    .map(|v| v.into())
                    .collect();

                self.push_vals(&results);
            }
            Instruction::CallIndirect((type_idx, table_idx)) => {
                // [t1* i32] -> [t2*]
                let table_type = Self::table_type_at(module, *table_idx)?;
                if !matches!(table_type.elemtype, RefType::Func) {
                    return Err(ValidationErrorKind::table_type_mismatch());
                }

                self.pop_val_expect(ValueType::I32)?;

                let func_type = Self::type_at(module, *type_idx)?;
                let params: Vec<ValueType> =
                    func_type.params.iter().copied().map(|v| v.into()).collect();
                self.pop_vals_expect(&params)?;

                let results: Vec<ValueType> = func_type
                    .results
                    .iter()
                    .copied()
                    .map(|v| v.into())
                    .collect();

                self.push_vals(&results);
            }

            Instruction::Drop => {
                // [t] -> []
                self.pop_val()?;
            }
            Instruction::Select => {
                // [t t i32] -> [t]
                self.pop_val_expect(ValueType::I32)?;
                let t1 = self.pop_val()?;
                let t2 = self.pop_val()?;

                let is_num_or_vec =
                    |t: &ValueType| t.is_num() || t.is_vec() || *t == ValueType::Unknown;

                if !is_num_or_vec(&t1) || !is_num_or_vec(&t2) {
                    return Err(ValidationErrorKind::type_mismatch());
                }

                if t1 != t2 && t1 != ValueType::Unknown && t2 != ValueType::Unknown {
                    return Err(ValidationErrorKind::type_mismatch());
                }

                self.push_val(if t1 == ValueType::Unknown { t2 } else { t1 });
            }
            Instruction::SelectT(vt) => {
                // [t t i32] -> [t]
                let [t] = vt.as_slice() else {
                    return Err(ValidationErrorKind::invalid_select_types());
                };

                let t = t.into();
                self.pop_val_expect(ValueType::I32)?;
                self.pop_val_expect(t)?;
                self.pop_val_expect(t)?;
                self.push_val(t);
            }
            Instruction::LocalGet(idx) => {
                // [] -> [t]
                let vt = Self::local_type(functype, &entry.locals, *idx)?;
                self.push_val(vt);
            }
            Instruction::LocalSet(idx) => {
                // [t] -> []
                let vt = Self::local_type(functype, &entry.locals, *idx)?;
                self.pop_val_expect(vt)?;
            }
            Instruction::LocalTee(idx) => {
                // [t] -> [t]
                let vt = Self::local_type(functype, &entry.locals, *idx)?;
                self.pop_val_expect(vt)?;
                self.push_val(vt);
            }
            Instruction::GlobalGet(idx) => {
                // [] -> [t]
                let gt = Self::global_type_at(module, *idx)?;
                self.push_val(gt.into());
            }
            Instruction::GlobalSet(idx) => {
                // [t] -> []
                let gt = Self::global_type_at(module, *idx)?;
                if matches!(gt.mutflag, MutabilityFlag::Const) {
                    return Err(ValidationErrorKind::immutable_global());
                }
                self.pop_val_expect(gt.into())?;
            }
            Instruction::TableGet(idx) => {
                // [at] -> [t]
                self.pop_val_expect(ValueType::I32)?;
                let t = Self::table_type_at(module, *idx)?;
                self.push_val(t.elemtype.into());
            }
            Instruction::TableSet(idx) => {
                // [at t] -> []
                let t = Self::table_type_at(module, *idx)?;
                self.pop_val_expect(t.elemtype.into())?;
                self.pop_val_expect(ValueType::I32)?;
            }
            Instruction::I32Load(memarg) => {
                self.validate_mem_load(module, memarg, 4, ValType::I32)?;
            }
            Instruction::I64Load(memarg) => {
                self.validate_mem_load(module, memarg, 8, ValType::I64)?;
            }
            Instruction::F32Load(memarg) => {
                self.validate_mem_load(module, memarg, 4, ValType::F32)?;
            }
            Instruction::F64Load(memarg) => {
                self.validate_mem_load(module, memarg, 8, ValType::F64)?;
            }
            Instruction::I32Load8S(memarg) | Instruction::I32Load8U(memarg) => {
                self.validate_mem_load(module, memarg, 1, ValType::I32)?;
            }
            Instruction::I32Load16S(memarg) | Instruction::I32Load16U(memarg) => {
                self.validate_mem_load(module, memarg, 2, ValType::I32)?;
            }
            Instruction::I64Load8S(memarg) | Instruction::I64Load8U(memarg) => {
                self.validate_mem_load(module, memarg, 1, ValType::I64)?;
            }
            Instruction::I64Load16S(memarg) | Instruction::I64Load16U(memarg) => {
                self.validate_mem_load(module, memarg, 2, ValType::I64)?;
            }
            Instruction::I64Load32S(memarg) | Instruction::I64Load32U(memarg) => {
                self.validate_mem_load(module, memarg, 4, ValType::I64)?;
            }
            Instruction::I32Store(memarg) => {
                self.validate_mem_store(module, memarg, 4, ValType::I32)?;
            }
            Instruction::I64Store(memarg) => {
                self.validate_mem_store(module, memarg, 8, ValType::I64)?;
            }
            Instruction::F32Store(memarg) => {
                self.validate_mem_store(module, memarg, 4, ValType::F32)?;
            }
            Instruction::F64Store(memarg) => {
                self.validate_mem_store(module, memarg, 8, ValType::F64)?;
            }
            Instruction::I32Store8(memarg) => {
                self.validate_mem_store(module, memarg, 1, ValType::I32)?;
            }
            Instruction::I32Store16(memarg) => {
                self.validate_mem_store(module, memarg, 2, ValType::I32)?;
            }
            Instruction::I64Store8(memarg) => {
                self.validate_mem_store(module, memarg, 1, ValType::I64)?;
            }
            Instruction::I64Store16(memarg) => {
                self.validate_mem_store(module, memarg, 2, ValType::I64)?;
            }
            Instruction::I64Store32(memarg) => {
                self.validate_mem_store(module, memarg, 4, ValType::I64)?;
            }
            Instruction::MemorySize(idx) => {
                // mems[0] is defined in the context
                let _ = Self::mem_type_at(module, *idx)?;

                // [] -> [i32]
                self.push_val(ValueType::I32);
            }
            Instruction::MemoryGrow(idx) => {
                // mems[0] is defined in the context
                let _ = Self::mem_type_at(module, *idx)?;

                // [i32] -> [i32]
                self.pop_val_expect(ValueType::I32)?;
                self.push_val(ValueType::I32);
            }
            Instruction::I32Const(_) => self.push_val(ValueType::I32),
            Instruction::I64Const(_) => self.push_val(ValueType::I64),
            Instruction::F32Const(_) => self.push_val(ValueType::F32),
            Instruction::F64Const(_) => self.push_val(ValueType::F64),
            Instruction::I32Eqz => {
                // [i32] -> [i32]
                self.pop_val_expect(ValueType::I32)?;
                self.push_val(ValueType::I32);
            }

            Instruction::I32Eq
            | Instruction::I32Ne
            | Instruction::I32LtS
            | Instruction::I32LtU
            | Instruction::I32LeS
            | Instruction::I32LeU
            | Instruction::I32GtS
            | Instruction::I32GtU
            | Instruction::I32GeS
            | Instruction::I32GeU => self.validate_compop(ValType::I32)?,

            Instruction::I64Eqz => {
                // [i64] -> [i32]
                self.pop_val_expect(ValueType::I64)?;
                self.push_val(ValueType::I32);
            }

            Instruction::I64Eq
            | Instruction::I64Ne
            | Instruction::I64LtS
            | Instruction::I64LtU
            | Instruction::I64GtS
            | Instruction::I64GtU
            | Instruction::I64LeS
            | Instruction::I64LeU
            | Instruction::I64GeS
            | Instruction::I64GeU => self.validate_compop(ValType::I64)?,

            Instruction::F32Eq
            | Instruction::F32Ne
            | Instruction::F32Lt
            | Instruction::F32Gt
            | Instruction::F32Le
            | Instruction::F32Ge => self.validate_compop(ValType::F32)?,

            Instruction::F64Eq
            | Instruction::F64Ne
            | Instruction::F64Lt
            | Instruction::F64Gt
            | Instruction::F64Le
            | Instruction::F64Ge => self.validate_compop(ValType::F64)?,

            Instruction::I32Clz | Instruction::I32Ctz | Instruction::I32Popcnt => {
                self.validate_unop(ValType::I32)?
            }
            Instruction::I32Add
            | Instruction::I32Sub
            | Instruction::I32Mul
            | Instruction::I32DivS
            | Instruction::I32DivU
            | Instruction::I32RemS
            | Instruction::I32RemU
            | Instruction::I32And
            | Instruction::I32Or
            | Instruction::I32Xor
            | Instruction::I32Shl
            | Instruction::I32ShrS
            | Instruction::I32ShrU
            | Instruction::I32Rotl
            | Instruction::I32Rotr => self.validate_binop(ValType::I32)?,

            Instruction::I64Clz | Instruction::I64Ctz | Instruction::I64Popcnt => {
                self.validate_unop(ValType::I64)?
            }

            Instruction::I64Add
            | Instruction::I64Sub
            | Instruction::I64Mul
            | Instruction::I64DivS
            | Instruction::I64DivU
            | Instruction::I64RemS
            | Instruction::I64RemU
            | Instruction::I64And
            | Instruction::I64Or
            | Instruction::I64Xor
            | Instruction::I64Shl
            | Instruction::I64ShrS
            | Instruction::I64ShrU
            | Instruction::I64Rotl
            | Instruction::I64Rotr => self.validate_binop(ValType::I64)?,

            Instruction::F32Abs
            | Instruction::F32Neg
            | Instruction::F32Ceil
            | Instruction::F32Floor
            | Instruction::F32Trunc
            | Instruction::F32Nearest
            | Instruction::F32Sqrt => self.validate_unop(ValType::F32)?,

            Instruction::F32Add
            | Instruction::F32Sub
            | Instruction::F32Mul
            | Instruction::F32Div
            | Instruction::F32Min
            | Instruction::F32Max
            | Instruction::F32Copysign => self.validate_binop(ValType::F32)?,

            Instruction::F64Abs
            | Instruction::F64Neg
            | Instruction::F64Ceil
            | Instruction::F64Floor
            | Instruction::F64Trunc
            | Instruction::F64Nearest
            | Instruction::F64Sqrt => self.validate_unop(ValType::F64)?,

            Instruction::F64Add
            | Instruction::F64Sub
            | Instruction::F64Mul
            | Instruction::F64Div
            | Instruction::F64Min
            | Instruction::F64Max
            | Instruction::F64Copysign => self.validate_binop(ValType::F64)?,

            Instruction::I32WrapI64 => self.validate_convop(ValType::I64, ValType::I32)?,
            Instruction::I32TruncF32S | Instruction::I32TruncF32U => {
                self.validate_convop(ValType::F32, ValType::I32)?;
            }
            Instruction::I32TruncF64S | Instruction::I32TruncF64U => {
                self.validate_convop(ValType::F64, ValType::I32)?
            }
            Instruction::I64ExtendI32S | Instruction::I64ExtendI32U => {
                self.validate_convop(ValType::I32, ValType::I64)?
            }
            Instruction::I64TruncF32S | Instruction::I64TruncF32U => {
                self.validate_convop(ValType::F32, ValType::I64)?
            }
            Instruction::I64TruncF64S | Instruction::I64TruncF64U => {
                self.validate_convop(ValType::F64, ValType::I64)?
            }
            Instruction::F32ConvertI32S | Instruction::F32ConvertI32U => {
                self.validate_convop(ValType::I32, ValType::F32)?;
            }
            Instruction::F32ConvertI64S | Instruction::F32ConvertI64U => {
                self.validate_convop(ValType::I64, ValType::F32)?;
            }
            Instruction::F32DemoteF64 => {
                self.validate_convop(ValType::F64, ValType::F32)?;
            }

            Instruction::F64ConvertI32S | Instruction::F64ConvertI32U => {
                self.validate_convop(ValType::I32, ValType::F64)?;
            }
            Instruction::F64ConvertI64S | Instruction::F64ConvertI64U => {
                self.validate_convop(ValType::I64, ValType::F64)?
            }
            Instruction::F64PromoteF32 => self.validate_convop(ValType::F32, ValType::F64)?,

            Instruction::I32ReinterpretF32 => self.validate_convop(ValType::F32, ValType::I32)?,
            Instruction::I64ReinterpretF64 => self.validate_convop(ValType::F64, ValType::I64)?,
            Instruction::F32ReinterpretI32 => self.validate_convop(ValType::I32, ValType::F32)?,
            Instruction::F64ReinterpretI64 => self.validate_convop(ValType::I64, ValType::F64)?,
            Instruction::I32Extend8S | Instruction::I32Extend16S => {
                self.validate_convop(ValType::I32, ValType::I32)?
            }
            Instruction::I64Extend8S | Instruction::I64Extend16S | Instruction::I64Extend32S => {
                self.validate_convop(ValType::I64, ValType::I64)?
            }

            Instruction::RefNull(rt) => self.push_val(ValueType::Ref(*rt)),
            Instruction::RefIsNull => {
                self.pop_ref()?;
                self.push_val(ValueType::I32);
            }
            Instruction::RefFunc(idx) => {
                // Check that the function index is valid in this module
                if idx.as_usize() >= module.func_count() {
                    return Err(IndexValidationError::function(*idx, module.func_count()).into());
                }
                // Check that the function index has been declared for use
                // as reference.
                if !declared_refs.contains(idx) {
                    return Err(ValidationErrorKind::undeclared_function_reference());
                }

                self.push_val(ValueType::Ref(RefType::Func))
            }

            Instruction::I32TruncSatF32S | Instruction::I32TruncSatF32U => {
                self.validate_convop(ValType::F32, ValType::I32)?
            }
            Instruction::I32TruncSatF64S | Instruction::I32TruncSatF64U => {
                self.validate_convop(ValType::F64, ValType::I32)?
            }
            Instruction::I64TruncSatF32S | Instruction::I64TruncSatF32U => {
                self.validate_convop(ValType::F32, ValType::I64)?
            }
            Instruction::I64TruncSatF64S | Instruction::I64TruncSatF64U => {
                self.validate_convop(ValType::F64, ValType::I64)?
            }
            Instruction::MemoryInit((dataidx, memidx)) => {
                // [at, i32, i32] -> []
                self.pop_val_expect(ValueType::I32)?;
                self.pop_val_expect(ValueType::I32)?;
                self.pop_val_expect(ValueType::I32)?;

                // memory exists
                Self::mem_type_at(module, *memidx)?;

                // data exists
                if module
                    .data
                    .as_ref()
                    .is_none_or(|data| (*dataidx as usize) >= data.len())
                {
                    return Err(IndexValidationError::data(*dataidx, module.data_count()).into());
                }
            }
            Instruction::DataDrop(idx) => {
                if module
                    .data
                    .as_ref()
                    .is_none_or(|data| (*idx as usize) >= data.len())
                {
                    return Err(IndexValidationError::data(*idx, module.data_count()).into());
                }
            }
            Instruction::MemoryCopy((dstidx, srcidx)) => {
                // [at at at] -> []
                self.pop_val_expect(ValueType::I32)?;
                self.pop_val_expect(ValueType::I32)?;
                self.pop_val_expect(ValueType::I32)?;

                // mems exist
                Self::mem_type_at(module, *dstidx)?;
                Self::mem_type_at(module, *srcidx)?;
            }
            Instruction::MemoryFill(memidx) => {
                // [at i32 at] -> []
                self.pop_val_expect(ValueType::I32)?;
                self.pop_val_expect(ValueType::I32)?;
                self.pop_val_expect(ValueType::I32)?;

                // mem exist
                Self::mem_type_at(module, *memidx)?;
            }
            Instruction::TableInit((elemidx, tableidx)) => {
                // [at, i32, i32] -> []
                self.pop_val_expect(ValueType::I32)?;
                self.pop_val_expect(ValueType::I32)?;
                self.pop_val_expect(ValueType::I32)?;

                // table exists
                let tabletype = Self::table_type_at(module, *tableidx)?;

                // element exists
                let index = *elemidx as usize;
                let elems = module.elements.as_deref().unwrap_or(&[]);
                let elem = elems
                    .get(index)
                    .ok_or_else(|| IndexValidationError::element(index as u32, elems.len()))?;

                let elemtype = match elem.items {
                    ElementSegmentItems::Functions(_) => RefType::Func,
                    ElementSegmentItems::Expressions(rt, ..) => rt,
                };

                if elemtype != tabletype.elemtype {
                    return Err(ValidationErrorKind::type_mismatch());
                }
            }
            Instruction::ElemDrop(idx) => {
                let count = module.elements.as_ref().map_or(0, Vec::len);
                let index = *idx as usize;
                if index >= count {
                    return Err(IndexValidationError::element(index as u32, count).into());
                }
            }
            Instruction::TableCopy((dstidx, srcidx)) => {
                // [at at at] -> []
                self.pop_val_expect(ValueType::I32)?;
                self.pop_val_expect(ValueType::I32)?;
                self.pop_val_expect(ValueType::I32)?;

                let dst_type = Self::table_type_at(module, *dstidx)?;
                let src_type = Self::table_type_at(module, *srcidx)?;

                if src_type.elemtype != dst_type.elemtype {
                    return Err(ValidationErrorKind::type_mismatch());
                }
            }
            Instruction::TableGrow(idx) => {
                // [t at] -> [at]
                let tabletype = Self::table_type_at(module, *idx)?;
                self.pop_val_expect(ValueType::I32)?;
                self.pop_val_expect(tabletype.elemtype.into())?;

                self.push_val(ValueType::I32);
            }
            Instruction::TableSize(idx) => {
                // [] -> [at]
                Self::table_type_at(module, *idx)?;
                self.push_val(ValueType::I32);
            }
            Instruction::TableFill(idx) => {
                // [at t at] -> []
                self.pop_val_expect(ValueType::I32)?;

                let tabletype = Self::table_type_at(module, *idx)?;
                self.pop_val_expect(tabletype.elemtype.into())?;

                self.pop_val_expect(ValueType::I32)?;
                Self::table_type_at(module, *idx)?;
            }
        }
        Ok(())
    }

    // Section validation

    // Start function, if present, must have type [] -> []

    // Gather the indeices that have been declared for reference
    fn collect_declared_function_refs(module: &Module) -> Vec<FuncIndex> {
        // Scan exports, element segments and global, table initializers
        let mut refs = vec![];

        if let Some(ref exportsec) = module.exports {
            for e in exportsec {
                if let ExportDesc::Func(idx) = e.desc {
                    refs.push(idx);
                }
            }
        }
        if let Some(ref elemsec) = module.elements {
            for e in elemsec {
                match &e.items {
                    ElementSegmentItems::Functions(items) => refs.extend(items.iter().copied()),
                    ElementSegmentItems::Expressions(_, exprs) => {
                        refs.extend(exprs.iter().flat_map(|expr| expr.function_refs()))
                    }
                }
            }
        }
        if let Some(ref tablesec) = module.tables {
            refs.extend(
                tablesec
                    .iter()
                    .filter_map(|t| t.expr.as_ref())
                    .flat_map(|expr| expr.function_refs()),
            );
        }
        if let Some(ref globalsec) = module.globals {
            refs.extend(globalsec.iter().flat_map(|g| g.body.function_refs()));
        }

        refs
    }

    fn validate_module(module: &Module) -> result::Result<(), ValidationError> {
        sections::table::validate(module)?;
        sections::memory::validate(module)?;
        sections::element::validate(module)?;
        sections::import::validate(module)?;
        sections::data::validate(module)?;
        sections::global::validate(module)?;
        sections::export::validate(module)?;
        sections::start::validate(module)?;

        let funcsec = module.functions.as_deref().unwrap_or(&[]);
        let codesec = module.codes.as_deref().unwrap_or(&[]);

        if funcsec.len() != codesec.len() {
            return Err(ValidationErrorKind::function_code_mismatch().into());
        }

        let declared_refs = Self::collect_declared_function_refs(module);

        for (index, (type_idx, entry)) in funcsec.iter().zip(codesec).enumerate() {
            let functype = module
                .types
                .as_ref()
                .and_then(|t| t.get(*type_idx as usize))
                .ok_or_else(|| ValidationError::Function {
                    index: index + module.imported_func_count(),
                    source: FunctionValidationError::UnknownType(IndexValidationError::ty(
                        *type_idx,
                        module.type_count(),
                    )),
                })?;
            Validator::default()
                .validate_function(functype, entry, module, &declared_refs)
                .map_err(|e| ValidationError::Function {
                    index: index + module.imported_func_count(),
                    source: e,
                })?
        }

        Ok(())
    }
}

pub fn validate_module(module: &Module) -> result::Result<(), ValidationError> {
    Validator::validate_module(module)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binary::module::decode_bytes;

    #[test]
    fn test_ref_func_undeclared() -> anyhow::Result<()> {
        // Function 0 exists, but is not declared for reference use.
        let wat = r#"
            (module
                (func $f
                    ref.func $f
                    drop))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            result.is_err(),
            "expected validation error for undeclared function reference, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_ref_func_declared() -> anyhow::Result<()> {
        let wat = r#"
            (module
                (elem declare func $f)
                (func $f
                    ref.func $f
                    drop))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        validate_module(&m)?;
        Ok(())
    }

    #[test]
    fn test_ref_func_declared_by_global_initializer() -> anyhow::Result<()> {
        // The global initializer is the only declaration of the reference.
        let wat = r#"
            (module
                (global funcref (ref.func $f))
                (func $f
                    ref.func $f
                    drop))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        validate_module(&m)?;
        Ok(())
    }

    #[test]
    fn test_ref_func_declared_by_table_initializer() -> anyhow::Result<()> {
        // The table initializer is the only declaration of the reference.
        let wat = r#"
            (module
                (table 1 funcref (ref.func $f))
                (func $f
                    ref.func $f
                    drop))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        validate_module(&m)?;
        Ok(())
    }

    #[test]
    fn test_type_unary_operand_empty() -> anyhow::Result<()> {
        let wat = r#"
            (func (i32.eqz) (drop))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            result.is_err(),
            "expected validation error, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_block_type_index_out_of_range() -> anyhow::Result<()> {
        let wat = r#"
            (module
                (type (func))
                (func (type 0)
                    (block (type 1))))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            result.is_err(),
            "expected validation error for out-of-range block type index, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_type_unary_operand_empty_in_block() -> anyhow::Result<()> {
        let wat = r#"
            (func
                (i32.const 0)
                (block
                    (i32.eqz)
                    (drop)))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            result.is_err(),
            "expected validation error, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_type_unary_operand_empty_in_loop() -> anyhow::Result<()> {
        let wat = r#"
            (func
                (i32.const 0)
                (loop
                    (i32.eqz)
                    (drop)))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            result.is_err(),
            "expected validation error, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_type_unary_operand_empty_in_if() -> anyhow::Result<()> {
        let wat = r#"
            (func
                (i32.const 0)
                (i32.const 0)
                (if
                    (then
                        (i32.eqz)
                        (drop))))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            result.is_err(),
            "expected validation error, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_type_unary_operand_empty_in_else() -> anyhow::Result<()> {
        let wat = r#"
            (func
                (i32.const 0)
                (i32.const 0)
                (if (result i32)
                    (then (i32.const 0))
                    (else (i32.eqz)))
                (drop))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            result.is_err(),
            "expected validation error, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_type_unary_operand_empty_in_br() -> anyhow::Result<()> {
        let wat = r#"
            (func
                (i32.const 0)
                (block
                    (br 0 (i32.eqz))
                    (drop)))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            result.is_err(),
            "expected validation error, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_type_unary_operand_empty_in_br_if() -> anyhow::Result<()> {
        let wat = r#"
            (func
                (i32.const 0)
                (block
                    (br_if 0 (i32.eqz) (i32.const 1))
                    (drop)))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            result.is_err(),
            "expected validation error, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_type_unary_operand_empty_in_br_table() -> anyhow::Result<()> {
        let wat = r#"
            (func
                (i32.const 0)
                (block
                    (br_table 0 (i32.eqz))
                    (drop)))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            result.is_err(),
            "expected validation error, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_br_table_operand_type_mismatch() -> anyhow::Result<()> {
        // Both targets expect i32, but the actual branch operand is i64.
        // Pushing target types without checking operands hides this mismatch.
        let wat = r#"
            (module
                (func (result i32)
                    i64.const 42
                    i32.const 0
                    br_table 0 0))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            result.is_err(),
            "expected validation error for i64 branch operand targeting i32, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_type_unary_operand_empty_in_return() -> anyhow::Result<()> {
        let wat = r#"
            (func (return (i32.eqz)) (drop))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            result.is_err(),
            "expected validation error, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_br_if_unbound_label() -> anyhow::Result<()> {
        let wat = r#"
            (module (func $unbound-label (br_if 1 (i32.const 1))))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            result.is_err(),
            "expected validation error, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_type_unary_operand_empty_in_select() -> anyhow::Result<()> {
        let wat = r#"
            (func
                (select (i32.eqz) (i32.const 1) (i32.const 2))
                (drop))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            result.is_err(),
            "expected validation error, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_memory_instructions_unknown_memory() -> anyhow::Result<()> {
        // Each function uses a memory instruction in a module without memory.
        let cases = [
            (
                "memory.size",
                r#"
                    (module
                        (func (result i32)
                            memory.size))
                "#,
                0,
            ),
            (
                "memory.grow",
                r#"
                    (module
                        (func (result i32)
                            i32.const 1
                            memory.grow))
                "#,
                1,
            ),
            (
                "i32.load",
                r#"
                    (module
                        (func (result i32)
                            i32.const 0
                            i32.load))
                "#,
                1,
            ),
            (
                "i32.store",
                r#"
                    (module
                        (func
                            i32.const 0
                            i32.const 0
                            i32.store))
                "#,
                2,
            ),
        ];

        for (name, wat, inst_index) in cases {
            let bytes = wat::parse_str(wat)?;
            let m = decode_bytes(bytes)?;
            let result = validate_module(&m);
            assert!(
                matches!(
                    result,
                    Err(ValidationError::Function {
                        index: 0,
                        source: FunctionValidationError::Instruction {
                            index,
                            name: _,
                            kind: ValidationErrorKind::InvalidIndex(IndexValidationError {
                                space: IndexSpace::Memory,
                                index: 0,
                                count: 0
                            })
                        }
                    }) if index == inst_index
                ),
                "{name}: expected invalid memory index, got: {:?}",
                result
            );
        }
        Ok(())
    }

    #[test]
    fn test_table_size_imported_table() -> anyhow::Result<()> {
        let wat = r#"
            (module
                (import "e" "t" (table 1 funcref))
                (func (result i32) table.size))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(result.is_ok(), "expected Ok, got: {:?}", result);
        Ok(())
    }

    #[test]
    fn test_valid_unreached_add() -> anyhow::Result<()> {
        let wat = r#"
            (func (result i32)
                (i32.const 1)
                (unreachable)
                (i32.add)
            )"#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(result.is_ok(), "expected Ok, got: {:?}", result);
        Ok(())
    }

    #[test]
    fn test_valid_unreached_ref_isnull() -> anyhow::Result<()> {
        let wat = r#"
            (func (result i32)
                (i32.const 1)
                (unreachable)
                (ref.is_null)
            )"#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(result.is_ok(), "expected Ok, got: {:?}", result);
        Ok(())
    }

    #[test]
    fn test_invalid_unreached_type_mismatch() -> anyhow::Result<()> {
        let wat = r#"
            (func (result i32)
                (i32.const 1)
                (unreachable)
                (i64.const 2)
                (i32.add)
            )"#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            matches!(result, Err(ValidationError::Function { .. })),
            "expected TypeMismatch, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_function_error_index_includes_imports() -> anyhow::Result<()> {
        // The invalid function is at position 1 in the function section and
        // uses type 2, but its index in the function index space is 3.
        let wat = r#"
            (module
                (type (func))
                (type (func (param i32)))
                (type (func (result i32)))
                (import "env" "f0" (func (type 0)))
                (import "env" "f1" (func (type 0)))
                (func (type 1))
                (func (type 2)))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            matches!(result, Err(ValidationError::Function { index: 3, .. })),
            "expected function error at index 3, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_local_index_out_of_range() -> anyhow::Result<()> {
        // 3 params + 2 declared locals: the local index space has 5 entries,
        // so index 5 is the first out of range.
        let wat = r#"
            (module
                (func (param i32 i32 i32) (local i64 i64)
                    local.get 5
                    drop))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            matches!(
                result,
                Err(ValidationError::Function {
                    index: 0,
                    source: FunctionValidationError::Instruction {
                        index: 0,
                        name: _,
                        kind: ValidationErrorKind::InvalidIndex(IndexValidationError {
                            space: IndexSpace::Local,
                            index: 5,
                            count: 5
                        })
                    }
                })
            ),
            "expected invalid local index, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_data_drop_index_out_of_range() -> anyhow::Result<()> {
        // 1 type and 2 data segments, so a count taken from the wrong
        // section shows up in the error.
        let wat = r#"
            (module
                (func
                    data.drop 2)
                (data "a")
                (data "b"))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            matches!(
                result,
                Err(ValidationError::Function {
                    index: 0,
                    source: FunctionValidationError::Instruction {
                        index: 0,
                        name: _,
                        kind: ValidationErrorKind::InvalidIndex(IndexValidationError {
                            space: IndexSpace::Data,
                            index: 2,
                            count: 2
                        })
                    }
                })
            ),
            "expected invalid data index, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_memory_init_data_index_out_of_range() -> anyhow::Result<()> {
        // 1 type and 2 data segments, so a count taken from the wrong
        // section shows up in the error.
        let wat = r#"
            (module
                (memory 1)
                (func
                    i32.const 0
                    i32.const 0
                    i32.const 0
                    memory.init 2)
                (data "a")
                (data "b"))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            matches!(
                result,
                Err(ValidationError::Function {
                    index: 0,
                    source: FunctionValidationError::Instruction {
                        index: 3,
                        name: _,
                        kind: ValidationErrorKind::InvalidIndex(IndexValidationError {
                            space: IndexSpace::Data,
                            index: 2,
                            count: 2
                        })
                    }
                })
            ),
            "expected invalid data index, got: {:?}",
            result
        );
        Ok(())
    }
}
