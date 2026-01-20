use crate::binary::{
    reader::{FromReader, InvalidEnumValueError, ReadError, Reader},
    types::{FuncIdx, RefType, ValType},
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Instruction {
    Nop,

    //Block,
    //Loop,
    If(ValType),
    Else,

    End,

    Call(u32),
    Drop,

    LocalGet(u32),
    LocalSet(u32),
    LocalTee(u32),

    I32Const(i32),
    I64Const(i64),

    I32Eqz,
    I32Eq,
    I32Ne,
    I32LtS,
    I32LtU,
    I32LeS,
    I32LeU,
    I32GtS,
    I32GtU,
    I32GeS,
    I32GeU,

    I32Clz,
    I32Ctz,
    I32Popcnt,
    I32Add,
    I32Sub,
    I32Mul,
    I32DivS,
    I32DivU,
    I32RemS,
    I32RemU,
    I32And,
    I32Or,
    I32Xor,
    I32Shl,
    I32ShrS,
    I32ShrU,
    I32Rotl,
    I32Rotr,

    I32Extend8S,
    I32Extend16S,

    RefNull(RefType),
    RefFunc(FuncIdx),
}

pub fn decode_instruction(reader: &mut Reader) -> Result<Instruction, InstructionError> {
    let offset = reader.position() as usize;
    let opcode = reader.read_u8()?;
    match opcode {
        0x01 => Ok(Instruction::Nop),
        0x04 => {
            let bt: ValType = decode_arg(reader, "if")?;
            Ok(Instruction::If(bt))
        }
        0x05 => Ok(Instruction::Else),
        0x0b => Ok(Instruction::End),

        0x10 => {
            let idx: u32 = decode_arg(reader, "call")?;
            Ok(Instruction::Call(idx))
        }
        0x1a => Ok(Instruction::Drop),

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

        0x42 => {
            let v: i64 = decode_arg(reader, "i64.const")?;
            Ok(Instruction::I64Const(v))
        }

        0x45 => Ok(Instruction::I32Eqz),
        0x46 => Ok(Instruction::I32Eq),
        0x47 => Ok(Instruction::I32Ne),
        0x48 => Ok(Instruction::I32LtS),
        0x49 => Ok(Instruction::I32LtU),
        0x4a => Ok(Instruction::I32GtS),
        0x4b => Ok(Instruction::I32GtU),
        0x4c => Ok(Instruction::I32LeS),
        0x4d => Ok(Instruction::I32LeU),
        0x4e => Ok(Instruction::I32GeS),
        0x4f => Ok(Instruction::I32GeU),

        0x67 => Ok(Instruction::I32Clz),
        0x68 => Ok(Instruction::I32Ctz),
        0x69 => Ok(Instruction::I32Popcnt),
        0x6a => Ok(Instruction::I32Add),
        0x6b => Ok(Instruction::I32Sub),
        0x6c => Ok(Instruction::I32Mul),
        0x6d => Ok(Instruction::I32DivS),
        0x6e => Ok(Instruction::I32DivU),
        0x6f => Ok(Instruction::I32RemS),
        0x70 => Ok(Instruction::I32RemU),
        0x71 => Ok(Instruction::I32And),
        0x72 => Ok(Instruction::I32Or),
        0x73 => Ok(Instruction::I32Xor),
        0x74 => Ok(Instruction::I32Shl),
        0x75 => Ok(Instruction::I32ShrS),
        0x76 => Ok(Instruction::I32ShrU),
        0x77 => Ok(Instruction::I32Rotl),
        0x78 => Ok(Instruction::I32Rotr),

        0xc0 => Ok(Instruction::I32Extend8S),
        0xc1 => Ok(Instruction::I32Extend16S),

        0xd0 => {
            let rt: RefType = decode_arg(reader, "ref.null")?;
            Ok(Instruction::RefNull(rt))
        }
        0xd2 => {
            let fi: FuncIdx = decode_arg(reader, "ref.func")?;
            Ok(Instruction::RefFunc(fi))
        }

        _ => Err(InstructionError {
            kind: InstructionErrorKind::InvalidOpCode(InvalidEnumValueError {
                value: opcode,
                enum_name: std::any::type_name::<Instruction>(),
            }),
            instr: None,
            offset,
        }),
    }
}

fn decode_arg<'a, T: FromReader<'a, Error = ReadError>>(
    reader: &mut Reader<'a>,
    instr: &'static str,
) -> Result<T, InstructionError> {
    reader.read().map_err(|e: ReadError| InstructionError {
        offset: e.offset,
        kind: InstructionErrorKind::ExpectedArgument(e),
        instr: Some(instr),
    })
}

#[derive(Debug)]
#[non_exhaustive]
pub struct InstructionError {
    kind: InstructionErrorKind,
    instr: Option<&'static str>,
    offset: usize,
}

impl std::fmt::Display for InstructionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(instr) = self.instr {
            write!(
                f,
                "decoding instruction '{}' at offset {a:#0x} ({a})",
                instr,
                a = self.offset
            )
        } else {
            write!(
                f,
                "decoding instruction at offset {a:#0x} ({a})",
                a = self.offset
            )
        }
    }
}

