use std::collections::HashSet;
use std::result;

use crate::binary::module::Module;
use crate::binary::sections::export::ExportKind;
use crate::binary::types::MemIndex;
use crate::validation::{IndexSpace, IndexValidationError, ValidationErrorKind};

pub(crate) fn validate(module: &Module) -> result::Result<(), ValidationErrorKind> {
    let Some(ref exportsec) = module.exports else {
        return Ok(());
    };
    let mut names: HashSet<String> = HashSet::new();

    let func_count = module.func_count();
    let table_count = module.table_count();
    let mem_count = module.memory_count();
    let global_count = module.global_count();

    for e in exportsec {
        if !names.insert(e.name.clone()) {
            return Err(ValidationErrorKind::duplicate_export_name());
        }
        let idx = e.index as usize;
        match e.kind {
            ExportKind::Func => {
                if idx >= func_count {
                    return Err(IndexValidationError {
                        space: IndexSpace::Function,
                        index: idx,
                        count: func_count,
                    }
                    .into());
                }
            }
            ExportKind::Table => {
                if idx >= table_count {
                    return Err(IndexValidationError {
                        space: IndexSpace::Table,
                        index: idx,
                        count: table_count,
                    }
                    .into());
                }
            }
            ExportKind::Memory => {
                if idx >= mem_count {
                    return Err(
                        IndexValidationError::memory(MemIndex(idx as u32), mem_count).into(),
                    );
                }
            }
            ExportKind::Global => {
                if idx >= global_count {
                    return Err(ValidationErrorKind::unknown_global());
                }
            }
        }
    }

    Ok(())
}
