use std::{error, fmt, result};

use crate::validation::{IndexValidationError, ValidationError, Validator};
use crate::{FuncType, Module};

pub(crate) fn validate(module: &Module) -> result::Result<(), ValidationError> {
    let Some(ref startsec) = module.start else {
        return Ok(());
    };

    let functype = Validator::func_type_at(module, startsec.0).map_err(|e| {
        ValidationError::StartFunction(StartFunctionValidationError::UnknownFunction(e))
    })?;

    if !functype.params.is_empty() || !functype.results.is_empty() {
        return Err(ValidationError::StartFunction(
            StartFunctionValidationError::InvalidSignature(functype.clone()),
        ));
    }
    Ok(())
}

// Start Function Validation Error
#[derive(Debug)]
#[non_exhaustive]
pub enum StartFunctionValidationError {
    UnknownFunction(IndexValidationError),
    InvalidSignature(FuncType),
}

impl error::Error for StartFunctionValidationError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match &self {
            Self::UnknownFunction(e) => Some(e),
            Self::InvalidSignature { .. } => None,
        }
    }
}

impl fmt::Display for StartFunctionValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self {
            Self::UnknownFunction(_) => write!(f, "invalid function"),
            Self::InvalidSignature(functype) => {
                write!(f, "invalid signature, expected () -> (), got {functype}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ValType;
    use crate::binary::module::decode_bytes;
    use crate::validation::{IndexSpace, validate_module};

    #[test]
    fn test_start_defined_function() -> anyhow::Result<()> {
        let wat = r#"
            (module
                (func $main)
                (start $main))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(result.is_ok(), "expected Ok, got: {:?}", result);
        Ok(())
    }

    #[test]
    fn test_start_imported_function() -> anyhow::Result<()> {
        let wat = r#"
            (module
                (import "env" "init" (func $init))
                (start $init))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(result.is_ok(), "expected Ok, got: {:?}", result);
        Ok(())
    }

    #[test]
    fn test_start_unknown_function() -> anyhow::Result<()> {
        // One imported and one defined function, so index 2 is out of range.
        let wat = r#"
            (module
                (import "env" "init" (func))
                (func)
                (start 2))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            matches!(
                result,
                Err(ValidationError::StartFunction(
                    StartFunctionValidationError::UnknownFunction(IndexValidationError {
                        space: IndexSpace::Function,
                        index: 2,
                        count: 2
                    })
                ))
            ),
            "expected unknown function, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_start_function_with_params() -> anyhow::Result<()> {
        let wat = r#"
            (module
                (func $main (param i32))
                (start $main))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            matches!(
                &result,
                Err(ValidationError::StartFunction(
                    StartFunctionValidationError::InvalidSignature(FuncType { params, results })
                )) if params.as_slice() == [ValType::I32] && results.is_empty()
            ),
            "expected invalid signature, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_start_function_with_results() -> anyhow::Result<()> {
        let wat = r#"
            (module
                (func $main (result i32)
                    i32.const 0)
                (start $main))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            matches!(
                &result,
                Err(ValidationError::StartFunction(
                    StartFunctionValidationError::InvalidSignature(FuncType { params, results })
                )) if params.is_empty() && results.as_slice() == [ValType::I32]
            ),
            "expected invalid signature, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_start_imported_function_with_params() -> anyhow::Result<()> {
        let wat = r#"
            (module
                (import "env" "init" (func $init (param i64)))
                (start $init))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            matches!(
                &result,
                Err(ValidationError::StartFunction(
                    StartFunctionValidationError::InvalidSignature(FuncType { params, results })
                )) if params.as_slice() == [ValType::I64] && results.is_empty()
            ),
            "expected invalid signature, got: {:?}",
            result
        );
        Ok(())
    }
}
