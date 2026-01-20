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
    fn is_num(&self) -> bool {
        matches!(
            self,
            Self::Val(ValType::I32 | ValType::I64 | ValType::F32 | ValType::F64) | Self::Unknown
        )
    }

    fn is_vec(&self) -> bool {
        matches!(self, Self::Val(ValType::V128) | Self::Unknown)
    }

    fn is_ref(&self) -> bool {
        matches!(self, Self::Val(ValType::Ref(_)) | Self::Unknown)
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
        let last = self.ctrls.last().unwrap();
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
            .pop()
            .ok_or(ValidationError::ControlStackUnderflow)?;

        let _ = self.pop_vals_expect(&frame.end_types)?;
        if self.vals.len() != frame.height {
            return Err(ValidationError::ControlStackUnderflow);
        }
        Ok(frame)
    }

    fn label_types(frame: CtrlFrame) -> Vec<ValTypeOrUnknown> {
        // TODO Loop
        frame.end_types
    }

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
        _functype: &FuncType,
        _body: &[Instruction],
    ) -> result::Result<(), ValidationError> {
        Err(ValidationError::ControlStackUnderflow)
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
                Validator::default().validate_function(&type_section[*idx as usize], &entry.body)
            })
    }
}

pub fn validate_module(module: &Module) -> result::Result<(), ValidationError> {
    Validator::validate_module(module)
}
