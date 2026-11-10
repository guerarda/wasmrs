use std::result;

use crate::RefType;
use crate::binary::module::Module;
use crate::validation::Validator;
use crate::validation::const_expr;

use crate::binary::sections::element::{ElementSegmentItems, ElementSegmentMode};
use crate::validation::{ValidationErrorKind, ValueType};

/// Validate the Element section of a module
///
/// For each segment:
/// - Function indices exist, or each expression is constant and has type
///   `elemtype`
/// - If active, the target table exists and its type matches the segment's type
///   and the offset expression is constant with type `i32`
pub(crate) fn validate(module: &Module) -> result::Result<(), ValidationErrorKind> {
    let Some(ref elemsec) = module.elements else {
        return Ok(());
    };
    for e in elemsec {
        let rt = match &e.items {
            ElementSegmentItems::Functions(funcs) => {
                for f in funcs {
                    Validator::func_type_at(module, *f)?;
                }
                RefType::Func
            }
            ElementSegmentItems::Expressions(rt, expr) => {
                let expected = &[ValueType::from(*rt)];
                expr.iter()
                    .try_for_each(|e| const_expr::validate(module, e, expected))?;
                *rt
            }
        };

        if let ElementSegmentMode::Active {
            table_index,
            offset,
        } = &e.mode
        {
            let idx = table_index.unwrap_or(0);
            let table_type = Validator::table_type_at(module, idx)?;

            if table_type.elemtype != rt {
                return Err(ValidationErrorKind::type_mismatch());
            }

            const_expr::validate(module, offset, &[ValueType::I32])?;
        }
    }
    Ok(())
}
