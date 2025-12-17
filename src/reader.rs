use crate::leb128::{self, DecodeError};

use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    io::{self, Cursor, Read},
};

pub struct Reader<'a> {
    pub cursor: Cursor<&'a [u8]>,
}

impl<'a> Reader<'a> {
    pub fn from_bytes(bytes: &'a [u8], pos: usize) -> Self {
        let mut r = Reader {
            cursor: Cursor::new(&bytes),
        };
        r.cursor.set_position(pos as u64);
        r
    }

    pub fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        self.cursor.read_exact(buf).map_err(|e| ReadError {
            offset: self.cursor.position() as usize,
            kind: ReadErrorKind::Read(e),
        })
    }

    pub fn position(&self) -> u64 {
        self.cursor.position()
    }

    pub fn read_u8(&mut self) -> Result<u8> {
        leb128::read_u8(&mut self.cursor).map_err(|e| ReadError {
            offset: self.cursor.position() as usize,
            kind: ReadErrorKind::Decode(e),
        })
    }

    pub fn read_u32(&mut self) -> Result<u32> {
        leb128::read_leb128_u32(&mut self.cursor).map_err(|e| ReadError {
            offset: self.cursor.position() as usize,
            kind: ReadErrorKind::Decode(e),
        })
    }

    pub fn read_vec<T: FromReader<'a>>(&mut self) -> Result<Vec<T>> {
        let len = self.read_u32()?;
        (0..len)
            .map(|_| T::from_reader(self))
            .collect::<Result<Vec<T>>>()
    }
}

pub trait FromReader<'a>: Sized {
    fn from_reader(reader: &mut Reader<'a>) -> Result<Self>;
}

pub type Result<T> = std::result::Result<T, ReadError>;

#[derive(Debug)]
#[non_exhaustive]
pub struct ReadError {
    pub offset: usize,
    pub kind: ReadErrorKind,
}

impl Display for ReadError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "reading byte at offset {}", self.offset)
    }
}

impl Error for ReadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            ReadErrorKind::Decode(e) => Some(e),
            ReadErrorKind::Read(e) => Some(e),
            ReadErrorKind::Parse(e) => Some(e),
        }
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum ReadErrorKind {
    #[non_exhaustive]
    Parse(ParseError),

    #[non_exhaustive]
    Decode(DecodeError),

    #[non_exhaustive]
    Read(io::Error),
}

#[derive(Debug)]
#[non_exhaustive]
pub struct ParseError {
    pub kind: ParseErrorKind,
}

impl Display for ParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "parse error")
    }
}

impl Error for ParseError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.kind)
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum ParseErrorKind {
    #[non_exhaustive]
    BadMagic,

    #[non_exhaustive]
    BadVersion,

    #[non_exhaustive]
    UnexpectedValue {
        value: String,
        expected: &'static str,
    },

    #[non_exhaustive]
    InvalidEnumValue(InvalidEnumValueError),
}

impl Display for ParseErrorKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadMagic => f.write_str("bad magic number"),
            Self::BadVersion => f.write_str("bad version number"),
            Self::UnexpectedValue { value, expected } => {
                write!(f, "unexpected value: expected {expected}, got {value}")
            }
            Self::InvalidEnumValue(_) => f.write_str("invalid enum value"),
        }
    }
}

impl Error for ParseErrorKind {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidEnumValue(e) => Some(e),
            _ => None,
        }
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub struct InvalidEnumValueError {
    pub value: u8,
    pub enum_name: &'static str,
}

impl Display for InvalidEnumValueError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} is not a valid value for {}",
            self.value.to_string(),
            self.enum_name
        )
    }
}

impl Error for InvalidEnumValueError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        None
    }
}
