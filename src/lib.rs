use binary::module::{self, Module};

use std::result;

mod binary;
mod instructions;
mod limits;
pub mod runtime;
mod validation;

pub use binary::MalformedError;
pub use binary::types::RefType;

use validation::ValidationError;

use crate::runtime::RuntimeError;

#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    Malformed(MalformedError),
    Invalid(ValidationError),
    Trap(RuntimeError),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Malformed(e) => write!(f, "malformed module: {}", e),
            Error::Invalid(e) => write!(f, "invalid module: {}", e),
            Error::Trap(e) => write!(f, "trap: {}", e),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Malformed(e) => Some(e),
            Self::Invalid(e) => Some(e),
            Self::Trap(e) => Some(e),
        }
    }
}

impl From<MalformedError> for Error {
    fn from(value: MalformedError) -> Self {
        Error::Malformed(value)
    }
}

impl From<ValidationError> for Error {
    fn from(value: ValidationError) -> Self {
        Error::Invalid(value)
    }
}

impl From<RuntimeError> for Error {
    fn from(value: RuntimeError) -> Self {
        Error::Trap(value)
    }
}

// Parse module
pub fn parse_module(bytes: &[u8]) -> result::Result<Module, Error> {
    Ok(module::decode_bytes(bytes.to_vec())?)
}

// Validate module
pub fn validate_module(m: &Module) -> result::Result<(), Error> {
    Ok(validation::validate_module(m)?)
}
