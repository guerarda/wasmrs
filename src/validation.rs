use crate::{
    binary::{
        module::Module,
        sections::{
            code::{CodeEntry, FuncLocal},
            global::{GlobalType, MutabilityFlag},
            memory::MemType,
            table::TableType,
        },
        types::{BlockType, FuncType, GlobalIdx, MemIdx, RefType, ValType},
    },
    instructions::Instruction,
};

use std::{error, fmt, result};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ValTypeOrUnknown {
    Val(ValType),
    Unknown,
}

impl ValTypeOrUnknown {
    fn is_num(&self) -> bool {
        matches!(
            self,
            Self::Val(ValType::I32 | ValType::I64 | ValType::F32 | ValType::F64) | Self::Unknown
        )
    }

    fn is_vec(&self) -> bool {
        matches!(self, Self::Val(ValType::V128) | Self::Unknown)
    }

    #[allow(dead_code)]
    fn is_ref(&self) -> bool {
        matches!(self, Self::Val(ValType::Ref(_)) | Self::Unknown)
    }
}

impl From<&ValType> for ValTypeOrUnknown {
    fn from(value: &ValType) -> Self {
        ValTypeOrUnknown::Val(*value)
    }
}

impl From<&GlobalType> for ValTypeOrUnknown {
    fn from(value: &GlobalType) -> Self {
        ValTypeOrUnknown::Val(value.type_)
    }
}

#[derive(Debug)]
pub struct CtrlFrame {
    opcode: Instruction,
    start_types: Vec<ValTypeOrUnknown>,
    end_types: Vec<ValTypeOrUnknown>,
    height: usize,
    unreachable: bool,
}

#[derive(Debug, Default)]
struct Validator {
    vals: Vec<ValTypeOrUnknown>,
    ctrls: Vec<CtrlFrame>,
}

#[derive(Debug)]
#[non_exhaustive]
pub enum ValidationError {
    ControlStackUnderflow,
    ValueStackUnderflow,
    TypeMismatch,
    TableTypeMismatch,
    ElseWithoutMatchingIf,
    UnknownLocal,
    UnknownGlobal,
    UnknownType,
    UnknownTable,
    UnknownFunction,
    UnknownMemory,
    ImmutableGlobal,
    MissingGlobalSection,
    InvalidSelectTypes,
    InvalidMemAlignment,
}

impl error::Error for ValidationError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        None
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ControlStackUnderflow => write!(f, "control stack underflow"),
            Self::ValueStackUnderflow => write!(f, "value stack underflow"),
            Self::TypeMismatch => write!(f, "type mismatch"),
            Self::TableTypeMismatch => write!(f, "table type mismatch"),
            Self::ElseWithoutMatchingIf => write!(f, "else without matching if"),
            Self::UnknownLocal => write!(f, "unknown local"),
            Self::UnknownGlobal => write!(f, "unknown global"),
            Self::UnknownType => write!(f, "unknown type"),
            Self::UnknownTable => write!(f, "unknown table"),
            Self::UnknownFunction => write!(f, "unknown function"),
            Self::UnknownMemory => write!(f, "unknown memory"),
            Self::ImmutableGlobal => write!(f, "immutable global"),
            Self::MissingGlobalSection => write!(f, "missing global section"),
            Self::InvalidSelectTypes => write!(f, "select types must have exactly one entry"),
            Self::InvalidMemAlignment => write!(f, "invalid mem alignment"),
        }
    }
}

impl Validator {
    fn push_val(&mut self, val: ValTypeOrUnknown) {
        self.vals.push(val)
    }

    fn push_vals(&mut self, vals: &[ValTypeOrUnknown]) {
        self.vals.extend_from_slice(vals)
    }

    fn pop_vals_expect(
        &mut self,
        expected: &[ValTypeOrUnknown],
    ) -> result::Result<Vec<ValTypeOrUnknown>, ValidationError> {
        let mut res = vec![];
        for t in expected.iter().rev() {
            res.push(self.pop_val_expect(*t)?);
        }

        Ok(res)
    }

