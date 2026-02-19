use crate::binary::{
    reader::{FromReader, InvalidEnumValueError, ReadError, Reader, VecReadError},
    types::{
        BlockType, BranchTableIdx, BranchTableIdxReadError, DataIdx, ElemIdx, FuncIdx, GlobalIdx,
        LabelIdx, MemArg, MemArgReadError, MemIndex, RefType, TableIdx, TypeIdx, ValType,
    },
};

/// Generates the enum for instruction of the form:
/// ```
/// #[derive(Debug, Clone)]
/// pub enum Instruction {
///    OpCode,
///    OpCodeWithArg(u32),
/// }
/// ```
/// And the match arms for the decode_instruction function.
macro_rules! instructions {
    (
        $($name:ident $(($arg:ty))? : $opcode:literal : $instr_name:literal,)*
        $(
            @prefix $prefix:literal {
                $($pname:ident $(($parg:ty))? : $popcode:literal : $pinstr_name:literal,)*
            }
        )*
    ) => {
        /// WebAssembly Instructions
        #[cfg_attr(test, derive(PartialEq))]
        #[derive(Debug, Clone)]
        pub enum Instruction {
            $(
                $name $(($arg))?,
            )*
            $($(
                $pname $(($parg))?,
            )*)*
        }

        pub fn decode_instruction(reader: &mut Reader) -> Result<Instruction, InstructionError> {
            let offset = reader.position() as usize;
            let opcode = reader.read_u8()?;
            match opcode {
                $(
                    $opcode => instructions!(@decode reader $name $(($arg))? $instr_name),
                )*
                $(
                    $prefix => {
                        let second = reader.read_u32()?;
                        match second {
                            $(
                            $popcode => instructions!(@decode reader $pname $(($parg))? $pinstr_name),
                            )*
                            _ => Err(InstructionError {
                                kind: InstructionErrorKind::InvalidOpCode(InvalidEnumValueError {
                                    value: second as u8,
                                    enum_name: std::any::type_name::<Instruction>(),
                                }),
                                instr: None,
                                offset,
                            }),

                        }

                    }
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
    // Load
    I32Load(MemArg) : 0x28 : "i32.load",
    I64Load(MemArg) : 0x29 : "i64.load",
    F32Load(MemArg) : 0x2a : "f32.load",
    F64Load(MemArg) : 0x2b : "f64.load",
    I32Load8S(MemArg) : 0x2c :  "i32.load8_s",
    I32Load8U(MemArg) : 0x2d :  "i32.load8_u",
    I32Load16S(MemArg) : 0x2e :  "i32.load16_s",
    I32Load16U(MemArg) : 0x2f :  "i32.load16_u",
    I64Load8S(MemArg) : 0x30 :  "i64.load8_s",
    I64Load8U(MemArg) : 0x31 :  "i64.load8_u",
    I64Load16S(MemArg) : 0x32 :  "i64.load16_s",
    I64Load16U(MemArg) : 0x33 :  "i64.load16_u",
    I64Load32S(MemArg) : 0x34 :  "i64.load32_s",
    I64Load32U(MemArg) : 0x35 :  "i64.load32_u",

    // Store
    I32Store(MemArg) : 0x36 : "i32.store",
    I64Store(MemArg) : 0x37 : "i64.store",
    F32Store(MemArg) : 0x38 : "f32.store",
    F64Store(MemArg) : 0x39 : "f64.store",
    I32Store8(MemArg) : 0x3a : "i32.store8",
    I32Store16(MemArg) : 0x3b : "i32.store16",
    I64Store8(MemArg) : 0x3c : "i64.store8",
    I64Store16(MemArg) : 0x3d : "i64.store16",
    I64Store32(MemArg) : 0x3e : "i64.store32",

    MemorySize(MemIndex) :0x3f : "memory.size",
    MemoryGrow(MemIndex) :0x40 : "memory.grow",

    // Const
    I32Const(i32) : 0x41 : "i32.const",
    I64Const(i64) : 0x42 : "i64.const",
    F32Const(f32) : 0x43 : "f32.const",
    F64Const(f64) : 0x44 : "f64.const",

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

    I64Eqz : 0x50 : "i64.eqz",
    I64Eq : 0x51 : "i64.eq",
    I64Ne : 0x52 : "i64.ne",
    I64LtS : 0x53 : "i64.lt_s",
    I64LtU : 0x54 : "i64.lt_u",
    I64GtS : 0x55 : "i64.gt_s",
    I64GtU : 0x56 : "i64.gt_u",
    I64LeS : 0x57 : "i64.le_s",
    I64LeU : 0x58 : "i64.le_u",
    I64GeS : 0x59 : "i64.ge_s",
    I64GeU : 0x5a : "i64.ge_u",

    F32Eq : 0x5b : "f32.eq",
    F32Ne : 0x5c : "f32.ne",
    F32Lt : 0x5d : "f32.lt",
    F32Gt : 0x5e : "f32.gt",
    F32Le : 0x5f : "f32.le",
    F32Ge : 0x60 : "f32.ge",

    F64Eq : 0x61 : "f64.eq",
    F64Ne : 0x62 : "f64.ne",
    F64Lt : 0x63 : "f64.lt",
    F64Gt : 0x64 : "f64.gt",
    F64Le : 0x65 : "f64.le",
    F64Ge : 0x66 : "f64.ge",

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

    I64Clz : 0x79 : "i64.clz",
    I64Ctz : 0x7a : "i64.ctz",
    I64Popcnt : 0x7b : "i64.popcnt",
    I64Add : 0x7c : "i64.add",
    I64Sub : 0x7d : "i64.sub",
    I64Mul : 0x7e : "i64.mul",
    I64DivS : 0x7f : "i64.div_s",
    I64DivU : 0x80 : "i64.div_u",
    I64RemS : 0x81 : "i64.rem_s",
    I64RemU : 0x82 : "i64.rem_u",
    I64And : 0x83 : "i64.and",
    I64Or : 0x84 : "i64.or",
    I64Xor : 0x85 : "i64.xor",
    I64Shl : 0x86 : "i64.shl",
    I64ShrS : 0x87 : "i64.shr_s",
    I64ShrU : 0x88 : "i64.shr_u",
    I64Rotl : 0x89 : "i64.rotl",
    I64Rotr : 0x8a : "i64.rotr",

    F32Abs : 0x8b : "f32.abs",
    F32Neg : 0x8c : "f32.neg",
    F32Ceil : 0x8d : "f32.ceil",
    F32Floor : 0x8e : "f32.floor",
    F32Trunc : 0x8f : "f32.trunc",
    F32Nearest : 0x90 : "f32.nearest",
    F32Sqrt : 0x91 : "f32.sqrt",
    F32Add : 0x92 : "f32.add",
    F32Sub : 0x93 : "f32.sub",
    F32Mul : 0x94 : "f32.mul",
    F32Div : 0x95 : "f32.div",
    F32Min : 0x96 : "f32.min",
    F32Max : 0x97 : "f32.max",
    F32Copysign : 0x98 : "f32.copysign",

    F64Abs : 0x99 : "f64.abs",
    F64Neg : 0x9a : "f64.neg",
    F64Ceil : 0x9b : "f64.ceil",
    F64Floor : 0x9c : "f64.floor",
    F64Trunc : 0x9d : "f64.trunc",
    F64Nearest : 0x9e : "f64.nearest",
    F64Sqrt : 0x9f : "f64.sqrt",
    F64Add : 0xa0 : "f64.add",
    F64Sub : 0xa1 : "f64.sub",
    F64Mul : 0xa2 : "f64.mul",
    F64Div : 0xa3 : "f64.div",
    F64Min : 0xa4 : "f64.min",
    F64Max : 0xa5 : "f64.max",
    F64Copysign : 0xa6 : "f64.copysign",

    I32WrapI64 : 0xa7 : "i32.wrap_i64",

    I32TruncF32S : 0xa8 : "i32.trunc_f32_s",
    I32TruncF32U : 0xa9 : "i32.trunc_f32_u",
    I32TruncF64S : 0xaa : "i32.trunc_f64_s",
    I32TruncF64U : 0xab : "i32.trunc_f64_u",
    I64ExtendI32S : 0xac : "i64.extend_i32_s",
    I64ExtendI32U : 0xad : "i64.extend_i32_u",
    I64TruncF32S : 0xae : "i64.trunc_f32_s",
    I64TruncF32U : 0xaf : "i64.trunc_f32_u",
    I64TruncF64S : 0xb0 : "i64.trunc_f64_s",
    I64TruncF64U : 0xb1 : "i64.trunc_f64_u",
    F32ConvertI32S : 0xb2 : "f32.convert_i32_s",
    F32ConvertI32U : 0xb3 : "f32.convert_i32_u",
    F32ConvertI64S : 0xb4 : "f32.convert_i64_s",
    F32ConvertI64U : 0xb5 : "f32.convert_i64_u",
    F32DemoteF64 : 0xb6 : "f32.demote_f64",
    F64ConvertI32S : 0xb7 : "f64.convert_i32_s",
    F64ConvertI32U : 0xb8 : "f64.convert_i32_u",
    F64ConvertI64S : 0xb9 : "f64.convert_i64_s",
    F64ConvertI64U : 0xba : "f64.convert_i64_u",
    F64PromoteF32 : 0xbb : "f64.promot_f32",

    I32ReinterpretF32 : 0xbc : "i32.reinterpret_f32",
    I64ReinterpretF64 : 0xbd : "i64.reinterpret_f64",
    F32ReinterpretI32 : 0xbe : "f32.reinterpret_i32",
    F64ReinterpretI64 : 0xbf : "f64.reinterpret_i64",

    I32Extend8S : 0xc0 : "i32.extend8_s",
    I32Extend16S : 0xc1 : "i32.extend16_s",
    I64Extend8S: 0xc2 : "i64.extend8_s",
    I64Extend16S: 0xc3 : "i64.extend16_s",
    I64Extend32S: 0xc4 : "i64.extend32_s",

    // Reference
    RefNull(RefType) : 0xd0 : "ref.null",
    RefFunc(FuncIdx) : 0xd2 : "ref.func",

    @prefix 0xfc {
        I32TruncSatF32S : 0x00 : "i32_trunc_sat_f32_s",
        I32TruncSatF32U : 0x01 : "i32_trunc_sat_f32_u",
        I64TruncSatF64S : 0x06 : "i64_trunc_sat_f64_s",
        I64TruncSatF64U : 0x07 : "i64_trunc_sat_f64_u",
        MemoryInit((MemIndex, DataIdx)) : 0x08 : "memory.init",
        DataDrop(DataIdx) : 0x09 : "data.drop",
        TableInit((TableIdx, ElemIdx)) : 0x0c : "table.init",
        ElemDrop(ElemIdx) : 0x0d : "elem.drop",
    }
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
    fn test_decode_prefix_single_byte() {
        // i32_trunc_sat_f32_s: 0xfc 0x00
        let mut reader = Reader::from_bytes(&[0xfc, 0x00], 0);
        assert_eq!(
            decode_instruction(&mut reader).unwrap(),
            Instruction::I32TruncSatF32S
        );

        // i64_trunc_sat_f64_u: 0xfc 0x07
        let mut reader = Reader::from_bytes(&[0xfc, 0x07], 0);
        assert_eq!(
            decode_instruction(&mut reader).unwrap(),
            Instruction::I64TruncSatF64U
        );
    }

    #[test]
    fn test_decode_prefix_multi_byte_leb128() {
        // Sub-opcode 7 encoded as 2-byte LEB128: 0x87 0x00
        let mut reader = Reader::from_bytes(&[0xfc, 0x87, 0x00], 0);
        assert_eq!(
            decode_instruction(&mut reader).unwrap(),
            Instruction::I64TruncSatF64U
        );

        // Sub-opcode 0 encoded as 2-byte LEB128: 0x80 0x00
        let mut reader = Reader::from_bytes(&[0xfc, 0x80, 0x00], 0);
        assert_eq!(
            decode_instruction(&mut reader).unwrap(),
            Instruction::I32TruncSatF32S
        );

        // Sub-opcode 7 encoded as 5-byte LEB128 (max for u32): 0x87 0x80 0x80 0x80 0x00
        let mut reader = Reader::from_bytes(&[0xfc, 0x87, 0x80, 0x80, 0x80, 0x00], 0);
        assert_eq!(
            decode_instruction(&mut reader).unwrap(),
            Instruction::I64TruncSatF64U
        );
    }

    #[test]
    fn test_decode_prefix_too_long_leb128() {
        // Sub-opcode encoded as 6 bytes (exceeds u32 LEB128 max of 5)
        let mut reader = Reader::from_bytes(&[0xfc, 0x87, 0x80, 0x80, 0x80, 0x80, 0x00], 0);
        assert!(decode_instruction(&mut reader).is_err());
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
