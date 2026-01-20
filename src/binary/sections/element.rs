use std::{error, fmt, result};

use crate::binary::{
    reader::{FromReader, ReadError, ReadErrorKind, Reader, VecReadError},
    sections::{SectionEntry, SectionErrorKind},
    types::{ConstExpression, ConstExpressionReadError, RefType},
};

/// Element Section
#[derive(Debug)]
pub enum ElementSegmentMode {
    Passive,
    Active {
        #[allow(dead_code)]
        table_index: Option<u32>,
        #[allow(dead_code)]
        offset: ConstExpression,
    },
    Declarative,
}

#[derive(Debug)]
pub enum ElementSegmentModeReadError {
    Flag(ReadError),
    TableIndex(ReadError),
    Expression(ConstExpressionReadError),
}

impl error::Error for ElementSegmentModeReadError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Flag(e) => Some(e),
            Self::TableIndex(e) => Some(e),
            Self::Expression(e) => Some(e),
        }
    }
}

impl fmt::Display for ElementSegmentModeReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Flag(_) => write!(f, "reading flag"),
            Self::TableIndex(_) => write!(f, "reading table index"),
            Self::Expression(_) => write!(f, "reading expression"),
        }
    }
}

impl<'a> FromReader<'a> for ElementSegmentMode {
    type Error = ElementSegmentModeReadError;

    fn from_reader(reader: &mut Reader<'a>) -> result::Result<Self, Self::Error> {
        let offset = reader.position() as usize;
        let flag: u32 = reader.read().map_err(Self::Error::Flag)?;

        if (flag & !0b111) != 0 {
            return Err(Self::Error::Flag(ReadError {
                offset,
                kind: ReadErrorKind::UnexpectedValue {
                    value: flag.to_string(),
                    expected: "0 <= flag <= 7 for element segment".to_string(),
                },
            }));
        }

        let mode = if flag & 0b001 != 0 {
            if flag & 0b010 != 0 {
                ElementSegmentMode::Passive
            } else {
                ElementSegmentMode::Declarative
            }
        } else {
            let table_index = if flag & 0b010 != 0 {
                Some(reader.read().map_err(Self::Error::TableIndex)?)
            } else {
                None
            };
            ElementSegmentMode::Active {
                table_index,
                offset: reader.read().map_err(Self::Error::Expression)?,
            }
        };
        Ok(mode)
    }
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

#[derive(Debug)]
#[allow(dead_code)]
pub struct FuncIndex(u32);

impl<'a> FromReader<'a> for FuncIndex {
    type Error = ReadError;

    fn from_reader(reader: &mut Reader<'a>) -> result::Result<Self, Self::Error> {
        reader.read().map(FuncIndex)
    }
}

#[derive(Debug)]
pub enum ElementSegmentItems {
    #[allow(dead_code)]
    Functions(Vec<FuncIndex>),
    #[allow(dead_code)]
    Expressions(RefType, Vec<ConstExpression>),
}

#[derive(Debug)]
pub struct ElementSegment {
    #[allow(dead_code)]
    pub mode: ElementSegmentMode,
    #[allow(dead_code)]
    pub items: ElementSegmentItems,
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
            if flag & 0b010 != 0 {
                ElementSegmentMode::Passive
            } else {
                ElementSegmentMode::Declarative
            }
        } else {
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
            let rt: RefType = reader
                .read()
                .map_err(ElementSectionReadError::ItemsRefType)?;
            let exprs: Vec<ConstExpression> = reader
                .read()
                .map_err(ElementSectionReadError::ItemsExpressions)
                .map_err(SectionErrorKind::ElementSection)?;

            ElementSegmentItems::Expressions(rt, exprs)
        } else {
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
