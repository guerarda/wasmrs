use std::{error, fmt, result};

use crate::binary::module::Module;
use crate::binary::sections::data::DataSegmentMode;
use crate::validation::const_expr::ConstExpressionValidationError;
use crate::validation::{IndexValidationError, ValidationError, ValueType, const_expr};

pub(crate) fn validate(module: &Module) -> result::Result<(), ValidationError> {
    let Some(ref datasec) = module.data else {
        return Ok(());
    };

    let mem_count = module.memory_count();
    for (index, d) in datasec.iter().enumerate() {
        if let DataSegmentMode::Active { mem_index, offset } = &d.mode {
            if (mem_index.as_usize()) >= mem_count {
                return Err(ValidationError::Data {
                    index,
                    source: DataValidationError::InvalidMemory(IndexValidationError::memory(
                        *mem_index, mem_count,
                    )),
                });
            }
            const_expr::validate(module, offset, &[ValueType::I32]).map_err(|e| {
                ValidationError::Data {
                    index,
                    source: DataValidationError::InvalidOffset(e),
                }
            })?;
        }
    }
    Ok(())
}

/// Data Validation Error
#[derive(Debug)]
pub enum DataValidationError {
    InvalidMemory(IndexValidationError),
    InvalidOffset(ConstExpressionValidationError),
}

impl error::Error for DataValidationError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match &self {
            Self::InvalidMemory(e) => Some(e),
            Self::InvalidOffset(e) => Some(e),
        }
    }
}
impl fmt::Display for DataValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self {
            Self::InvalidMemory(_) => write!(f, "invalid memory"),
            Self::InvalidOffset(_) => write!(f, "invalid offset"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binary::module::decode_bytes;
    use crate::validation::{IndexSpace, ValidationError, validate_module};

    #[test]
    fn test_data_segment_imported_memory() -> anyhow::Result<()> {
        let wat = r#"
            (module
                (import "env" "mem" (memory 1))
                (data (i32.const 0) "hi"))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(result.is_ok(), "expected Ok, got: {:?}", result);
        Ok(())
    }

    #[test]
    fn test_data_segment_unknown_memory() -> anyhow::Result<()> {
        // The data segment refers to memory index 1, which is out of range.
        let wat = r#"
            (module
                (import "env" "mem" (memory 1))
                (data (memory 1) (i32.const 0) "hi"))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            matches!(
                result,
                Err(ValidationError::Data {
                    index: 0,
                    source: DataValidationError::InvalidMemory(IndexValidationError {
                        space: IndexSpace::Memory,
                        index: 1,
                        count: 1
                    })
                })
            ),
            "expected invalid memory index, got: {:?}",
            result
        );
        Ok(())
    }
}
