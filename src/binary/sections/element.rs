use std::{error, fmt, result};

use crate::binary::{
    reader::{FromReader, ReadError, ReadErrorKind, Reader, VecReadError},
    sections::{SectionEntry, SectionErrorKind},
    types::{ConstExpression, ConstExpressionReadError, FuncIndex, RefType},
};

/// Element Section
#[derive(Debug)]
pub enum ElementSegmentMode {
    Passive,
    Active {
        table_index: Option<u32>,
        offset: ConstExpression,
    },
    Declarative,
}

#[derive(Debug)]
pub struct ElementKindMarker();
impl<'a> FromReader<'a> for ElementKindMarker {
    type Error = ReadError;

    fn from_reader(reader: &mut Reader<'a>) -> result::Result<Self, Self::Error> {
        let offset = reader.position() as usize;
        let v = reader.read_u8()?;

        if v == 0x00 {
            Ok(ElementKindMarker())
        } else {
            Err(ReadError {
                offset,
                kind: ReadErrorKind::UnexpectedValue {
                    value: v.to_string(),
                    expected: 0x60.to_string(),
                },
            })
        }
    }
}

impl<'a> FromReader<'a> for FuncIndex {
    type Error = ReadError;

    fn from_reader(reader: &mut Reader<'a>) -> result::Result<Self, Self::Error> {
        reader.read().map(FuncIndex)
    }
}

#[derive(Debug)]
pub enum ElementSegmentItems {
    Functions(Vec<FuncIndex>),
    Expressions(RefType, Vec<ConstExpression>),
}

#[derive(Debug)]
pub struct ElementSegment {
    pub mode: ElementSegmentMode,
    pub items: ElementSegmentItems,
}

impl ElementSegment {
    pub fn reftype(&self) -> RefType {
        match &self.items {
            ElementSegmentItems::Functions(_) => RefType::Func,
            ElementSegmentItems::Expressions(rt, ..) => *rt,
        }
    }
}

impl From<ElementSectionReadError> for SectionErrorKind {
    fn from(value: ElementSectionReadError) -> Self {
        Self::ElementSection(value)
    }
}

pub type ElementSection = Vec<ElementSegment>;
impl SectionEntry for ElementSegment {
    fn decode(reader: &mut Reader) -> result::Result<Self, SectionErrorKind> {
        let offset = reader.position() as usize;
        let flag: u32 = reader.read().map_err(ElementSectionReadError::ModeFlag)?;

        if (flag & !0b111) != 0 {
            return Err(ElementSectionReadError::ModeFlag(ReadError {
                offset,
                kind: ReadErrorKind::UnexpectedValue {
                    value: flag.to_string(),
                    expected: "0 <= flag <= 7 for element segment".to_string(),
                },
            }))?;
        }

        let mode = if flag & 0b001 != 0 {
            // Bit 0 set, bit 1 distinguishes between passive (0)
            // or declarative (1)
            if flag & 0b010 != 0 {
                ElementSegmentMode::Declarative
            } else {
                ElementSegmentMode::Passive
            }
        } else {
            // Bit 0 not set: active segment. bit 1 is set for an
            // explicit table index
            let table_index = if flag & 0b010 != 0 {
                Some(
                    reader
                        .read()
                        .map_err(ElementSectionReadError::ModeTableIndex)?,
                )
            } else {
                None
            };
            ElementSegmentMode::Active {
                table_index,
                offset: reader
                    .read()
                    .map_err(ElementSectionReadError::ModeOffsetExpression)?,
            }
        };

        let items = if flag & 0b100 != 0 {
            // Bit 2 is set, use element type and expressions.
            // For flag == 4, rt is implied to be ref func.
            let rt = if flag != 4 {
                reader
                    .read()
                    .map_err(ElementSectionReadError::ItemsRefType)?
            } else {
                RefType::Func
            };
            let exprs: Vec<ConstExpression> = reader
                .read()
                .map_err(ElementSectionReadError::ItemsExpressions)
                .map_err(SectionErrorKind::ElementSection)?;

            ElementSegmentItems::Expressions(rt, exprs)
        } else {
            // Bit 2 is not set, use element kind and indices.
            // For flag 0, elemkind is implied
            if flag != 0 {
                // Flag 0 doesn't have elemkind marker
                let _: ElementKindMarker =
                    reader.read().map_err(ElementSectionReadError::ModeKind)?;
            }
            let functions: Vec<FuncIndex> = reader
                .read()
                .map_err(ElementSectionReadError::ItemsFunctions)
                .map_err(SectionErrorKind::ElementSection)?;

            ElementSegmentItems::Functions(functions)
        };
        Ok(Self { mode, items })
    }
}

