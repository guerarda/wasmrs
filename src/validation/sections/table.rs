use std::{error, fmt, result};

use crate::binary::module::Module;
use crate::validation::const_expr::{self, ConstExpressionValidationError};
use crate::validation::{LimitValidationError, ValidationError, ValueType};

/// Validate the Table section of a Module
///
/// For each table:
/// - The limits are valid (`min <= max`)
/// - If present, the init expression is constant and has the type `elemetype`
/// - If missing, stands for `ref.null` which is always valid, as every
///   supported reference type is nullable
pub fn validate(module: &Module) -> result::Result<(), ValidationError> {
    let Some(ref tablesec) = module.tables else {
        return Ok(());
    };

    for (index, table) in tablesec.iter().enumerate() {
        // Validate the table limit
        let l = &table.tabletype.limit;
        if let Some(max) = l.max
            && l.min > max
        {
            return Err(ValidationError::Table {
                index,
                source: TableValidationError::InvalidLimit(LimitValidationError::MinExceedsMax(
                    l.min, max,
                )),
            });
        }

        // Validate that the expression, if present is a valid const
        // expression and evaluate to the table's declared type
        if let Some(expr) = &table.expr {
            const_expr::validate(module, expr, &[ValueType::from(table.tabletype.elemtype)])
                .map_err(|e| ValidationError::Table {
                    index,
                    source: TableValidationError::InvalidConstExpression(e),
                })?;
        }
    }
    Ok(())
}

/// Table ValidationError
#[derive(Debug)]
#[non_exhaustive]
pub enum TableValidationError {
    InvalidConstExpression(ConstExpressionValidationError),
    InvalidLimit(LimitValidationError),
}

impl error::Error for TableValidationError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::InvalidConstExpression(e) => Some(e),
            Self::InvalidLimit(e) => Some(e),
        }
    }
}

impl fmt::Display for TableValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConstExpression(_) => write!(f, "validating const expression"),
            Self::InvalidLimit(_) => write!(f, "validating limit"),
        }
    }
}