    fn pop_val(&mut self) -> result::Result<ValTypeOrUnknown, ValidationError> {
        let last = self.ctrls.last().expect("unexpected empty control stack");
        if self.vals.len() == last.height && last.unreachable {
            return Ok(ValTypeOrUnknown::Unknown);
        }

        if self.vals.len() == last.height {
            return Err(ValidationError::ValueStackUnderflow);
        }

        Ok(self.vals.pop().expect("unexpected empty value stack"))
    }

    fn pop_val_expect(
        &mut self,
        expected: ValTypeOrUnknown,
    ) -> result::Result<ValTypeOrUnknown, ValidationError> {
        let actual = self.pop_val()?;
        if actual != expected
            && actual != ValTypeOrUnknown::Unknown
            && expected != ValTypeOrUnknown::Unknown
        {
            return Err(ValidationError::TypeMismatch);
        }
        Ok(actual)
    }

    fn push_ctrl(
        &mut self,
        opcode: Instruction,
        start: Vec<ValTypeOrUnknown>,
        end: Vec<ValTypeOrUnknown>,
    ) {
        self.ctrls.push(CtrlFrame {
            opcode,
            start_types: start,
            end_types: end,
            height: self.vals.len(),
            unreachable: false,
        })
    }

    fn pop_ctrl(&mut self) -> result::Result<CtrlFrame, ValidationError> {
        let frame = self
            .ctrls
            .last()
            .ok_or(ValidationError::ControlStackUnderflow)?;
        let height = frame.height;

        // need to clone here because pop_vals_expect below
        // needs a mutable ref
        let end_types = frame.end_types.clone();

        let _ = self.pop_vals_expect(&end_types)?;
        if self.vals.len() != height {
            return Err(ValidationError::TypeMismatch);
        }
        Ok(self.ctrls.pop().expect("unexpected empty control stack"))
    }

    /// Marks the current frame as unreachable
    fn unreachable(&mut self) {
        let last = self
            .ctrls
            .last_mut()
            .expect("unexpected empty control stack");
        self.vals.resize(last.height, ValTypeOrUnknown::Unknown);
        last.unreachable = true;
    }

    /// Converts a Block Type into a ([t1*], [t2*])
    fn block_type(
        bt: &BlockType,
        module: &Module,
    ) -> result::Result<(Vec<ValTypeOrUnknown>, Vec<ValTypeOrUnknown>), ValidationError> {
        Ok(match bt {
            BlockType::Empty => (vec![], vec![]),
            BlockType::Value(vt) => (vec![], vec![ValTypeOrUnknown::Val(*vt)]),
            BlockType::Index(idx) => {
                let types = &module.types.as_ref().ok_or(ValidationError::UnknownType)?;

                let t = &types[*idx as usize];
                (
                    t.params
                        .iter()
                        .copied()
                        .map(ValTypeOrUnknown::Val)
                        .collect(),
                    t.results
                        .iter()
                        .copied()
                        .map(ValTypeOrUnknown::Val)
                        .collect(),
                )
            }
        })
    }

    /// Returns the function type at the given type index (not function index)
    fn type_at(module: &Module, type_idx: u32) -> result::Result<&FuncType, ValidationError> {
        module
            .types
            .as_ref()
            .and_then(|types| types.get(type_idx as usize))
            .ok_or(ValidationError::UnknownType)
    }

    /// Returns the type for an idx in the Function Section.
    // TODO doesn't handle imports, index is in 0..imports..functions,
    fn func_type_at(module: &Module, func_idx: u32) -> result::Result<&FuncType, ValidationError> {
        let &type_idx = module
            .functions
            .as_ref()
            .and_then(|funcs| funcs.get(func_idx as usize))
            .ok_or(ValidationError::UnknownFunction)?;

        Self::type_at(module, type_idx)
    }

    /// Returns the table type at the given index
    fn table_type(module: &Module, table_idx: u32) -> result::Result<&TableType, ValidationError> {
        module
            .tables
            .as_ref()
            .and_then(|tables| tables.get(table_idx as usize))
            .ok_or(ValidationError::UnknownTable)
    }

