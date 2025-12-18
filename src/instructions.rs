use crate::reader::{FromReader, InvalidEnumValueError, ReadError, Reader};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Instruction {
    Nop,
    End,

    LocalGet(u32),
    LocalSet(u32),
    LocalTee(u32),

    I32Const(i32),

    I32Add,
}

//impl Instruction {
pub fn decode_instruction(reader: &mut Reader) -> Result<Instruction, InstructionError> {
    let opcode = reader.read_u8().map_err(|e| InstructionError::from(e))?;
    match opcode {
        0x01 => Ok(Instruction::Nop),
        0x0b => Ok(Instruction::End),

        0x20 => {
            let idx: u32 = decode_arg(reader, "local.get")?;
            Ok(Instruction::LocalGet(idx))
        }
        0x21 => {
            let idx: u32 = decode_arg(reader, "local.set")?;
            Ok(Instruction::LocalSet(idx))
        }
        0x22 => {
            let idx: u32 = decode_arg(reader, "local.tee")?;
            Ok(Instruction::LocalTee(idx))
        }

        0x41 => {
            let v: i32 = decode_arg(reader, "i32.const")?;
            Ok(Instruction::I32Const(v))
        }

        0x6a => Ok(Instruction::I32Add),

        _ => Err(InstructionError {
            kind: InstructionErrorKind::InvalidOpCode(InvalidEnumValueError {
                value: opcode,
                enum_name: std::any::type_name::<Instruction>(),
            }),
            instr: None,
        }),
    }
}
//}

fn decode_arg<'a, T: FromReader<'a>>(
    reader: &mut Reader<'a>,
    instr: &'static str,
) -> Result<T, InstructionError> {
    let v = reader.read().map_err(|e| InstructionError {
        kind: InstructionErrorKind::ExpectedArgument(e),
        instr: Some(instr),
    })?;
    Ok(v)
}

#[derive(Debug)]
#[non_exhaustive]
pub struct InstructionError {
    kind: InstructionErrorKind,
    instr: Option<&'static str>,
}

#[derive(Debug)]
#[non_exhaustive]
pub enum InstructionErrorKind {
    Read(ReadError),
    InvalidOpCode(InvalidEnumValueError),
    ExpectedArgument(ReadError),
    ExpectedAdditionalArgument {
        idx: u32,
        of: u32,
        source: ReadError,
    },
}

impl std::fmt::Display for InstructionErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!()
    }
}

impl std::error::Error for InstructionErrorKind {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            InstructionErrorKind::Read(e) => Some(e),
            InstructionErrorKind::InvalidOpCode(e) => Some(e),
            InstructionErrorKind::ExpectedArgument(e) => Some(e),
            InstructionErrorKind::ExpectedAdditionalArgument {
                idx: _,
                of: _,
                source,
            } => Some(source),
        }
    }
}

impl From<ReadError> for InstructionError {
    fn from(value: ReadError) -> Self {
        InstructionError {
            kind: InstructionErrorKind::Read(value),
            instr: None,
        }
    }
}
