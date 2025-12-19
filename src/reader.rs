use crate::leb128::{self, DecodeError};

use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    io::{self, BufRead, Cursor, Read},
    string::FromUtf8Error,
};

use crate::types::{FuncType, ValType};

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
        let offset = self.cursor.position() as usize;
        self.cursor.read_exact(buf).map_err(|e| ReadError {
            offset,
            kind: ReadErrorKind::Read(e),
        })
    }

    pub fn position(&self) -> u64 {
        self.cursor.position()
    }

    pub fn has_data_left(&mut self) -> Result<bool> {
        let offset = self.cursor.position() as usize;
        self.cursor
            .fill_buf()
            .map(|b| !b.is_empty())
            .map_err(|e| ReadError {
                offset,
                kind: ReadErrorKind::Read(e),
            })
    }

    pub fn read_u8(&mut self) -> Result<u8> {
        let offset = self.cursor.position() as usize;
        leb128::read_u8(&mut self.cursor).map_err(|e| ReadError {
            offset,
            kind: ReadErrorKind::Decode(e),
        })
    }

    pub fn read_u32(&mut self) -> Result<u32> {
        let offset = self.cursor.position() as usize;
        leb128::read_leb128_u32(&mut self.cursor).map_err(|e| ReadError {
            offset,
            kind: ReadErrorKind::Decode(e),
        })
    }

    pub fn read_i32(&mut self) -> Result<i32> {
        let offset = self.cursor.position() as usize;
        leb128::read_leb128_i32(&mut self.cursor).map_err(|e| ReadError {
            offset,
            kind: ReadErrorKind::Decode(e),
        })
    }

    pub fn read<T: FromReader<'a>>(&mut self) -> Result<T> {
        T::from_reader(self)
    }

    pub fn expect<T>(&mut self, expected: T) -> Result<T>
    where
        T: FromReader<'a> + PartialEq + fmt::Debug,
    {
        let offset = self.position() as usize;
        let v = self.read()?;

        if v == expected {
            Ok(v)
        } else {
            Err(ReadError {
                offset,
                kind: ReadErrorKind::UnexpectedValue {
                    value: format!("{:?}", v),
                    expected: format!("{:?}", expected),
                },
            })
        }
    }

    pub fn read_vec<T, F>(&mut self, mut f: F) -> Result<Vec<T>>
    where
        F: FnMut(&mut Self) -> Result<T>,
    {
        let len = self.read_u32()?;
        (0..len).map(|_| f(self)).collect()
    }

    pub fn read_name(&mut self) -> Result<String> {
        let offset = self.cursor.position() as usize;
        let bytes: Vec<u8> = self.read()?;

        String::from_utf8(bytes).map_err(|e| ReadError {
            offset,
            kind: ReadErrorKind::FromUtf8(e),
        })
    }
}

pub type Result<T> = std::result::Result<T, ReadError>;

/// FromReader Trait
pub trait FromReader<'a>: Sized {
    fn from_reader(reader: &mut Reader<'a>) -> Result<Self>;
}

impl<'a, T: FromReader<'a>> FromReader<'a> for Vec<T> {
    fn from_reader(reader: &mut Reader<'a>) -> Result<Self> {
        let len = reader.read_u32()?;
        (0..len).map(|_| T::from_reader(reader)).collect()
    }
}

impl<'a> FromReader<'a> for FuncType {
    fn from_reader(reader: &mut Reader<'a>) -> Result<FuncType> {
        let _: u8 = reader.expect(0x60)?; // TODO Enum or const

        Ok(FuncType {
            params: reader.read()?,
            results: reader.read()?,
        })
    }
}

impl<'a> FromReader<'a> for ValType {
    fn from_reader(reader: &mut Reader<'a>) -> Result<Self> {
        let pos = reader.position() as usize;
        reader
            .read_u8()?
            .try_into()
            .map_err(|e| ReadError::at_offset(e, pos))
    }
}

impl<'a> FromReader<'a> for u8 {
    fn from_reader(reader: &mut Reader<'a>) -> Result<Self> {
        reader.read_u8()
    }
}

impl<'a> FromReader<'a> for u32 {
    fn from_reader(reader: &mut Reader<'a>) -> Result<Self> {
        reader.read_u32()
    }
}

impl<'a> FromReader<'a> for i32 {
    fn from_reader(reader: &mut Reader<'a>) -> Result<Self> {
        reader.read_i32()
    }
}

/// Errors
#[derive(Debug)]
#[non_exhaustive]
pub struct ReadError {
    pub offset: usize,
    pub kind: ReadErrorKind,
}

impl Display for ReadError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ReadErrorKind::Decode(_) => {
                write!(f, "decoding byte at offset {}", self.offset)
            }
            ReadErrorKind::Read(_) => {
                write!(f, "reading byte at offset {}", self.offset)
            }
            ReadErrorKind::InvalidEnumValue(_) => {
                write!(f, "converting byte at offset {} to enum value", self.offset)
            }
            ReadErrorKind::UnexpectedValue { value, expected } => {
                write!(
                    f,
                    "byte at offset {}, expected {expected}, got {value} instead",
                    self.offset
                )
            }
            ReadErrorKind::FromUtf8(_) => {
                write!(
                    f,
                    "converting byte sequence starting at offset {} to utf8 string",
                    self.offset
                )
            }
            ReadErrorKind::BadMagic => f.write_str("bad magic"),
            ReadErrorKind::BadVersion => f.write_str("bad version"),
        }
    }
}

impl Error for ReadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            ReadErrorKind::Decode(e) => Some(e),
            ReadErrorKind::Read(e) => Some(e),
            ReadErrorKind::InvalidEnumValue(e) => Some(e),
            ReadErrorKind::FromUtf8(e) => Some(e),
            _ => None,
        }
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum ReadErrorKind {
    Decode(DecodeError),
    Read(io::Error),
    FromUtf8(FromUtf8Error),
    InvalidEnumValue(InvalidEnumValueError),

    #[non_exhaustive]
    UnexpectedValue {
        value: String,
        expected: String,
    },

    #[non_exhaustive]
    BadMagic,

    #[non_exhaustive]
    BadVersion,
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
            "{:#0x} is not a valid value for {}",
            self.value, self.enum_name
        )
    }
}

impl Error for InvalidEnumValueError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        None
    }
}

impl From<InvalidEnumValueError> for ReadErrorKind {
    fn from(e: InvalidEnumValueError) -> ReadErrorKind {
        ReadErrorKind::InvalidEnumValue(e)
    }
}

impl ReadError {
    pub fn at_offset(error: impl Into<ReadErrorKind>, offset: usize) -> Self {
        let kind = error.into();
        ReadError { offset, kind }
    }
}