impl std::error::Error for InstructionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.kind)
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum InstructionErrorKind {
    Read(ReadError),
    InvalidOpCode(InvalidEnumValueError),
    ExpectedArgument(ReadError),

    #[allow(dead_code)]
    ExpectedAdditionalArgument {
        idx: u32,
        of: u32,
        source: ReadError,
    },
}

impl std::fmt::Display for InstructionErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstructionErrorKind::Read(_) => f.write_str("reading instruction opcode"),
            InstructionErrorKind::InvalidOpCode(_) => f.write_str("unknown opcode"),
            InstructionErrorKind::ExpectedArgument(_) => f.write_str("reading argument"),
            InstructionErrorKind::ExpectedAdditionalArgument { idx, of, .. } => {
                write!(f, "reading argument {} of {}", idx + 1, of)
            }
        }
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
        let offset = value.offset;

        InstructionError {
            kind: InstructionErrorKind::Read(value),
            instr: None,
            offset,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binary::reader::Reader;

    #[test]
    fn test_decode_control_flow() {
        // Nop
        let mut reader = Reader::from_bytes(&[0x01], 0);
        assert_eq!(decode_instruction(&mut reader).unwrap(), Instruction::Nop);

        // Else
        let mut reader = Reader::from_bytes(&[0x05], 0);
        assert_eq!(decode_instruction(&mut reader).unwrap(), Instruction::Else);

        // End
        let mut reader = Reader::from_bytes(&[0x0b], 0);
        assert_eq!(decode_instruction(&mut reader).unwrap(), Instruction::End);

        // If with i32 block type
        let mut reader = Reader::from_bytes(&[0x04, 0x7f], 0);
        assert_eq!(
            decode_instruction(&mut reader).unwrap(),
            Instruction::If(ValType::I32)
        );
    }

    #[test]
    fn test_decode_i32_arithmetic() {
        let cases: &[(u8, Instruction)] = &[
            (0x6a, Instruction::I32Add),
            (0x6b, Instruction::I32Sub),
            (0x6c, Instruction::I32Mul),
            (0x6d, Instruction::I32DivS),
            (0x6e, Instruction::I32DivU),
            (0x6f, Instruction::I32RemS),
            (0x70, Instruction::I32RemU),
        ];

        for (opcode, expected) in cases {
            let bytes = [*opcode];
            let mut reader = Reader::from_bytes(&bytes, 0);
            assert_eq!(decode_instruction(&mut reader).unwrap(), *expected);
        }
    }

    #[test]
    fn test_decode_i32_bitwise() {
        let cases: &[(u8, Instruction)] = &[
            (0x71, Instruction::I32And),
            (0x72, Instruction::I32Or),
            (0x73, Instruction::I32Xor),
            (0x74, Instruction::I32Shl),
            (0x75, Instruction::I32ShrS),
            (0x76, Instruction::I32ShrU),
            (0x77, Instruction::I32Rotl),
            (0x78, Instruction::I32Rotr),
        ];

        for (opcode, expected) in cases {
            let bytes = [*opcode];
            let mut reader = Reader::from_bytes(&bytes, 0);
            assert_eq!(decode_instruction(&mut reader).unwrap(), *expected);
        }
    }

    #[test]
    fn test_decode_i32_comparison() {
        let cases: &[(u8, Instruction)] = &[
            (0x45, Instruction::I32Eqz),
            (0x46, Instruction::I32Eq),
            (0x47, Instruction::I32Ne),
            (0x48, Instruction::I32LtS),
            (0x49, Instruction::I32LtU),
            (0x4a, Instruction::I32GtS),
            (0x4b, Instruction::I32GtU),
            (0x4c, Instruction::I32LeS),
            (0x4d, Instruction::I32LeU),
            (0x4e, Instruction::I32GeS),
            (0x4f, Instruction::I32GeU),
        ];

        for (opcode, expected) in cases {
            let bytes = [*opcode];
            let mut reader = Reader::from_bytes(&bytes, 0);
            assert_eq!(decode_instruction(&mut reader).unwrap(), *expected);
        }
    }

    #[test]
    fn test_decode_i32_unary() {
        let cases: &[(u8, Instruction)] = &[
            (0x67, Instruction::I32Clz),
            (0x68, Instruction::I32Ctz),
            (0x69, Instruction::I32Popcnt),
        ];

        for (opcode, expected) in cases {
            let bytes = [*opcode];
            let mut reader = Reader::from_bytes(&bytes, 0);
            assert_eq!(decode_instruction(&mut reader).unwrap(), *expected);
        }
    }

    #[test]
    fn test_decode_i32_sign_extend() {
        let cases: &[(u8, Instruction)] = &[
            (0xc0, Instruction::I32Extend8S),
            (0xc1, Instruction::I32Extend16S),
        ];

        for (opcode, expected) in cases {
            let bytes = [*opcode];
            let mut reader = Reader::from_bytes(&bytes, 0);
            assert_eq!(decode_instruction(&mut reader).unwrap(), *expected);
        }
    }

    #[test]
    fn test_decode_local_ops() {
        // LocalGet
        let mut reader = Reader::from_bytes(&[0x20, 0x00], 0);
        assert_eq!(
            decode_instruction(&mut reader).unwrap(),
            Instruction::LocalGet(0)
        );
        let mut reader = Reader::from_bytes(&[0x20, 0x05], 0);
        assert_eq!(
            decode_instruction(&mut reader).unwrap(),
            Instruction::LocalGet(5)
        );

        // LocalSet
        let mut reader = Reader::from_bytes(&[0x21, 0x01], 0);
        assert_eq!(
            decode_instruction(&mut reader).unwrap(),
            Instruction::LocalSet(1)
        );

        // LocalTee
        let mut reader = Reader::from_bytes(&[0x22, 0x02], 0);
        assert_eq!(
            decode_instruction(&mut reader).unwrap(),
            Instruction::LocalTee(2)
        );
    }

    #[test]
    fn test_decode_call() {
        let mut reader = Reader::from_bytes(&[0x10, 0x00], 0);
        assert_eq!(
            decode_instruction(&mut reader).unwrap(),
            Instruction::Call(0)
        );

        let mut reader = Reader::from_bytes(&[0x10, 0x0a], 0);
        assert_eq!(
            decode_instruction(&mut reader).unwrap(),
            Instruction::Call(10)
        );
    }

    #[test]
    fn test_decode_const() {
        // I32Const
        let mut reader = Reader::from_bytes(&[0x41, 0x00], 0);
        assert_eq!(
            decode_instruction(&mut reader).unwrap(),
            Instruction::I32Const(0)
        );

        let mut reader = Reader::from_bytes(&[0x41, 0x2a], 0);
        assert_eq!(
            decode_instruction(&mut reader).unwrap(),
            Instruction::I32Const(42)
        );

        let mut reader = Reader::from_bytes(&[0x41, 0x7f], 0);
        assert_eq!(
            decode_instruction(&mut reader).unwrap(),
            Instruction::I32Const(-1)
        );

        // I64Const
        let mut reader = Reader::from_bytes(&[0x42, 0x00], 0);
        assert_eq!(
            decode_instruction(&mut reader).unwrap(),
            Instruction::I64Const(0)
        );

        // 100 in signed LEB128 needs two bytes to avoid sign extension
        let mut reader = Reader::from_bytes(&[0x42, 0xe4, 0x00], 0);
        assert_eq!(
            decode_instruction(&mut reader).unwrap(),
            Instruction::I64Const(100)
        );
    }

    #[test]
    fn test_decode_invalid_opcode() {
        let mut reader = Reader::from_bytes(&[0xff], 0);
        assert!(decode_instruction(&mut reader).is_err());
    }

    #[test]
    fn test_decode_truncated_argument() {
        // i32.const without argument
        let mut reader = Reader::from_bytes(&[0x41], 0);
        assert!(decode_instruction(&mut reader).is_err());

        // local.get without index
        let mut reader = Reader::from_bytes(&[0x20], 0);
        assert!(decode_instruction(&mut reader).is_err());
    }
}
