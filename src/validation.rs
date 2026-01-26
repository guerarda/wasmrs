use crate::{
    binary::{
        module::Module,
        types::{FuncType, ValType},
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
    #[allow(dead_code)]
    fn is_num(&self) -> bool {
        matches!(
            self,
            Self::Val(ValType::I32 | ValType::I64 | ValType::F32 | ValType::F64) | Self::Unknown
        )
    }

    #[allow(dead_code)]
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

#[derive(Debug)]
pub struct CtrlFrame {
    #[allow(dead_code)]
    opcode: Instruction,
    #[allow(dead_code)]
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
        start: &[ValTypeOrUnknown],
        end: &[ValTypeOrUnknown],
    ) {
        self.ctrls.push(CtrlFrame {
            opcode,
            start_types: start.to_vec(),
            end_types: end.to_vec(),
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

    #[allow(dead_code)]
    fn label_types(frame: CtrlFrame) -> Vec<ValTypeOrUnknown> {
        // TODO Loop
        frame.end_types
    }

    #[allow(dead_code)]
    fn unreachable(&mut self) {
        let last = self
            .ctrls
            .last_mut()
            .expect("unexpected empty control stack");
        self.vals.resize(last.height, ValTypeOrUnknown::Unknown);
        last.unreachable = true;
    }

    fn validate_function(
        &mut self,
        functype: &FuncType,
        body: &[Instruction],
        _module: &Module,
    ) -> result::Result<(), ValidationError> {
        let start_types: Vec<ValTypeOrUnknown> =
            functype.params.iter().map(ValTypeOrUnknown::from).collect();
        let end_types: Vec<ValTypeOrUnknown> = functype
            .results
            .iter()
            .map(ValTypeOrUnknown::from)
            .collect();
        self.push_ctrl(Instruction::Call(0), &start_types, &end_types);

        for inst in body {
            match inst {
                Instruction::Unreachable => todo!(),
                Instruction::Nop => todo!(),
                Instruction::Block(_) => todo!(),
                Instruction::Loop(_) => todo!(),
                Instruction::If(_) => todo!(),
                Instruction::Else => todo!(),
                Instruction::End => {
                    let frame = self.pop_ctrl()?;
                    self.push_vals(&frame.end_types);
                }
                Instruction::Br(_) => todo!(),
                Instruction::BrIf(_) => todo!(),
                Instruction::BrTable(_) => todo!(),
                Instruction::Call(_) => todo!(),
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
                        && !matches!(t1, ValTypeOrUnknown::Unknown)
                        && !matches!(t2, ValTypeOrUnknown::Unknown)
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
                    self.pop_val_expect(ValTypeOrUnknown::Val(ValType::I32))?;
                    self.pop_val_expect(ValTypeOrUnknown::Val(*vt))?;
                    self.pop_val_expect(ValTypeOrUnknown::Val(*vt))?;
                    self.push_val(ValTypeOrUnknown::Val(*vt));
                }
                Instruction::LocalGet(_) => todo!(),
                Instruction::LocalSet(_) => todo!(),
                Instruction::LocalTee(_) => todo!(),
                Instruction::I32Const(_) => todo!(),
                Instruction::I64Const(_) => todo!(),
                Instruction::I32Eqz => {
                    self.pop_val_expect(ValTypeOrUnknown::Val(ValType::I32))?;
                    self.push_val(ValTypeOrUnknown::Val(ValType::I32));
                }
                Instruction::I32Eq => todo!(),
                Instruction::I32Ne => todo!(),
                Instruction::I32LtS => todo!(),
                Instruction::I32LtU => todo!(),
                Instruction::I32LeS => todo!(),
                Instruction::I32LeU => todo!(),
                Instruction::I32GtS => todo!(),
                Instruction::I32GtU => todo!(),
                Instruction::I32GeS => todo!(),
                Instruction::I32GeU => todo!(),
                Instruction::I32Clz => todo!(),
                Instruction::I32Ctz => todo!(),
                Instruction::I32Popcnt => todo!(),
                Instruction::I32Add => {
                    self.pop_val_expect(ValTypeOrUnknown::Val(ValType::I32))?;
                    self.pop_val_expect(ValTypeOrUnknown::Val(ValType::I32))?;
                    self.push_val(ValTypeOrUnknown::Val(ValType::I32));
                }
                Instruction::I32Sub => {
                    self.pop_val_expect(ValTypeOrUnknown::Val(ValType::I32))?;
                    self.pop_val_expect(ValTypeOrUnknown::Val(ValType::I32))?;
                    self.push_val(ValTypeOrUnknown::Val(ValType::I32));
                }
                Instruction::I32Mul => {
                    self.pop_val_expect(ValTypeOrUnknown::Val(ValType::I32))?;
                    self.pop_val_expect(ValTypeOrUnknown::Val(ValType::I32))?;
                    self.push_val(ValTypeOrUnknown::Val(ValType::I32));
                }
                Instruction::I32DivS => todo!(),
                Instruction::I32DivU => todo!(),
                Instruction::I32RemS => todo!(),
                Instruction::I32RemU => todo!(),
                Instruction::I32And => todo!(),
                Instruction::I32Or => todo!(),
                Instruction::I32Xor => todo!(),
                Instruction::I32Shl => todo!(),
                Instruction::I32ShrS => todo!(),
                Instruction::I32ShrU => todo!(),
                Instruction::I32Rotl => todo!(),
                Instruction::I32Rotr => todo!(),
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
                Validator::default().validate_function(
                    &type_section[*idx as usize],
                    &entry.body,
                    module,
                )
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
    #[ignore]
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
    #[ignore]
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
    #[ignore]
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
    #[ignore]
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
    #[ignore]
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
    #[ignore]
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
    #[ignore]
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
    #[ignore]
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
    #[ignore]
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
