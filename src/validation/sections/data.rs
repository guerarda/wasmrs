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