    /// Return the type of the function local at the given index
    fn local_type(
        functype: &FuncType,
        locals: &[FuncLocal],
        idx: u32,
    ) -> result::Result<ValTypeOrUnknown, ValidationError> {
        if let Some(&param) = functype.params.get(idx as usize) {
            return Ok(ValTypeOrUnknown::Val(param));
        }
        let local_idx = idx - functype.params.len() as u32;
        let mut count: u32 = 0;

        for local in locals {
            if local_idx < count + local.count {
                return Ok(ValTypeOrUnknown::Val(local.valtype));
            }
            count += local.count;
        }
        Err(ValidationError::UnknownLocal)
    }

    /// Returns the global type at the given index
    fn global_type(
        module: &Module,
        idx: GlobalIdx,
    ) -> result::Result<&GlobalType, ValidationError> {
        let globals = module
            .globals
            .as_ref()
            .ok_or(ValidationError::MissingGlobalSection)?;

        let entry = globals
            .get(idx as usize)
            .ok_or(ValidationError::UnknownGlobal)?;

        Ok(&entry.gt)
    }

    /// Returns the label_types for the nth frame from the top
    fn label_types(n: usize, frames: &[CtrlFrame]) -> &[ValTypeOrUnknown] {
        let frame = &frames[frames.len() - n - 1];
        if matches!(frame.opcode, Instruction::Loop(_)) {
            &frame.start_types
        } else {
            &frame.end_types
        }
    }

    fn mem_type_at(module: &Module, mem_idx: MemIdx) -> result::Result<&MemType, ValidationError> {
        module
            .memories
            .as_ref()
            .and_then(|mems| mems.get(mem_idx as usize))
            .ok_or(ValidationError::UnknownMemory)
    }

    /// Validate that the memory alignment is valid for the given type
    fn validate_mem_alignment(align: u32, t: ValType) -> result::Result<(), ValidationError> {
        if (1 << align) > t.size_bytes() {
            Err(ValidationError::InvalidMemAlignment)
        } else {
            Ok(())
        }
    }

    fn validate_bin_op(&mut self, valtype: ValType) -> result::Result<(), ValidationError> {
        let valtype = ValTypeOrUnknown::Val(valtype);
        self.pop_val_expect(valtype)?;
        self.pop_val_expect(valtype)?;
        self.push_val(valtype);
        Ok(())
    }

    fn validate_unary_op(&mut self, valtype: ValType) -> result::Result<(), ValidationError> {
        let valtype = ValTypeOrUnknown::Val(valtype);
        self.pop_val_expect(valtype)?;
        self.push_val(valtype);
        Ok(())
    }

    fn validate_comp_op(&mut self, valtype: ValType) -> result::Result<(), ValidationError> {
        let valtype = ValTypeOrUnknown::Val(valtype);
        self.pop_val_expect(valtype)?;
        self.pop_val_expect(valtype)?;
        self.push_val(ValTypeOrUnknown::Val(ValType::I32));
        Ok(())
    }

