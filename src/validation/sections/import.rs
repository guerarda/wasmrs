use std::{error, fmt, result};

use crate::binary::module::Module;
use crate::binary::sections::import::ImportDesc;
use crate::validation::{IndexValidationError, LimitValidationError, ValidationError, Validator};

pub(crate) fn validate(module: &Module) -> result::Result<(), ValidationError> {
    let Some(ref importsec) = module.imports else {
        return Ok(());
    };
    let types_len = module.types.as_ref().map(|t| t.len()).unwrap_or(0);
    let mut mems_count = module.memories.as_ref().map(|t| t.len()).unwrap_or(0);

    for (index, i) in importsec.iter().enumerate() {
        match &i.desc {
            ImportDesc::Func(idx) => {
                if *idx as usize >= types_len {
                    return Err(ValidationError::Import {
                        index,
                        source: ImportValidationError::UnknownFunctionType(
                            IndexValidationError::ty(*idx, types_len),
                        ),
                    });
                }
            }
            ImportDesc::Mem(memtype) => {
                Validator::validate_memory_limit(memtype).map_err(|e| ValidationError::Import {
                    index,
                    source: ImportValidationError::InvalidMemory(e),
                })?;

                mems_count += 1;
                if mems_count > 1 {
                    return Err(ValidationError::Import {
                        index,
                        source: ImportValidationError::MultipleMemories,
                    });
                }
            }
            _ => (),
        }
    }
    Ok(())
}

/// Import Validation error
#[derive(Debug)]
#[non_exhaustive]
pub enum ImportValidationError {
    UnknownFunctionType(IndexValidationError),
    InvalidMemory(LimitValidationError),
    MultipleMemories,
}

impl error::Error for ImportValidationError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::UnknownFunctionType(e) => Some(e),
            Self::InvalidMemory(e) => Some(e),
            Self::MultipleMemories => None,
        }
    }
}

impl fmt::Display for ImportValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownFunctionType(_) => {
                write!(f, "validating imported function type")
            }
            Self::InvalidMemory(_) => {
                write!(f, "validating imported memory limit")
            }
            Self::MultipleMemories => write!(f, "multiple memories not allowed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binary::module::decode_bytes;
    use crate::validation::{IndexSpace, validate_module};

    #[test]
    fn test_import_unknown_function_type() -> anyhow::Result<()> {
        let wat = r#"
            (module
                (type (func))
                (import "e" "g" (global i32))
                (import "e" "f" (func (type 1))))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            matches!(
                result,
                Err(ValidationError::Import {
                    index: 1,
                    source: ImportValidationError::UnknownFunctionType(IndexValidationError {
                        space: IndexSpace::Type,
                        index: 1,
                        count: 1
                    })
                })
            ),
            "expected unknown function type at import index 1, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_import_memory_min_exceeds_range() -> anyhow::Result<()> {
        let wat = r#"
            (module
                (import "e" "g" (global i32))
                (import "e" "m" (memory 65537)))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            matches!(
                result,
                Err(ValidationError::Import {
                    index: 1,
                    source: ImportValidationError::InvalidMemory(
                        LimitValidationError::MinExceedsRange {
                            min: 65537,
                            limit: 65536
                        }
                    )
                })
            ),
            "expected min_exceeds_range at import index 1, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_import_memory_max_exceeds_range() -> anyhow::Result<()> {
        let wat = r#"
            (module
                (import "e" "g" (global i32))
                (import "e" "m" (memory 1 65537)))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            matches!(
                result,
                Err(ValidationError::Import {
                    index: 1,
                    source: ImportValidationError::InvalidMemory(
                        LimitValidationError::MaxExceedsRange {
                            max: 65537,
                            limit: 65536
                        }
                    )
                })
            ),
            "expected max_exceeds_range at import index 1, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_import_memory_min_exceeds_max() -> anyhow::Result<()> {
        let wat = r#"
            (module
                (import "e" "g" (global i32))
                (import "e" "m" (memory 2 1)))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            matches!(
                result,
                Err(ValidationError::Import {
                    index: 1,
                    source: ImportValidationError::InvalidMemory(
                        LimitValidationError::MinExceedsMax(2, 1)
                    )
                })
            ),
            "expected min_exceeds_max at import index 1, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_import_multiple_memories() -> anyhow::Result<()> {
        let cases = [
            (
                r#"
                    (module
                        (import "e" "g" (global i32))
                        (import "e" "m0" (memory 1))
                        (import "e" "m1" (memory 1)))
                "#,
                2,
            ),
            (
                r#"
                    (module
                        (import "e" "g" (global i32))
                        (import "e" "m" (memory 1))
                        (memory 1))
                "#,
                1,
            ),
        ];

        for (wat, expected_index) in cases {
            let bytes = wat::parse_str(wat)?;
            let m = decode_bytes(bytes)?;
            let result = validate_module(&m);
            assert!(
                matches!(
                    result,
                    Err(ValidationError::Import {
                        index,
                        source: ImportValidationError::MultipleMemories,
                    }) if index == expected_index
                ),
                "expected MultipleMemories at import index {}, got: {:?}",
                expected_index,
                result
            );
        }
        Ok(())
    }
}
