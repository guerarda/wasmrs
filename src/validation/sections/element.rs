use std::{error, fmt, result};

use crate::RefType;
use crate::binary::module::Module;
use crate::validation::{IndexValidationError, Validator, const_expr};

use crate::binary::sections::element::{ElementSegmentItems, ElementSegmentMode};
use crate::validation::const_expr::ConstExpressionValidationError;
use crate::validation::{ValidationError, ValueType};

/// Validate the Element section of a module
///
/// For each segment:
/// - Function indices exist, or each expression is constant and has type
///   `elemtype`
/// - If active, the target table exists and its type matches the segment's type
///   and the offset expression is constant with type `i32`
pub(crate) fn validate(module: &Module) -> result::Result<(), ValidationError> {
    let Some(ref elemsec) = module.elements else {
        return Ok(());
    };

    for (index, element) in elemsec.iter().enumerate() {
        let rt = match &element.items {
            ElementSegmentItems::Functions(funcs) => {
                for (f_index, f) in funcs.iter().enumerate() {
                    Validator::func_type_at(module, *f).map_err(|e| ValidationError::Element {
                        index,
                        source: ElementValidationError::UnknownFunction {
                            index: f_index,
                            source: e,
                        },
                    })?;
                }
                RefType::Func
            }
            ElementSegmentItems::Expressions(rt, exprs) => {
                let expected = &[ValueType::from(*rt)];
                for (expr_index, expr) in exprs.iter().enumerate() {
                    const_expr::validate(module, expr, expected).map_err(|e| {
                        ValidationError::Element {
                            index,
                            source: ElementValidationError::InvalidInitExpression {
                                index: expr_index,
                                source: e,
                            },
                        }
                    })?
                }
                *rt
            }
        };

        if let ElementSegmentMode::Active {
            table_index,
            offset,
        } = &element.mode
        {
            let idx = table_index.unwrap_or(0);
            let table_type =
                Validator::table_type_at(module, idx).map_err(|e| ValidationError::Element {
                    index,
                    source: ElementValidationError::UnknownTable(e),
                })?;

            if table_type.elemtype != rt {
                return Err(ValidationError::Element {
                    index,
                    source: ElementValidationError::TableTypeMismatch {
                        expected: table_type.elemtype,
                        actual: rt,
                    },
                });
            }

            const_expr::validate(module, offset, &[ValueType::I32]).map_err(|e| {
                ValidationError::Element {
                    index,
                    source: ElementValidationError::InvalidOffset(e),
                }
            })?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binary::module::decode_bytes;
    use crate::validation::{IndexSpace, validate_module};

    #[test]
    fn test_element_valid_active_and_passive() -> anyhow::Result<()> {
        let wat = r#"
            (module
                (table 1 funcref)
                (func)
                (elem (i32.const 0) func 0)
                (elem funcref (ref.func 0) (ref.null func)))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(result.is_ok(), "expected Ok, got: {:?}", result);
        Ok(())
    }

    #[test]
    fn test_element_unknown_function() -> anyhow::Result<()> {
        // Second segment, second item refers to function 1, only one exists
        let wat = r#"
            (module
                (table 2 funcref)
                (func)
                (elem (i32.const 0) func 0)
                (elem (i32.const 0) func 0 1))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            matches!(
                result,
                Err(ValidationError::Element {
                    index: 1,
                    source: ElementValidationError::UnknownFunction {
                        index: 1,
                        source: IndexValidationError {
                            space: IndexSpace::Function,
                            index: 1,
                            count: 1,
                        },
                    },
                })
            ),
            "expected unknown function at segment 1 item 1, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_element_init_expression_type_mismatch() -> anyhow::Result<()> {
        // First segment, second item is an externref in a funcref segment
        let wat = r#"
            (module
                (elem funcref (ref.null func) (ref.null extern)))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            matches!(
                result,
                Err(ValidationError::Element {
                    index: 0,
                    source: ElementValidationError::InvalidInitExpression {
                        index: 1,
                        source: ConstExpressionValidationError::ResultTypeMismatch { .. },
                    },
                })
            ),
            "expected init expression type mismatch at segment 0 item 1, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_element_unknown_table() -> anyhow::Result<()> {
        let wat = r#"
            (module
                (table 1 funcref)
                (func)
                (elem (table 1) (i32.const 0) func 0))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            matches!(
                result,
                Err(ValidationError::Element {
                    index: 0,
                    source: ElementValidationError::UnknownTable(IndexValidationError {
                        space: IndexSpace::Table,
                        index: 1,
                        count: 1,
                    }),
                })
            ),
            "expected unknown table, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_element_table_type_mismatch() -> anyhow::Result<()> {
        let wat = r#"
            (module
                (table 1 externref)
                (func)
                (elem (i32.const 0) func 0))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            matches!(
                result,
                Err(ValidationError::Element {
                    index: 0,
                    source: ElementValidationError::TableTypeMismatch {
                        expected: RefType::Extern,
                        actual: RefType::Func,
                    },
                })
            ),
            "expected table type mismatch, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_element_offset_not_i32() -> anyhow::Result<()> {
        let wat = r#"
            (module
                (table 1 funcref)
                (func)
                (elem (i64.const 0) func 0))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            matches!(
                result,
                Err(ValidationError::Element {
                    index: 0,
                    source: ElementValidationError::InvalidOffset(
                        ConstExpressionValidationError::ResultTypeMismatch { .. }
                    ),
                })
            ),
            "expected invalid offset, got: {:?}",
            result
        );
        Ok(())
    }
}