/// Errors
#[derive(Debug)]
#[non_exhaustive]
pub enum ElementSectionReadError {
    ModeFlag(ReadError),
    ModeTableIndex(ReadError),
    ModeType(ReadError),
    ModeOffsetExpression(ConstExpressionReadError),
    ModeKind(ReadError),
    ModeIndex(ReadError),
    ItemsFunctions(VecReadError<ReadError>),
    ItemsRefType(ReadError),
    ItemsExpressions(VecReadError<ConstExpressionReadError>),
}

impl error::Error for ElementSectionReadError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::ModeFlag(e) => Some(e),
            Self::ModeTableIndex(e) => Some(e),
            Self::ModeType(e) => Some(e),
            Self::ModeOffsetExpression(e) => Some(e),
            Self::ModeKind(e) => Some(e),
            Self::ModeIndex(e) => Some(e),
            Self::ItemsFunctions(e) => Some(e),
            Self::ItemsRefType(e) => Some(e),
            Self::ItemsExpressions(e) => Some(e),
        }
    }
}

impl fmt::Display for ElementSectionReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ModeFlag(_) => write!(f, "reading mode flag"),
            Self::ModeTableIndex(_) => write!(f, "reading mode table index"),
            Self::ModeType(_) => write!(f, "reading mode type"),
            Self::ModeOffsetExpression(_) => write!(f, "reading mode expression"),
            Self::ModeKind(_) => write!(f, "reading mode element kind"),
            Self::ModeIndex(_) => write!(f, "reading mode index"),
            Self::ItemsFunctions(_) => write!(f, "reading segment functions"),
            Self::ItemsRefType(_) => write!(f, "reading mode reference type"),
            Self::ItemsExpressions(_) => write!(f, "reading expressoins"),
        }
    }
}

#[cfg(test)]
mod tests {
    //! Byte-level decode tests for `ElementSegment`.
    //!
    //! Byte sequences are derived from WebAssembly core spec §5.5.12
    //! (Element Section), not from the current decoder. The eight branches
    //! of the spec grammar are:
    //!
    //!   0:u32 e₀:expr y*:list(funcidx)                        -> active table 0, funcs
    //!   1:u32 rt:elemkind y*:list(funcidx)                    -> passive, funcs
    //!   2:u32 x:tableidx e:expr rt:elemkind y*:list(funcidx)  -> active explicit table, funcs
    //!   3:u32 rt:elemkind y*:list(funcidx)                    -> declarative, funcs
    //!   4:u32 e₀:expr e*:list(expr)                           -> active table 0, exprs (implicit (ref null func))
    //!   5:u32 rt:reftype e*:list(expr)                        -> passive, exprs
    //!   6:u32 x:tableidx e₀:expr rt:reftype e*:list(expr)     -> active explicit table, exprs
    //!   7:u32 rt:reftype e*:list(expr)                        -> declarative, exprs
    //!
    //! `elemkind ::= 0x00`, `reftype` is one byte (`0x70` funcref, `0x6f` externref),
    //! `list(T)` is a u32 LEB128 length prefix followed by elements, and
    //! `i32.const N; end` is `0x41 <signed-LEB128(N)> 0x0b`.

    use super::*;
    use crate::instructions::Instruction;

    fn assert_offset_zero(expr: &ConstExpression) {
        assert!(
            matches!(
                expr.0.as_slice(),
                [Instruction::I32Const(0), Instruction::End]
            ),
            "unexpected offset expression: {:?}",
            expr.0
        );
    }

