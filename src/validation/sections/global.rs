use std::{error, fmt, result};

use crate::binary::module::Module;
use crate::validation::const_expr::{self, ConstExpressionValidationError};
use crate::validation::{ValidationError, ValueType};

/// Validate the Global section of a Module
///
/// For each global:
/// - The global's expression is constant and its type matches the global's type
pub(crate) fn validate(module: &Module) -> result::Result<(), ValidationError> {
    let Some(ref globalsec) = module.globals else {
        return Ok(());
    };

    for (idx, global) in globalsec.iter().enumerate() {
        const_expr::validate(module, &global.body, &[ValueType::from(global.gt.type_)]).map_err(
            |e| ValidationError::Global {
                index: module.imported_global_count() + idx,
                source: GlobalValidationError::InvalidConstExpression(e),
            },
        )?;
    }
    Ok(())
}

/// Global Validation error
#[derive(Debug)]
#[non_exhaustive]
pub enum GlobalValidationError {
    InvalidConstExpression(ConstExpressionValidationError),
}

impl error::Error for GlobalValidationError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::InvalidConstExpression(e) => Some(e),
        }
    }
}

impl fmt::Display for GlobalValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConstExpression(_) => write!(f, "validating const expression"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binary::module::decode_bytes;
    use crate::validation::{
        IndexSpace, IndexValidationError, ValidationErrorKind, validate_module,
    };

    #[test]
    fn test_global_ref_func_out_of_bounds() -> anyhow::Result<()> {
        // no functions defined, so index 5 is out of bounds
        let wat = r#"
            (module (global funcref (ref.func 5)))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            matches!(
                result,
                Err(ValidationError::Global {
                    index: 0,
                    source: GlobalValidationError::InvalidConstExpression(
                        ConstExpressionValidationError::Instruction {
                            index: _,
                            name: _,
                            source: ValidationErrorKind::InvalidIndex(IndexValidationError {
                                space: IndexSpace::Function,
                                index: 5,
                                count: 0
                            })
                        }
                    )
                })
            ),
            "expected invalid function index, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_global_valid_i32_const() -> anyhow::Result<()> {
        let wat = r#"
            (module (global i32 (i32.const 0)))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(result.is_ok(), "expected Ok, got: {:?}", result);
        Ok(())
    }

    #[test]
    fn test_global_type_mismatch() -> anyhow::Result<()> {
        let wat = r#"
            (module (global i32 (f32.const 0)))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            matches!(
                result,
                Err(ValidationError::Global {
                    index: 0,
                    source: GlobalValidationError::InvalidConstExpression(
                        ConstExpressionValidationError::ResultTypeMismatch { .. }
                    )
                })
            ),
            "expected TypeMismatch, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_global_error_index_includes_imports() -> anyhow::Result<()> {
        let wat = r#"
            (module
                (import "e" "g" (global i32))
                (global i32 (f32.const 0)))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            matches!(
                result,
                Err(ValidationError::Global {
                    index: 1,
                    source: GlobalValidationError::InvalidConstExpression(
                        ConstExpressionValidationError::ResultTypeMismatch { .. }
                    )
                })
            ),
            "expected type mismatch at global index 1 after imported global, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_global_non_const_instruction() -> anyhow::Result<()> {
        let wat = r#"
            (module (global f32 (f32.const 0) (f32.neg)))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            matches!(
                result,
                Err(ValidationError::Global {
                    index: 0,
                    source: GlobalValidationError::InvalidConstExpression(
                        ConstExpressionValidationError::InstructionNotAllowed {
                            index: 1,
                            name: "f32.neg"
                        }
                    )
                })
            ),
            "expected ConstantExpressionRequired, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_global_get_imported_immutable() -> anyhow::Result<()> {
        let wat = r#"
            (module
                (import "e" "g" (global i32))
                (global i32 (global.get 0)))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(result.is_ok(), "expected Ok, got: {:?}", result);
        Ok(())
    }

    #[test]
    fn test_global_get_locally_defined() -> anyhow::Result<()> {
        // global.get referencing a locally defined global is not allowed in const expr
        let wat = r#"
            (module
                (global i32 (i32.const 0))
                (global i32 (global.get 0)))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            matches!(
                result,
                Err(ValidationError::Global {
                    index: 1,
                    source: GlobalValidationError::InvalidConstExpression(
                        ConstExpressionValidationError::Instruction {
                            index: 0,
                            source: ValidationErrorKind::InvalidIndex(IndexValidationError {
                                space: IndexSpace::Global,
                                index: 0,
                                count: 0
                            }),
                            ..
                        }
                    )
                })
            ),
            "expected UnknownGlobal, got: {:?}",
            result
        );
        Ok(())
    }
    #[test]
    fn test_global_get_imported_global_in_function() -> anyhow::Result<()> {
        let wat = r#"
            (module
                (import "e" "g" (global i32))
                (func (result i32) global.get 0))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(result.is_ok(), "expected Ok, got: {:?}", result);
        Ok(())
    }

    #[test]
    fn test_global_get_imported_and_defined_globals() -> anyhow::Result<()> {
        let wat = r#"
            (module
                (import "env" "g" (global i32))
                (global i64 (i64.const 42))
                (func (export "get") (result i32 i64)
                    global.get 0
                    global.get 1))
        "#;
        let bytes = wat::parse_str(wat)?;

        // Distinct types ensure each index resolves to the correct global.
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(result.is_ok(), "expected Ok, got: {:?}", result);
        Ok(())
    }
}
