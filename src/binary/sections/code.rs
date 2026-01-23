use core::fmt;
use std::error;

use crate::{
    binary::{
        reader::{FromReader, ReadError, Reader, Result, VecReadError},
        sections::{SectionEntry, SectionErrorKind},
        types::ValType,
    },
    instructions::{Instruction, InstructionError, decode_instruction},
    limits::MAX_WASM_FUNCTION_LOCALS,
};

/// Code Section
#[derive(Debug)]
pub struct FuncLocal {
    pub count: u32,
    pub valtype: ValType,
}

#[derive(Debug)]
pub struct CodeEntry {
    #[allow(dead_code)]
    pub size: usize,
    pub locals: Vec<FuncLocal>,
    pub body: Vec<Instruction>,
}

pub type CodeSection = Vec<CodeEntry>;

impl SectionEntry for CodeEntry {
    fn decode(reader: &mut Reader) -> std::result::Result<Self, SectionErrorKind> {
        let size = reader
            .read_u32()
            .map_err(CodeSectionReadError::FunctionCodeSize)?;
        let mut reader = reader
            .scoped(size)
            .map_err(CodeSectionReadError::FunctionCodeSize)?;

        let locals: Vec<FuncLocal> = reader
            .read()
            .map_err(CodeSectionReadError::FunctionLocals)?;

        // There's a limit for number of functions locals
        // TODO It should consider function parameters as implicit locals
        let _ = locals
            .iter()
            .try_fold(0u32, |acc, &FuncLocal { count, .. }| acc.checked_add(count))
            .filter(|&n| n <= MAX_WASM_FUNCTION_LOCALS)
            .ok_or(CodeSectionReadError::TooManyLocals)?;

        let mut body = Vec::new();
        let mut depth: u32 = 1; // The function itself is a block, closed by the final END.

        while !reader.is_exhausted() {
            let instr =
                decode_instruction(&mut reader).map_err(CodeSectionReadError::FunctionBody)?;

            match instr {
                Instruction::Block(_) | Instruction::Loop(_) | Instruction::If(_) => {
                    depth += 1;
                }
                Instruction::End => {
                    depth -= 1;
                }
                _ => (),
            }
            body.push(instr);
        }
        if depth == 0 {
            Ok(CodeEntry {
                size: size as usize,
                locals,
                body,
            })
        } else {
            Err(CodeSectionReadError::MissingEnd.into())
        }
    }
}

impl<'a> FromReader<'a> for FuncLocal {
    type Error = ReadError;

    fn from_reader(reader: &mut Reader<'a>) -> Result<Self> {
        Ok(FuncLocal {
            count: reader.read()?,
            valtype: reader.read()?,
        })
    }
}

/// Errors
#[derive(Debug)]
#[non_exhaustive]
pub enum CodeSectionReadError {
    FunctionCodeSize(ReadError),
    FunctionLocals(VecReadError<ReadError>),
    TooManyLocals,
    FunctionBody(InstructionError),
    MissingEnd,
}

impl error::Error for CodeSectionReadError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::FunctionCodeSize(e) => Some(e),
            Self::FunctionBody(e) => Some(e),
            Self::TooManyLocals => None,
            Self::FunctionLocals(e) => Some(e),
            Self::MissingEnd => None,
        }
    }
}

impl fmt::Display for CodeSectionReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FunctionCodeSize(_) => write!(f, "reading the function code byte size"),
            Self::FunctionLocals(_) => write!(f, "reading function local"),
            Self::TooManyLocals => write!(f, "checking function locals count"),
            Self::FunctionBody(_) => write!(f, "reading function body"),
            Self::MissingEnd => write!(f, "missing end instruction (0x0b)"),
        }
    }
}

impl From<CodeSectionReadError> for SectionErrorKind {
    fn from(value: CodeSectionReadError) -> Self {
        SectionErrorKind::CodeSection(value)
    }
}
