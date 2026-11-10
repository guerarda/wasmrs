use std::{error, fmt, result};

use crate::binary::module::Module;
use crate::validation::const_expr::{self, ConstExpressionValidationError};
use crate::validation::{LimitValidationError, ValidationError, ValueType};

/// Validate the Table section of a Module
///
/// For each table:
/// - The limits are valid (`min <= max`)
/// - If present, the init expression is constant and has the type `elemtype`
/// - If missing, stands for `ref.null` which is always valid, as every
///   supported reference type is nullable
pub(crate) fn validate(module: &Module) -> result::Result<(), ValidationError> {
    let Some(ref tablesec) = module.tables else {
        return Ok(());
    };

    for (index, table) in tablesec.iter().enumerate() {
        // Validate the table limit
        let l = &table.tabletype.limit;
        if let Some(max) = l.max
            && l.min > max
        {
            return Err(ValidationError::Table {
                index,
                source: TableValidationError::InvalidLimit(LimitValidationError::MinExceedsMax(
                    l.min, max,
                )),
            });
        }

        // Validate that the expression, if present is a valid const
        // expression and evaluate to the table's declared type
        if let Some(expr) = &table.expr {
            const_expr::validate(module, expr, &[ValueType::from(table.tabletype.elemtype)])
                .map_err(|e| ValidationError::Table {
                    index,
                    source: TableValidationError::InvalidConstExpression(e),
                })?;
        }
    }
    Ok(())
}

/// Table ValidationError
#[derive(Debug)]
#[non_exhaustive]
pub enum TableValidationError {
    InvalidConstExpression(ConstExpressionValidationError),
    InvalidLimit(LimitValidationError),
}

impl error::Error for TableValidationError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::InvalidConstExpression(e) => Some(e),
            Self::InvalidLimit(e) => Some(e),
        }
    }
}

impl fmt::Display for TableValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConstExpression(_) => write!(f, "validating const expression"),
            Self::InvalidLimit(_) => write!(f, "validating limit"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RefType;
    use crate::binary::module::decode_bytes;
    use crate::validation::{
        IndexSpace, IndexValidationError, ValidationErrorKind, validate_module,
    };

    #[test]
    fn test_table_min_exceeds_max() -> anyhow::Result<()> {
        let wat = r#"
            (module (table 2 1 funcref))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            matches!(
                result,
                Err(ValidationError::Table {
                    index: 0,
                    source: TableValidationError::InvalidLimit(
                        LimitValidationError::MinExceedsMax(2, 1)
                    )
                })
            ),
            "expected MinExceedsMax, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_table_initializer_type_mismatch() -> anyhow::Result<()> {
        let wat = r#"
            (module (table 1 funcref (ref.null extern)))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            matches!(
                &result,
                Err(ValidationError::Table {
                    index: 0,
                    source: TableValidationError::InvalidConstExpression(
                        ConstExpressionValidationError::ResultTypeMismatch { expected, actual }
                    )
                }) if expected == &[ValueType::Ref(RefType::Func)]
                    && actual == &[ValueType::Ref(RefType::Extern)]
            ),
            "expected table initializer type mismatch, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_table_ref_func_out_of_bounds() -> anyhow::Result<()> {
        let wat = r#"
            (module (table 1 funcref (ref.func 5)))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            matches!(
                result,
                Err(ValidationError::Table {
                    index: 0,
                    source: TableValidationError::InvalidConstExpression(
                        ConstExpressionValidationError::Instruction {
                            index: 0,
                            source: ValidationErrorKind::InvalidIndex(IndexValidationError {
                                space: IndexSpace::Function,
                                index: 5,
                                count: 0
                            }),
                            ..
                        }
                    )
                })
            ),
            "expected invalid function index, got: {:?}",
            result
        );
        Ok(())
    }
}
