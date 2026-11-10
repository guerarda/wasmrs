use std::result;

use crate::Module;
use crate::validation::{ValidationErrorKind, Validator};

pub(crate) fn validate(module: &Module) -> result::Result<(), ValidationErrorKind> {
    let Some(ref startsec) = module.start else {
        return Ok(());
    };

    let functype = Validator::func_type_at(module, startsec.0)?;

    if !functype.params.is_empty() || !functype.results.is_empty() {
        return Err(ValidationErrorKind::start_function());
    }
    Ok(())
}
