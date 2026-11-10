use std::{error, fmt, result};

use crate::RefType;
use crate::binary::module::Module;
use crate::validation::IndexValidationError;
use crate::validation::Validator;
use crate::validation::const_expr;

use crate::binary::sections::element::{ElementSegmentItems, ElementSegmentMode};
use crate::validation::const_expr::ConstExpressionValidationError;
use crate::validation::{ValidationErrorKind, ValueType};

/// Validate the Element section of a module
///
/// For each segment:
/// - Function indices exist, or each expression is constant and has type
///   `elemtype`
/// - If active, the target table exists and its type matches the segment's type
///   and the offset expression is constant with type `i32`
pub(crate) fn validate(module: &Module) -> result::Result<(), ValidationErrorKind> {
    let Some(ref elemsec) = module.elements else {
        return Ok(());
    };
    for e in elemsec {
        let rt = match &e.items {
            ElementSegmentItems::Functions(funcs) => {
                for f in funcs {
                    Validator::func_type_at(module, *f)?;
                }
                RefType::Func
            }
            ElementSegmentItems::Expressions(rt, expr) => {
                let expected = &[ValueType::from(*rt)];
                expr.iter()
                    .try_for_each(|e| const_expr::validate(module, e, expected))?;
                *rt
            }
        };

        if let ElementSegmentMode::Active {
            table_index,
            offset,
        } = &e.mode
        {
            let idx = table_index.unwrap_or(0);
            let table_type = Validator::table_type_at(module, idx)?;

            if table_type.elemtype != rt {
                return Err(ValidationErrorKind::type_mismatch());
            }

            const_expr::validate(module, offset, &[ValueType::I32])?;
        }
    }
    Ok(())
}

/// Element Validation Error
#[derive(Debug)]
#[non_exhaustive]
pub enum ElementValidationError {
    UnknownFunction {
        index: usize,
        source: IndexValidationError,
    },
    InvalidInitExpression {
        index: usize,
        source: ConstExpressionValidationError,
    },
    UnknownTable(IndexValidationError),
    TableTypeMismatch {
        expected: RefType,
        actual: RefType,
    },
    InvalidOffset(ConstExpressionValidationError),
}

impl error::Error for ElementValidationError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match &self {
            Self::UnknownFunction { index: _, source } => Some(source),
            Self::InvalidInitExpression { index: _, source } => Some(source),
            Self::UnknownTable(e) => Some(e),
            Self::TableTypeMismatch { .. } => None,
            Self::InvalidOffset(e) => Some(e),
        }
    }
}

impl fmt::Display for ElementValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self {
            Self::UnknownFunction { index, .. } => {
                write!(f, "validating function index at item {index}")
            }
            Self::InvalidInitExpression { index, .. } => {
                write!(f, "validating init expression at item {index}")
            }
            Self::UnknownTable(_) => write!(f, "validating table index"),
            Self::TableTypeMismatch { expected, actual } => {
                write!(
                    f,
                    "segment's type ({actual}) doesn't match the table's type ({expected})"
                )
            }
            Self::InvalidOffset(_) => {
                write!(f, "validating offset expression")
            }
        }
    }
}
