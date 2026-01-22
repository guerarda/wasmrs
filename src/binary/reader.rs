mod leb128;

use leb128::DecodeError;

use std::io::BufRead;
use std::{error, fmt, io, ops::Range, string::FromUtf8Error};

pub struct Reader<'a> {
    pub cursor: io::Cursor<&'a [u8]>,
    range: Range<u64>,
}

impl<'a> Reader<'a> {
    pub fn from_bytes(bytes: &'a [u8], pos: usize) -> Self {
        let mut r = Reader {
            cursor: io::Cursor::new(bytes),
            range: (pos as u64)..(bytes.len() as u64),
        };
        r.cursor.set_position(pos as u64);
        r
    }

    pub fn from_bytes_range(
        bytes: &'a [u8],
        start: usize,
        end: usize,
    ) -> std::result::Result<Self, ReadError> {
        // Check that the range is valid for the slice
        if bytes.get(start..end).is_none() {
            return Err(ReadError {
                offset: start,
                kind: ReadErrorKind::OutOfRange {
                    size: (end - start) as u32,
                    remaining: (bytes.len() - start) as u64,
                },
            });
        }
        let mut r = Reader {
            cursor: io::Cursor::new(bytes),
            range: (start as u64)..(end as u64),
        };
        r.cursor.set_position(start as u64);
        Ok(r)
    }

    pub fn scoped(&mut self, size: u32) -> Result<Reader<'a>> {
        let start = self.position();
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
            cursor: io::Cursor::new(self.cursor.get_ref()),
            range: start..end,
        };
        sub.cursor.set_position(start);

        Ok(sub)
    }

    pub fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        let offset = self.cursor.position() as usize;
        std::io::Read::read_exact(self, buf).map_err(|e| ReadError {
            offset,
            kind: ReadErrorKind::ReadExact {
                len: buf.len(),
                source: e,
            },
        })
    }

    pub fn position(&self) -> u64 {
        self.cursor.position()
    }

    pub fn is_exhausted(&self) -> bool {
        self.position() >= self.range.end
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

    pub fn peek(&mut self) -> Result<u8> {
        let offset = self.cursor.position() as usize;
        Ok(self.cursor.fill_buf().map_err(|e| ReadError {
            offset,
            kind: ReadErrorKind::Read(e),
        })?[0])
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

    pub fn read_u64(&mut self) -> Result<u64> {
        let offset = self.cursor.position() as usize;
        leb128::read_leb128_u64(self).map_err(|e| ReadError {
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

    pub fn read_i64(&mut self) -> Result<i64> {
        let offset = self.cursor.position() as usize;
        leb128::read_leb128_i64(self).map_err(|e| ReadError {
            offset,
            kind: ReadErrorKind::Decode(e),
        })
    }

    pub fn read<T: FromReader<'a>>(&mut self) -> std::result::Result<T, T::Error> {
        T::from_reader(self)
    }

    pub fn read_name(&mut self) -> Result<String> {
        let offset = self.position() as usize;

        let len = self.read_u32()?;
        let mut bytes = vec![0u8; len as usize];
        self.read_exact(&mut bytes)?;

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
    type Error;

    fn from_reader(reader: &mut Reader<'a>) -> std::result::Result<Self, Self::Error>;
}

impl<'a, T: FromReader<'a>> FromReader<'a> for Vec<T> {
    type Error = VecReadError<T::Error>;

    fn from_reader(reader: &mut Reader<'a>) -> std::result::Result<Self, Self::Error> {
        let len = reader.read_u32().map_err(VecReadError::Count)? as usize;
        (0..len)
            .map(|index| {
                T::from_reader(reader).map_err(|source| VecReadError::Element { index, source })
            })
            .collect()
    }
}

impl<'a> FromReader<'a> for u8 {
    type Error = ReadError;

    fn from_reader(reader: &mut Reader<'a>) -> Result<Self> {
        reader.read_u8()
    }
}

impl<'a> FromReader<'a> for u32 {
    type Error = ReadError;

    fn from_reader(reader: &mut Reader<'a>) -> Result<Self> {
        reader.read_u32()
    }
}

impl<'a> FromReader<'a> for u64 {
    type Error = ReadError;

    fn from_reader(reader: &mut Reader<'a>) -> Result<Self> {
        reader.read_u64()
    }
}

impl<'a> FromReader<'a> for i32 {
    type Error = ReadError;

    fn from_reader(reader: &mut Reader<'a>) -> Result<Self> {
        reader.read_i32()
    }
}

impl<'a> FromReader<'a> for i64 {
    type Error = ReadError;

    fn from_reader(reader: &mut Reader<'a>) -> Result<Self> {
        reader.read_i64()
    }
}

impl<'a> FromReader<'a> for String {
    type Error = ReadError;

    fn from_reader(reader: &mut Reader<'a>) -> Result<Self> {
        reader.read_name()
    }
}

/// Errors
#[derive(Debug)]
#[non_exhaustive]
pub struct ReadError {
    pub offset: usize,
    pub kind: ReadErrorKind,
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ReadErrorKind::Decode(_) => {
                write!(f, "decoding byte at offset {}", self.offset)
            }
            ReadErrorKind::ReadExact { len, .. } => {
                write!(f, "reading {len} bytes starting at offset {}", self.offset)
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

impl error::Error for ReadError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match &self.kind {
            ReadErrorKind::Decode(e) => Some(e),
            ReadErrorKind::Read(e) => Some(e),
            ReadErrorKind::ReadExact { source, .. } => Some(source),
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
    ReadExact {
        len: usize,
        source: io::Error,
    },
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

impl fmt::Display for InvalidEnumValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:#0x} is not a valid value for {}",
            self.value, self.enum_name
        )
    }
}

impl error::Error for InvalidEnumValueError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
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

#[derive(Debug)]
#[non_exhaustive]
pub enum VecReadError<E> {
    //#[error("reading the vector element count")]
    Count(ReadError),

    //#[error("reading element at index {index}")]
    Element { index: usize, source: E },
}

impl<E> error::Error for VecReadError<E>
where
    E: error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Count(e) => Some(e),
            Self::Element { source, .. } => Some(source),
        }
    }
}

impl<E> fmt::Display for VecReadError<E>
where
    E: std::error::Error + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Count(_) => write!(f, "reading the vector element count"),
            Self::Element { index, .. } => write!(f, "reading element at {index}"),
        }
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