    #[test]
    fn test_decode_flag_0_active_table0_funcs() -> anyhow::Result<()> {
        let bytes = [
            b"\x00" as &[u8], // flag 0
            b"\x41\x00\x0b",  // offset: i32.const 0; end
            b"\x02\x00\x01",  // funcidx list: len=2, [0, 1]
        ]
        .concat();

        let mut r = Reader::from_bytes(&bytes, 0);
        let seg = ElementSegment::decode(&mut r).map_err(|e| anyhow::anyhow!("{e}"))?;

        let ElementSegmentMode::Active {
            table_index,
            offset,
        } = &seg.mode
        else {
            panic!("expected Active, got {:?}", seg.mode);
        };
        assert_eq!(*table_index, None);
        assert_offset_zero(offset);

        let ElementSegmentItems::Functions(funcs) = &seg.items else {
            panic!("expected Functions, got {:?}", seg.items);
        };
        assert_eq!(funcs.len(), 2);
        assert_eq!(funcs[0].0, 0);
        assert_eq!(funcs[1].0, 1);

        Ok(())
    }

    #[test]
    fn test_decode_flag_1_passive_funcs() -> anyhow::Result<()> {
        let bytes = [
            b"\x01" as &[u8], // flag 1
            b"\x00",          // elemkind = 0x00 (ref func)
            b"\x01\x07",      // funcidx list: len=1, [7]
        ]
        .concat();

        let mut r = Reader::from_bytes(&bytes, 0);
        let seg = ElementSegment::decode(&mut r).map_err(|e| anyhow::anyhow!("{e}"))?;

        assert!(matches!(seg.mode, ElementSegmentMode::Passive));

        let ElementSegmentItems::Functions(funcs) = &seg.items else {
            panic!("expected Functions, got {:?}", seg.items);
        };
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].0, 7);

        Ok(())
    }

    #[test]
    fn test_decode_flag_2_active_explicit_table_funcs() -> anyhow::Result<()> {
        let bytes = [
            b"\x02" as &[u8], // flag 2
            b"\x01",          // tableidx = 1
            b"\x41\x00\x0b",  // offset: i32.const 0; end
            b"\x00",          // elemkind = 0x00
            b"\x01\x05",      // funcidx list: len=1, [5]
        ]
        .concat();

        let mut r = Reader::from_bytes(&bytes, 0);
        let seg = ElementSegment::decode(&mut r).map_err(|e| anyhow::anyhow!("{e}"))?;

        let ElementSegmentMode::Active {
            table_index,
            offset,
        } = &seg.mode
        else {
            panic!("expected Active, got {:?}", seg.mode);
        };
        assert_eq!(*table_index, Some(1));
        assert_offset_zero(offset);

        let ElementSegmentItems::Functions(funcs) = &seg.items else {
            panic!("expected Functions, got {:?}", seg.items);
        };
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].0, 5);

        Ok(())
    }

    #[test]
    fn test_decode_flag_3_declarative_funcs() -> anyhow::Result<()> {
        let bytes = [
            b"\x03" as &[u8], // flag 3
            b"\x00",          // elemkind = 0x00
            b"\x02\x02\x03",  // funcidx list: len=2, [2, 3]
        ]
        .concat();

        let mut r = Reader::from_bytes(&bytes, 0);
        let seg = ElementSegment::decode(&mut r).map_err(|e| anyhow::anyhow!("{e}"))?;

        assert!(matches!(seg.mode, ElementSegmentMode::Declarative));

        let ElementSegmentItems::Functions(funcs) = &seg.items else {
            panic!("expected Functions, got {:?}", seg.items);
        };
        assert_eq!(funcs.len(), 2);
        assert_eq!(funcs[0].0, 2);
        assert_eq!(funcs[1].0, 3);

        Ok(())
    }

    // Spec: `4:u32 e₀:expr e*:list(expr)` — no rt byte; type is the implicit
    // `(ref null func)`, which the decoder must default to `RefType::Func`.
    #[test]
    fn test_decode_flag_4_active_table0_exprs() -> anyhow::Result<()> {
        let bytes = [
            b"\x04" as &[u8],    // flag 4
            b"\x41\x00\x0b",     // offset: i32.const 0; end
            b"\x01\x41\x2a\x0b", // expr list: len=1, [i32.const 42; end]
        ]
        .concat();

        let mut r = Reader::from_bytes(&bytes, 0);
        let seg = ElementSegment::decode(&mut r).map_err(|e| anyhow::anyhow!("{e}"))?;

        let ElementSegmentMode::Active {
            table_index,
            offset,
        } = &seg.mode
        else {
            panic!("expected Active, got {:?}", seg.mode);
        };
        assert_eq!(*table_index, None);
        assert_offset_zero(offset);

        let ElementSegmentItems::Expressions(rt, exprs) = &seg.items else {
            panic!("expected Expressions, got {:?}", seg.items);
        };
        assert_eq!(*rt, RefType::Func);
        assert_eq!(exprs.len(), 1);
        assert!(matches!(
            exprs[0].0.as_slice(),
            [Instruction::I32Const(42), Instruction::End]
        ));

        Ok(())
    }

    #[test]
    fn test_decode_flag_5_passive_exprs() -> anyhow::Result<()> {
        let bytes = [
            b"\x05" as &[u8],    // flag 5
            b"\x70",             // reftype = funcref
            b"\x01\x41\x09\x0b", // expr list: len=1, [i32.const 9; end]
        ]
        .concat();

        let mut r = Reader::from_bytes(&bytes, 0);
        let seg = ElementSegment::decode(&mut r).map_err(|e| anyhow::anyhow!("{e}"))?;

        assert!(matches!(seg.mode, ElementSegmentMode::Passive));

        let ElementSegmentItems::Expressions(rt, exprs) = &seg.items else {
            panic!("expected Expressions, got {:?}", seg.items);
        };
        assert_eq!(*rt, RefType::Func);
        assert_eq!(exprs.len(), 1);
        assert!(matches!(
            exprs[0].0.as_slice(),
            [Instruction::I32Const(9), Instruction::End]
        ));

        Ok(())
    }

    #[test]
    fn test_decode_flag_6_active_explicit_table_exprs() -> anyhow::Result<()> {
        let bytes = [
            b"\x06" as &[u8], // flag 6
            b"\x01",          // tableidx = 1
            b"\x41\x00\x0b",  // offset: i32.const 0; end
            b"\x6f",          // reftype = externref
            b"\x00",          // expr list: len=0
        ]
        .concat();

        let mut r = Reader::from_bytes(&bytes, 0);
        let seg = ElementSegment::decode(&mut r).map_err(|e| anyhow::anyhow!("{e}"))?;

        let ElementSegmentMode::Active {
            table_index,
            offset,
        } = &seg.mode
        else {
            panic!("expected Active, got {:?}", seg.mode);
        };
        assert_eq!(*table_index, Some(1));
        assert_offset_zero(offset);

        let ElementSegmentItems::Expressions(rt, exprs) = &seg.items else {
            panic!("expected Expressions, got {:?}", seg.items);
        };
        assert_eq!(*rt, RefType::Extern);
        assert_eq!(exprs.len(), 0);

        Ok(())
    }

    #[test]
    fn test_decode_flag_7_declarative_exprs() -> anyhow::Result<()> {
        let bytes = [
            b"\x07" as &[u8],    // flag 7
            b"\x6f",             // reftype = externref
            b"\x01\x41\x05\x0b", // expr list: len=1, [i32.const 5; end]
        ]
        .concat();

        let mut r = Reader::from_bytes(&bytes, 0);
        let seg = ElementSegment::decode(&mut r).map_err(|e| anyhow::anyhow!("{e}"))?;

        assert!(matches!(seg.mode, ElementSegmentMode::Declarative));

        let ElementSegmentItems::Expressions(rt, exprs) = &seg.items else {
            panic!("expected Expressions, got {:?}", seg.items);
        };
        assert_eq!(*rt, RefType::Extern);
        assert_eq!(exprs.len(), 1);
        assert!(matches!(
            exprs[0].0.as_slice(),
            [Instruction::I32Const(5), Instruction::End]
        ));

        Ok(())
    }

    // Spec defines flags 0–7 only; bit 3 must be 0.
    #[test]
    fn test_decode_invalid_flag() -> anyhow::Result<()> {
        let bytes = b"\x08";
        let mut r = Reader::from_bytes(bytes, 0);
        let result = ElementSegment::decode(&mut r);
        assert!(
            result.is_err(),
            "expected error for flag 8, got: {:?}",
            result
        );
        Ok(())
    }
}
