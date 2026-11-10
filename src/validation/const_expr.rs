use crate::binary::module::Module;
use crate::binary::sections::import::ImportDesc;
use crate::binary::types::ConstExpression;
use crate::instructions::Instruction;
use crate::validation::{
    IndexSpace, IndexValidationError, ValidationErrorKind, ValueType, fmt_value_type_vec,
};
use crate::{MutabilityFlag, RefType};

use std::{error, fmt, result};

/// Const Expression Validation Error
#[derive(Debug)]
#[non_exhaustive]
pub enum ConstExpressionValidationError {
    GlobalNotAllowed {
        inst_index: usize,
        global_index: usize,
    },
    InstructionNotAllowed {
        index: usize,
        name: &'static str,
    },
    Instruction {
        index: usize,
        name: &'static str,
        source: ValidationErrorKind,
    },
    ResultTypeMismatch {
        expected: Vec<ValueType>,
        actual: Vec<ValueType>,
    },
}

impl error::Error for ConstExpressionValidationError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Instruction {
                index: _,
                name: _,
                source,
            } => Some(source),
            _ => None,
        }
    }
}

impl fmt::Display for ConstExpressionValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Instruction { index, name, .. } => {
                write!(f, "validating instruction at index {index} ({name})")
            }
            Self::ResultTypeMismatch { expected, actual } => {
                write!(f, "type mismatch, expected:")?;
                fmt_value_type_vec(f, expected)?;
                write!(f, ", got:")?;
                fmt_value_type_vec(f, actual)
            }
            Self::GlobalNotAllowed {
                inst_index,
                global_index,
            } => {
                write!(
                    f,
                    "global.get({global_index}) at index {inst_index} is not allowed, constant expression required"
                )
            }
            Self::InstructionNotAllowed { index, name } => write!(
                f,
                "instruction at index {index} ({name}) is not allowed in a constant expression"
            ),
        }
    }
}

/// Validates that `expr` is constant and evaluates to the expected type.
/// Also enforces that global.get instructions only refer to allowed globals
/// Imported or previously defined for Globals, and imported
/// only for Tables.
pub(super) fn validate(
    module: &Module,
    expr: &ConstExpression,
    expected: &[ValueType],
) -> result::Result<(), ConstExpressionValidationError> {
    let mut stack = vec![];
    for (i, inst) in expr.0.iter().enumerate() {
        match inst {
            Instruction::I32Const(_) => stack.push(ValueType::I32),
            Instruction::I64Const(_) => stack.push(ValueType::I64),
            Instruction::F32Const(_) => stack.push(ValueType::F32),
            Instruction::F64Const(_) => stack.push(ValueType::F64),
            Instruction::RefNull(rt) => stack.push(ValueType::Ref(*rt)),
            Instruction::RefFunc(idx) => {
                if idx.as_usize() >= module.func_count() {
                    return Err(ConstExpressionValidationError::Instruction {
                        index: i,
                        name: inst.name(),
                        source: IndexValidationError {
                            space: IndexSpace::Function,
                            index: idx.as_usize(),
                            count: module.func_count(),
                        }
                        .into(),
                    });
                }
                stack.push(ValueType::Ref(RefType::Func))
            }
            Instruction::GlobalGet(idx) => {
                let idx = idx.0 as usize;

                if idx >= module.imported_global_count() {
                    return Err(ConstExpressionValidationError::Instruction {
                        index: i,
                        name: inst.name(),
                        source: ValidationErrorKind::unknown_global(),
                    });
                }

                let import = module
                    .imports
                    .as_ref()
                    .and_then(|imps| {
                        imps.iter()
                            .filter(|i| matches!(i.desc, ImportDesc::Global(_)))
                            .nth(idx)
                    })
                    .ok_or_else(|| ConstExpressionValidationError::Instruction {
                        index: i,
                        name: inst.name(),
                        source: ValidationErrorKind::unknown_global(),
                    })?;

                let ImportDesc::Global(gt) = &import.desc else {
                    unreachable!()
                };

                if matches!(gt.mutflag, MutabilityFlag::Const) {
                    stack.push(ValueType::from(gt.type_));
                } else {
                    return Err(ConstExpressionValidationError::GlobalNotAllowed {
                        inst_index: i,
                        global_index: idx,
                    });
                }
            }
            Instruction::End => break,
            _ => {
                return Err(ConstExpressionValidationError::InstructionNotAllowed {
                    index: i,
                    name: inst.name(),
                });
            }
        }
    }
    if stack == expected {
        Ok(())
    } else {
        Err(ConstExpressionValidationError::ResultTypeMismatch {
            expected: expected.to_vec(),
            actual: stack,
        })
    }
}
