use crate::{
    binary::{
        reader::{FromReader, ReadError, Reader, Result},
        sections::{SectionEntry, SectionErrorKind},
        types::ValType,
    },
    instructions::{Instruction, decode_instruction},
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
        let size = reader.read_u32().map_err(SectionErrorKind::EntrySize)?;
        let mut reader = reader.scoped(size).map_err(SectionErrorKind::EntrySize)?;

        let locals: Vec<FuncLocal> = reader.read().map_err(SectionErrorKind::CodeFuncLocal)?;

        // There's a limit for number of functions locals
        // TODO It should consider function parameters as implicit locals
        let _ = locals
            .iter()
            .try_fold(0u32, |acc, &FuncLocal { count, .. }| acc.checked_add(count))
            .filter(|&n| n <= MAX_WASM_FUNCTION_LOCALS)
            .ok_or(SectionErrorKind::CodeFuncTooManyLocals)?;

        let mut body = Vec::new();
        while !reader.is_exhausted() {
            let instr = decode_instruction(&mut reader)?;
            body.push(instr);
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
