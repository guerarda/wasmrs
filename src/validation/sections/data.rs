use std::result;

use crate::binary::module::Module;
use crate::binary::sections::data::DataSegmentMode;
use crate::binary::types::MemIndex;
use crate::validation::{IndexValidationError, ValidationErrorKind, ValueType, const_expr};

pub(crate) fn validate(module: &Module) -> result::Result<(), ValidationErrorKind> {
    let Some(ref datasec) = module.data else {
        return Ok(());
    };

    let mem_count = module.memory_count();
    for d in datasec {
        if let DataSegmentMode::Active { mem_index, offset } = &d.mode {
            if (*mem_index as usize) >= mem_count {
                return Err(IndexValidationError::memory(MemIndex(*mem_index), mem_count).into());
            }
            const_expr::validate(module, offset, &[ValueType::I32])?;
        }
    }
    Ok(())
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
                Err(ValidationError::Module(ValidationErrorKind::InvalidIndex(
                    IndexValidationError {
                        space: IndexSpace::Memory,
                        index: 1,
                        count: 1
                    }
                )))
            ),
            "expected invalid memory index, got: {:?}",
            result
        );
        Ok(())
    }
}