    fn validate_function(
        &mut self,
        functype: &FuncType,
        entry: &CodeEntry,
        module: &Module,
    ) -> result::Result<(), ValidationError> {
        let start_types: Vec<ValTypeOrUnknown> =
            functype.params.iter().map(ValTypeOrUnknown::from).collect();
        let end_types: Vec<ValTypeOrUnknown> = functype
            .results
            .iter()
            .map(ValTypeOrUnknown::from)
            .collect();
        self.push_ctrl(Instruction::Call(0), start_types, end_types);

        for inst in &entry.body {
            match inst {
                Instruction::Unreachable => self.unreachable(),
                Instruction::Nop => continue,
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
                    let (t1, t2) = Self::block_type(bt, module)?;

                    self.pop_val_expect(ValTypeOrUnknown::Val(ValType::I32))?;
                    self.pop_vals_expect(&t1)?;

                    self.push_ctrl(Instruction::If(*bt), t1, t2);
                }
                Instruction::Else => {
                    let frame = self.pop_ctrl()?;
                    if !matches!(frame.opcode, Instruction::If(_)) {
                        return Err(ValidationError::ElseWithoutMatchingIf);
                    }
                    self.push_ctrl(Instruction::Else, frame.start_types, frame.end_types)
                }
                Instruction::End => {
                    let frame = self.pop_ctrl()?;
                    self.push_vals(&frame.end_types);
                }
                Instruction::Br(n) => {
                    let n = *n as usize;
                    if self.ctrls.len() <= n {
                        return Err(ValidationError::ControlStackUnderflow);
                    }
                    // FIXME Avoid Copy. pop_vals must be in ValueStack impl
                    let lt = Self::label_types(n, &self.ctrls).to_vec();
                    self.pop_vals_expect(&lt)?;
                    self.unreachable();
                }
                Instruction::BrIf(n) => {
                    let n = *n as usize;
                    if self.ctrls.len() < n {
                        return Err(ValidationError::ControlStackUnderflow);
                    }
                    self.pop_val_expect(ValTypeOrUnknown::Val(ValType::I32))?;

                    // FIXME Avoid Copy. pop_vals must be in ValueStack impl
                    let lt = Self::label_types(n, &self.ctrls).to_vec();
                    self.pop_vals_expect(&lt)?;
                    self.push_vals(&lt);
                }
                Instruction::BrTable(idx) => {
                    self.pop_val_expect(ValTypeOrUnknown::Val(ValType::I32))?;

                    let m = idx.default as usize;
                    if self.ctrls.len() <= m {
                        return Err(ValidationError::ControlStackUnderflow);
                    }
                    let m_types = Self::label_types(m, &self.ctrls).to_vec();
                    let arity = m_types.len();

                    for n in &idx.labels {
                        let n = *n as usize;
                        if self.ctrls.len() <= n {
                            return Err(ValidationError::ControlStackUnderflow);
                        }
                        let n_types = Self::label_types(n, &self.ctrls).to_vec();
                        if n_types.len() != arity {
                            return Err(ValidationError::TypeMismatch);
                        }
                        self.push_vals(&n_types);
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
                    let params: Vec<ValTypeOrUnknown> = func_type
                        .params
                        .iter()
                        .copied()
                        .map(ValTypeOrUnknown::Val)
                        .collect();
                    self.pop_vals_expect(&params)?;

                    let results: Vec<ValTypeOrUnknown> = func_type
                        .results
                        .iter()
                        .copied()
                        .map(ValTypeOrUnknown::Val)
                        .collect();

                    self.push_vals(&results);
                }
                Instruction::CallIndirect((type_idx, table_idx)) => {
                    // [t1* i32] -> [t2*]
                    let table_type = Self::table_type(module, *table_idx)?;

                    if !matches!(table_type.etype, RefType::Func) {
                        return Err(ValidationError::TableTypeMismatch);
                    }

                    self.pop_val_expect(ValTypeOrUnknown::Val(ValType::I32))?;

                    let func_type = Self::type_at(module, *type_idx)?;
                    let params: Vec<ValTypeOrUnknown> = func_type
                        .params
                        .iter()
                        .copied()
                        .map(ValTypeOrUnknown::Val)
                        .collect();
                    self.pop_vals_expect(&params)?;

                    let results: Vec<ValTypeOrUnknown> = func_type
                        .results
                        .iter()
                        .copied()
                        .map(ValTypeOrUnknown::Val)
                        .collect();

                    self.push_vals(&results);
                }

                Instruction::Drop => {
                    self.pop_val()?;
                }
                Instruction::Select => {
                    self.pop_val_expect(ValTypeOrUnknown::Val(ValType::I32))?;
                    let t1 = self.pop_val()?;
                    let t2 = self.pop_val()?;

                    if !((t1.is_num() && t2.is_num()) || (t1.is_vec() && t2.is_vec())) {
                        return Err(ValidationError::TypeMismatch);
                    }

                    if t1 != t2
                        && t1 != ValTypeOrUnknown::Unknown
                        && t2 != ValTypeOrUnknown::Unknown
                    {
                        return Err(ValidationError::TypeMismatch);
                    }

                    if matches!(t1, ValTypeOrUnknown::Unknown) {
                        self.push_val(t2);
                    } else {
                        self.push_val(t1);
                    }
                }
                Instruction::SelectT(vt) => {
                    let [t] = vt.as_slice() else {
                        return Err(ValidationError::InvalidSelectTypes);
                    };

                    self.pop_val_expect(ValTypeOrUnknown::Val(ValType::I32))?;
                    self.pop_val_expect(ValTypeOrUnknown::Val(*t))?;
                    self.pop_val_expect(ValTypeOrUnknown::Val(*t))?;
                    self.push_val(ValTypeOrUnknown::Val(*t));
                }
                Instruction::LocalGet(idx) => {
                    let vt = Self::local_type(functype, &entry.locals, *idx)?;
                    self.push_val(vt);
                }
                Instruction::LocalSet(idx) => {
                    let vt = Self::local_type(functype, &entry.locals, *idx)?;
                    self.pop_val_expect(vt)?;
                }
                Instruction::LocalTee(idx) => {
                    let vt = Self::local_type(functype, &entry.locals, *idx)?;
                    self.pop_val_expect(vt)?;
                    self.push_val(vt);
                }
                Instruction::GlobalGet(idx) => {
                    let gt = Self::global_type(module, *idx)?;
                    self.push_val(gt.into());
                }
                Instruction::GlobalSet(idx) => {
                    let gt = Self::global_type(module, *idx)?;
                    if matches!(gt.mutflag, MutabilityFlag::Const) {
                        return Err(ValidationError::ImmutableGlobal);
                    }
                    self.pop_val_expect(gt.into())?;
                }
                Instruction::I32Load(memarg) => {
                    // mems[0] is defined in the context
                    let _ = Self::mem_type_at(module, 0)?;

                    // alignment not larger than bit width
                    Self::validate_mem_alignment(memarg.align, ValType::I32)?;

                    // [i32] -> [t]
                    self.pop_val_expect(ValTypeOrUnknown::Val(ValType::I32))?;
                    self.push_val(ValTypeOrUnknown::Val(ValType::I32));
                }
                Instruction::I32Store(memarg) => {
                    // mems[0] is defined in the context
                    let _ = Self::mem_type_at(module, 0)?;

                    // alignment not larger than bit width
                    Self::validate_mem_alignment(memarg.align, ValType::I32)?;

                    // [i32 t] -> []
                    self.pop_val_expect(ValTypeOrUnknown::Val(ValType::I32))?;
                    self.pop_val_expect(ValTypeOrUnknown::Val(ValType::I32))?;
                }
                Instruction::I32Const(_) => self.push_val(ValTypeOrUnknown::Val(ValType::I32)),

                Instruction::I64Const(_) => self.push_val(ValTypeOrUnknown::Val(ValType::I64)),
                Instruction::I32Eqz => {
                    self.pop_val_expect(ValTypeOrUnknown::Val(ValType::I32))?;
                    self.push_val(ValTypeOrUnknown::Val(ValType::I32));
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
                | Instruction::I32GeU => self.validate_comp_op(ValType::I32)?,

                Instruction::I32Clz | Instruction::I32Ctz | Instruction::I32Popcnt => {
                    self.validate_unary_op(ValType::I32)?
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
                | Instruction::I32Rotr => self.validate_bin_op(ValType::I32)?,

                Instruction::I32Extend8S => todo!(),
                Instruction::I32Extend16S => todo!(),
                Instruction::RefNull(_ref_type) => todo!(),
                Instruction::RefFunc(_) => todo!(),
            }
        }
        self.pop_ctrl()?;
        Ok(())
    }

    fn validate_module(module: &Module) -> result::Result<(), ValidationError> {
        // At this point Function and Code section should be consistent
        debug_assert_eq!(module.functions.is_some(), module.codes.is_some());

        let Some(ref function_section) = module.functions else {
            return Ok(());
        };
        let Some(ref code_section) = module.codes else {
            return Ok(());
        };

        let Some(ref type_section) = module.types else {
            return Ok(());
        };

        debug_assert_eq!(function_section.len(), code_section.len());

        function_section
            .iter()
            .zip(code_section.iter())
            .try_for_each(|(idx, entry)| {
                Validator::default().validate_function(&type_section[*idx as usize], entry, module)
            })
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
    fn test_type_unary_operand_empty() -> anyhow::Result<()> {
        // (func (i32.eqz) (drop))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            b"\x01\x04\x01\x60\x00\x00",
            b"\x03\x02\x01\x00",
            b"\x0a\x06\x01\x04\x00\x45\x1a\x0b",
        ]
        .concat();
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
    fn test_type_unary_operand_empty_in_block() -> anyhow::Result<()> {
        // (func (i32.const 0) (block (i32.eqz) (drop)))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            b"\x01\x04\x01\x60\x00\x00",
            b"\x03\x02\x01\x00",
            b"\x0a\x0b\x01\x09\x00\x41\x00\x02\x40\x45\x1a\x0b\x0b",
        ]
        .concat();
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
        // (func (i32.const 0) (loop (i32.eqz) (drop)))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            b"\x01\x04\x01\x60\x00\x00",
            b"\x03\x02\x01\x00",
            b"\x0a\x0b\x01\x09\x00\x41\x00\x03\x40\x45\x1a\x0b\x0b",
        ]
        .concat();
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
        // (func (i32.const 0) (i32.const 0) (if (then (i32.eqz) (drop))))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            b"\x01\x04\x01\x60\x00\x00",
            b"\x03\x02\x01\x00",
            b"\x0a\x0d\x01\x0b\x00\x41\x00\x41\x00\x04\x40\x45\x1a\x0b\x0b",
        ]
        .concat();
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
        // (func (i32.const 0) (i32.const 0) (if (result i32) (then (i32.const 0)) (else (i32.eqz))) (drop))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            b"\x01\x04\x01\x60\x00\x00",
            b"\x03\x02\x01\x00",
            b"\x0a\x10\x01\x0e\x00\x41\x00\x41\x00\x04\x7f\x41\x00\x05\x45\x0b\x1a\x0b",
        ]
        .concat();
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
        // (func (i32.const 0) (block (br 0 (i32.eqz)) (drop)))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            b"\x01\x04\x01\x60\x00\x00",
            b"\x03\x02\x01\x00",
            b"\x0a\x0d\x01\x0b\x00\x41\x00\x02\x40\x45\x0c\x00\x1a\x0b\x0b",
        ]
        .concat();
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
        // (func (i32.const 0) (block (br_if 0 (i32.eqz) (i32.const 1)) (drop)))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            b"\x01\x04\x01\x60\x00\x00",
            b"\x03\x02\x01\x00",
            b"\x0a\x0f\x01\x0d\x00\x41\x00\x02\x40\x45\x41\x01\x0d\x00\x1a\x0b\x0b",
        ]
        .concat();
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
        // (func (i32.const 0) (block (br_table 0 (i32.eqz)) (drop)))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            b"\x01\x04\x01\x60\x00\x00",
            b"\x03\x02\x01\x00",
            b"\x0a\x0e\x01\x0c\x00\x41\x00\x02\x40\x45\x0e\x00\x00\x1a\x0b\x0b",
        ]
        .concat();
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
    fn test_type_unary_operand_empty_in_return() -> anyhow::Result<()> {
        // (func (return (i32.eqz)) (drop))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            b"\x01\x04\x01\x60\x00\x00",
            b"\x03\x02\x01\x00",
            b"\x0a\x07\x01\x05\x00\x45\x0f\x1a\x0b",
        ]
        .concat();
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
        // (func (select (i32.eqz) (i32.const 1) (i32.const 2)) (drop))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            b"\x01\x04\x01\x60\x00\x00",
            b"\x03\x02\x01\x00",
            b"\x0a\x0b\x01\x09\x00\x45\x41\x01\x41\x02\x1b\x1a\x0b",
        ]
        .concat();
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            result.is_err(),
            "expected validation error, got: {:?}",
            result
        );
        Ok(())
    }
}
