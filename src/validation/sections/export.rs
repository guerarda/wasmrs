use std::collections::HashSet;
use std::{error, fmt, result};

use crate::binary::module::Module;
use crate::binary::sections::export::ExportDesc;
use crate::validation::{IndexSpace, IndexValidationError, ValidationError};

pub(crate) fn validate(module: &Module) -> result::Result<(), ValidationError> {
    let Some(ref exportsec) = module.exports else {
        return Ok(());
    };
    let mut names: HashSet<String> = HashSet::new();

    let func_count = module.func_count();
    let table_count = module.table_count();
    let mem_count = module.memory_count();
    let global_count = module.global_count();

    for (index, e) in exportsec.iter().enumerate() {
        if !names.insert(e.name.clone()) {
            return Err(ValidationError::Export {
                index,
                source: ExportValidationError::DuplicateName(e.name.clone()),
            });
        }
        match e.desc {
            ExportDesc::Func(idx) if idx.as_usize() >= func_count => {
                return Err(ValidationError::Export {
                    index,
                    source: IndexValidationError::function(idx, func_count).into(),
                });
            }

            ExportDesc::Table(idx) if idx.as_usize() >= table_count => {
                return Err(ValidationError::Export {
                    index,
                    source: IndexValidationError::table(idx, table_count).into(),
                });
            }
            ExportDesc::Memory(idx) if idx.as_usize() >= mem_count => {
                return Err(ValidationError::Export {
                    index,
                    source: IndexValidationError::memory(idx, mem_count).into(),
                });
            }
            ExportDesc::Global(idx) if idx.as_usize() >= global_count => {
                return Err(ValidationError::Export {
                    index,
                    source: IndexValidationError::global(idx, global_count).into(),
                });
            }
            _ => (),
        }
    }

    Ok(())
}

/// Export Validation Error
#[derive(Debug)]
#[non_exhaustive]
pub enum ExportValidationError {
    DuplicateName(String),
    UnknownFunction(IndexValidationError),
    UnknownTable(IndexValidationError),
    UnknownMemory(IndexValidationError),
    UnknownGlobal(IndexValidationError),
}

impl From<IndexValidationError> for ExportValidationError {
    fn from(value: IndexValidationError) -> Self {
        match value.space {
            IndexSpace::Function => Self::UnknownFunction(value),
            IndexSpace::Table => Self::UnknownTable(value),
            IndexSpace::Memory => Self::UnknownMemory(value),
            IndexSpace::Global => Self::UnknownGlobal(value),
            _ => unreachable!(),
        }
    }
}

impl error::Error for ExportValidationError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match &self {
            Self::DuplicateName(_) => None,
            Self::UnknownFunction(e) => Some(e),
            Self::UnknownTable(e) => Some(e),
            Self::UnknownMemory(e) => Some(e),
            Self::UnknownGlobal(e) => Some(e),
        }
    }
}

impl fmt::Display for ExportValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self {
            Self::DuplicateName(name) => write!(f, "duplicate export with name '{name}'"),
            Self::UnknownFunction(_) => write!(f, "unknwon function"),
            Self::UnknownTable(_) => write!(f, "unknown table "),
            Self::UnknownMemory(_) => write!(f, "unknown memory"),
            Self::UnknownGlobal(_) => write!(f, "unknown global"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binary::module::decode_bytes;
    use crate::validation::validate_module;

    #[test]
    fn test_export_unknown_memory() -> anyhow::Result<()> {
        // The module has no memory, so exporting memory 0 is out of range.
        let wat = r#"
            (module (export "m" (memory 0)))
        "#;
        let bytes = wat::parse_str(wat)?;
        let m = decode_bytes(bytes)?;
        let result = validate_module(&m);
        assert!(
            matches!(
                result,
                Err(ValidationError::Export {
                    index: 0,
                    source: ExportValidationError::UnknownMemory(IndexValidationError {
                        space: IndexSpace::Memory,
                        index: 0,
                        count: 0
                    })
                })
            ),
            "expected invalid memory index, got: {:?}",
            result
        );
        Ok(())
    }
}
