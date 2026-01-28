use crate::binary::{
    reader::{FromReader, InvalidEnumValueError, ReadError, Reader, VecReadError},
    types::{
        BlockType, BranchTableIdx, BranchTableIdxReadError, FuncIdx, GlobalIdx, LabelIdx, MemArg,
        MemArgReadError, RefType, TableIdx, TypeIdx, ValType,
    },
};

/// Generates the enum for instruction of the form:
/// ```
/// #[derive(Debug, Clone, PartialEq)]
/// pub enum Instruction {
///    OpCode,
///    OpCodeWithArg(u32),
/// }
/// ```
/// And the match arms for the decode_instruction function.
macro_rules! instructions {
    (
        $($name:ident $(($arg:ty))? : $opcode:literal : $instr_name:literal,)*
    ) => {
        /// WebAssembly Instructions
        #[derive(Debug, Clone)]
        pub enum Instruction {
            $(
                $name $(($arg))?,
            )*
        }

        pub fn decode_instruction(reader: &mut Reader) -> Result<Instruction, InstructionError> {
            let offset = reader.position() as usize;
            let opcode = reader.read_u8()?;
            match opcode {
                $(
                    $opcode => instructions!(@decode reader $name $(($arg))? $instr_name),
                )*
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
    };

    // No arg instruction
    (@decode $reader:ident $name:ident $instr_name:literal) => {
        Ok(Instruction::$name)
    };

    // Single arg instruction
    (@decode $reader:ident $name:ident ($arg:ty) $instr_name:literal) => {
        {
            let arg: $arg = decode_arg($reader, $instr_name)?;
            Ok(Instruction::$name(arg))
        }
    };
}

instructions! {
    // Parametric
    Unreachable : 0x00 : "unreachable",
    Nop : 0x01 : "nop",
    Drop : 0x1a : "drop",
    Select : 0x1b : "select",
    SelectT(Vec<ValType>) : 0x1c : "select (typed)",

    // Control
    Block(BlockType) : 0x02 : "block",
    Loop(BlockType) : 0x03 : "loop",
    If(BlockType) : 0x04 : "if",
    Else : 0x05 : "else",

    End : 0x0b : "end",
    Br(LabelIdx) : 0x0c : "br",
    BrIf(LabelIdx) : 0x0d : "br_if",
    BrTable(BranchTableIdx) : 0x0e : "br_table",
    Return : 0x0f : "return",
    Call(u32) : 0x10 : "call",
    CallIndirect((TypeIdx, TableIdx)) : 0x11 : "call_indirect",

    // Variable
    LocalGet(u32) : 0x20 : "local.get",
    LocalSet(u32) : 0x21 : "local.set",
    LocalTee(u32) : 0x22 : "local.tee",
    GlobalGet(GlobalIdx) : 0x23 : "global.get",
    GlobalSet(GlobalIdx) : 0x24 : "global.set",

    // Memory
    I32Load(MemArg) : 0x28 : "i32.load",
    I32Store(MemArg) : 0x36 : "i32.store",

    // Const
    I32Const(i32) : 0x41 : "i32.const",
    I64Const(i64) : 0x42 : "i64.const",

    // Compare
    I32Eqz : 0x45 : "i32.eqz",
    I32Eq : 0x46 : "i32.eq",
    I32Ne : 0x47 : "i32.ne",
    I32LtS : 0x48 : "i32.lt_s",
    I32LtU : 0x49 : "i32.lt_u",
    I32GtS : 0x4a : "i32.gt_s",
    I32GtU : 0x4b : "i32.gt_u",
    I32LeS : 0x4c : "i32.le_s",
    I32LeU : 0x4d : "i32.le_u",
    I32GeS : 0x4e : "i32.ge_s",
    I32GeU : 0x4f : "i32.ge_u",

    // Unary ops
    I32Clz : 0x67 : "i32.clz",
    I32Ctz : 0x68 : "i32.ctz",
    I32Popcnt : 0x69 : "i32.popcnt",

    // Binary ops
    I32Add : 0x6a : "i32.add",
    I32Sub : 0x6b : "i32.sub",
    I32Mul : 0x6c : "i32.mul",
    I32DivS : 0x6d : "i32.div_s",
    I32DivU : 0x6e : "i32.div_u",
    I32RemS : 0x6f : "i32.rem_s",
    I32RemU : 0x70 : "i32.rem_u",
    I32And : 0x71 : "i32.and",
    I32Or : 0x72 : "i32.or",
    I32Xor : 0x73 : "i32.xor",
    I32Shl : 0x74 : "i32.shl",
    I32ShrS : 0x75 : "i32.shr_s",
    I32ShrU : 0x76 : "i32.shr_u",
    I32Rotl : 0x77 : "i32.rotl",
    I32Rotr : 0x78 : "i32.rotr",

    I32Extend8S : 0xc0 : "i32.extend8_s",
    I32Extend16S : 0xc1 : "i32.extend16_s",

    // Reference
    RefNull(RefType) : 0xd0 : "ref.null",
    RefFunc(FuncIdx) : 0xd2 : "ref.func",
}

fn decode_arg<'a, T>(reader: &mut Reader<'a>, instr: &'static str) -> Result<T, InstructionError>
where
    T: FromReader<'a>,
    T::Error: Into<ArgumentReadError>,
{
    let offset = reader.position() as usize;
    reader.read().map_err(|e: T::Error| InstructionError {
        offset,
        kind: InstructionErrorKind::Argument(e.into()),
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
    Argument(ArgumentReadError),
}

impl std::fmt::Display for InstructionErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstructionErrorKind::Read(_) => f.write_str("reading instruction opcode"),
            InstructionErrorKind::InvalidOpCode(_) => f.write_str("unknown opcode"),
            InstructionErrorKind::Argument(_) => f.write_str("reading argument"),
        }
    }
}

impl std::error::Error for InstructionErrorKind {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            InstructionErrorKind::Read(e) => Some(e),
            InstructionErrorKind::InvalidOpCode(e) => Some(e),
            InstructionErrorKind::Argument(e) => Some(e),
        }
    }
}

#[derive(Debug)]
pub enum ArgumentReadError {
    Read(ReadError),
    VecRead(VecReadError<ReadError>),
    BranchTableIdx(BranchTableIdxReadError),
    MemArg(MemArgReadError),
}

impl std::fmt::Display for ArgumentReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read(e) => e.fmt(f),
            Self::VecRead(e) => e.fmt(f),
            Self::BranchTableIdx(e) => e.fmt(f),
            Self::MemArg(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for ArgumentReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read(e) => Some(e),
            Self::VecRead(e) => Some(e),
            Self::BranchTableIdx(e) => Some(e),
            Self::MemArg(e) => Some(e),
        }
    }
}

impl From<ReadError> for ArgumentReadError {
    fn from(value: ReadError) -> Self {
        Self::Read(value)
    }
}

impl From<VecReadError<ReadError>> for ArgumentReadError {
    fn from(value: VecReadError<ReadError>) -> Self {
        Self::VecRead(value)
    }
}

impl From<BranchTableIdxReadError> for ArgumentReadError {
    fn from(value: BranchTableIdxReadError) -> Self {
        Self::BranchTableIdx(value)
    }
}

impl From<MemArgReadError> for ArgumentReadError {
    fn from(value: MemArgReadError) -> Self {
        Self::MemArg(value)
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
    use crate::binary::types::ValType;

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
            Instruction::If(BlockType::Value(ValType::I32))
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
