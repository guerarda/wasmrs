use crate::leb128::{self, DecodeError};

use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    io::{self, BufRead, Cursor},
    ops::Range,
    string::FromUtf8Error,
};

use crate::types::ValType;

pub struct Reader<'a> {
    pub cursor: Cursor<&'a [u8]>,
    range: Range<u64>,
}

impl<'a> Reader<'a> {
    pub fn from_bytes(bytes: &'a [u8], pos: usize) -> Self {
        let mut r = Reader {
            cursor: Cursor::new(bytes),
            range: (pos as u64)..(bytes.len() as u64),
        };
        r.cursor.set_position(pos as u64);
        r
    }

    pub fn scoped(&mut self, size: u32) -> Result<Reader<'a>> {
        self.scoped_at(self.position(), size)
    }

    pub fn scoped_at(&mut self, start: u64, size: u32) -> Result<Reader<'a>> {
        let end = start + size as u64;

        if end > self.range.end {
            let remaining = self.range.end.saturating_sub(start);
            return Err(ReadError {
                offset: start as usize,
                kind: ReadErrorKind::OutOfRange { size, remaining },
            });
        }

        // Advance past the requested range
        self.cursor.set_position(end);
        let mut sub = Reader {
            cursor: Cursor::new(self.cursor.get_ref()),
            range: start..end,
        };
        sub.cursor.set_position(start);

        Ok(sub)
    }

    pub fn is_exhausted(&self) -> bool {
        self.position() >= self.range.end
    }

    pub fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        let offset = self.cursor.position() as usize;
        std::io::Read::read_exact(self, buf).map_err(|e| ReadError {
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
        leb128::read_u8(self).map_err(|e| ReadError {
            offset,
            kind: ReadErrorKind::Decode(e),
        })
    }

    pub fn read_u32(&mut self) -> Result<u32> {
        let offset = self.cursor.position() as usize;
        leb128::read_leb128_u32(self).map_err(|e| ReadError {
            offset,
            kind: ReadErrorKind::Decode(e),
        })
    }

    pub fn read_i32(&mut self) -> Result<i32> {
        let offset = self.cursor.position() as usize;
        leb128::read_leb128_i32(self).map_err(|e| ReadError {
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

    #[allow(dead_code)]
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

impl<'a> io::Read for Reader<'a> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let rem = (self.range.end - self.position()) as usize;
        let req = buf.len().min(rem);

        if req == 0 && !buf.is_empty() {
            return Err(io::Error::other(format!(
                "requested {} bytes past range end ({})",
                buf.len(),
                self.range.end
            )));
        }
        self.cursor.read(&mut buf[..req])
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
                write!(
                    f,
                    "converting value at offset {a:#0x} ({a}), into an enum",
                    a = self.offset
                )
            }
            ReadErrorKind::OutOfRange { size, remaining } => {
                write!(f, "requested {} bytes, only {} available", size, remaining)
            }
            ReadErrorKind::UnexpectedValue { value, expected } => {
                write!(
                    f,
                    "value at offset {}, expected {expected}, got {value} instead",
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
    OutOfRange {
        size: u32,
        remaining: u64,
    },

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_out_of_range() {
        let bytes = [0x83];

        let mut r = Reader::from_bytes(&bytes, 0);
        assert!(r.read_u32().is_err(), "should be out of range")
    }
}
