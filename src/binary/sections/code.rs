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

        enum CtrlFrame {
            Function,
            Block,
            Loop,
            If(bool),
        }
        let mut stack: Vec<CtrlFrame> = vec![CtrlFrame::Function];
        let mut body = vec![];

        while !reader.is_exhausted() && !stack.is_empty() {
            let instr =
                decode_instruction(&mut reader).map_err(CodeSectionReadError::FunctionBody)?;

            match instr {
                Instruction::Block(_) => stack.push(CtrlFrame::Block),
                Instruction::Loop(_) => stack.push(CtrlFrame::Loop),
                Instruction::If(_) => stack.push(CtrlFrame::If(false)),
                Instruction::Else => {
                    let top = stack
                        .last_mut()
                        .ok_or(CodeSectionReadError::ElseOutsideIfBlock)?;
                    match &top {
                        CtrlFrame::If(false) => {
                            *top = CtrlFrame::If(true);
                        }
                        _ => return Err(CodeSectionReadError::ElseOutsideIfBlock.into()),
                    }
                }
                Instruction::End => {
                    let _ = stack.pop();
                }
                _ => (),
            }
            body.push(instr);
        }

        if !stack.is_empty() {
            return Err(CodeSectionReadError::MissingEnd.into());
        }

        if !reader.is_exhausted() {
            return Err(SectionErrorKind::SectionSizeMismatch {
                end: reader.position() as usize,
                expected: reader.end() as usize,
            });
        }

        Ok(CodeEntry {
            size: size as usize,
            locals,
            body,
        })
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
    ElseOutsideIfBlock,
}

impl error::Error for CodeSectionReadError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::FunctionCodeSize(e) => Some(e),
            Self::FunctionBody(e) => Some(e),
            Self::TooManyLocals => None,
            Self::FunctionLocals(e) => Some(e),
            Self::MissingEnd => None,
            Self::ElseOutsideIfBlock => None,
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
            Self::ElseOutsideIfBlock => write!(f, "else outside if block"),
        }
    }
}

impl From<CodeSectionReadError> for SectionErrorKind {
    fn from(value: CodeSectionReadError) -> Self {
        SectionErrorKind::CodeSection(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_else_outside_if_block() -> anyhow::Result<()> {
        // Body size: 3 bytes; no locals, else, end.
        let bytes = b"\x03\x00\x05\x0b";
        let mut reader = Reader::from_bytes(bytes, 0);

        let result = CodeEntry::decode(&mut reader);
        assert!(
            matches!(
                &result,
                Err(SectionErrorKind::CodeSection(
                    CodeSectionReadError::ElseOutsideIfBlock
                ))
            ),
            "expected else outside if block error, got: {result:?}"
        );

        Ok(())
    }

    #[test]
    fn test_duplicate_else() -> anyhow::Result<()> {
        // Body size: 9 bytes; no locals, i32.const 1, if (empty type),
        // else, duplicate else, end of if, end of function.
        let bytes = b"\x09\x00\x41\x01\x04\x40\x05\x05\x0b\x0b";
        let mut reader = Reader::from_bytes(bytes, 0);

        let result = CodeEntry::decode(&mut reader);
        assert!(
            matches!(
                &result,
                Err(SectionErrorKind::CodeSection(
                    CodeSectionReadError::ElseOutsideIfBlock
                ))
            ),
            "expected error for duplicate else, got: {:?}",
            result
        );

        Ok(())
    }

    #[test]
    fn test_else_inside_block_or_loop_nested_in_if() -> anyhow::Result<()> {
        // Each body is 11 bytes: no locals, i32.const 1, if (empty type),
        // block/loop (empty type), misplaced else, then three ends.
        let cases = [
            ("block", b"\x0b\x00\x41\x01\x04\x40\x02\x40\x05\x0b\x0b\x0b"),
            ("loop", b"\x0b\x00\x41\x01\x04\x40\x03\x40\x05\x0b\x0b\x0b"),
        ];

        for (name, bytes) in cases {
            let mut reader = Reader::from_bytes(bytes, 0);
            let result = CodeEntry::decode(&mut reader);
            assert!(
                matches!(
                    &result,
                    Err(SectionErrorKind::CodeSection(
                        CodeSectionReadError::ElseOutsideIfBlock
                    ))
                ),
                "expected error for else inside nested {name}, got: {:?}",
                result
            );
        }

        Ok(())
    }

    #[test]
    fn test_instruction_after_function_end() -> anyhow::Result<()> {
        // Body size: 3 bytes; no locals, end, trailing nop.
        let bytes = b"\x03\x00\x0b\x01";
        let mut reader = Reader::from_bytes(bytes, 0);

        let result = CodeEntry::decode(&mut reader);
        assert!(
            matches!(&result, Err(SectionErrorKind::SectionSizeMismatch { .. })),
            "expected section size mismatch for instruction after function end, got: {result:?}"
        );

        Ok(())
    }
}
