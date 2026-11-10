use std::{error, fmt, result};

use crate::binary::module::Module;
use crate::validation::{LimitValidationError, ValidationError, Validator};

/// Validate the Memory section of a Module
///
/// - At most, one memory is defined. Imported memories count
/// - Each memory's limit is valid
pub(crate) fn validate(module: &Module) -> result::Result<(), ValidationError> {
    let Some(ref memsec) = module.memories else {
        return Ok(());
    };

    if memsec.len() > 1 {
        return Err(ValidationError::Memory {
            index: 1,
            source: MemoryValidationError::MultipleMemories,
        });
    }

    for (index, mem) in memsec.iter().enumerate() {
        Validator::validate_memory_limit(mem).map_err(|e| ValidationError::Memory {
            index: index + module.imported_memory_count(),
            source: MemoryValidationError::InvalidLimit(e),
        })?;
    }

    Ok(())
}

/// Memory ValidationError
#[derive(Debug)]
#[non_exhaustive]
pub enum MemoryValidationError {
    MultipleMemories,
    InvalidLimit(LimitValidationError),
}

impl error::Error for MemoryValidationError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::MultipleMemories => None,
            Self::InvalidLimit(e) => Some(e),
        }
    }
}

impl fmt::Display for MemoryValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MultipleMemories => write!(f, "multiple memories declared, only one is allowed"),
            Self::InvalidLimit(_) => write!(f, "validating limit"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binary::module::decode_bytes;
    use crate::validation::validate_module;

    #[test]
    fn test_memory_min_exceeds_range() -> anyhow::Result<()> {
        let wat = r#"
            (module (memory 65537))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            matches!(
                result,
                Err(ValidationError::Memory {
                    index: 0,
                    source: MemoryValidationError::InvalidLimit(
                        LimitValidationError::MinExceedsRange {
                            min: 65537,
                            limit: 65536,
                        }
                    )
                })
            ),
            "expected MinExceedsRange, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_memory_max_exceeds_range() -> anyhow::Result<()> {
        let wat = r#"
            (module (memory 1 65537))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            matches!(
                result,
                Err(ValidationError::Memory {
                    index: 0,
                    source: MemoryValidationError::InvalidLimit(
                        LimitValidationError::MaxExceedsRange {
                            max: 65537,
                            limit: 65536,
                        }
                    )
                })
            ),
            "expected MaxExceedsRange, got: {:?}",
            result
        );
        Ok(())
    }

    #[test]
    fn test_memory_min_exceeds_max() -> anyhow::Result<()> {
        let wat = r#"
            (module (memory 2 1))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            matches!(
                result,
                Err(ValidationError::Memory {
                    index: 0,
                    source: MemoryValidationError::InvalidLimit(
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
    fn test_memory_multiple_memories() -> anyhow::Result<()> {
        let wat = r#"
            (module
                (memory 1)
                (memory 1))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            matches!(
                result,
                Err(ValidationError::Memory {
                    index: 1,
                    source: MemoryValidationError::MultipleMemories,
                })
            ),
            "expected MultipleMemories at memory index 1, got: {:?}",
            result
        );
        Ok(())
    }
}
