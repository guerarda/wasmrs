use crate::leb128;

use std::io::{Cursor, Read};

pub struct Reader<'a> {
    cursor: Cursor<&'a [u8]>,
}

impl<'a> Reader<'a> {
    pub fn from_bytes(bytes: &'a [u8]) -> Self {
        let r = Reader {
            cursor: Cursor::new(&bytes),
        };
        r
    }

    pub fn read_exact(&mut self, buf: &mut [u8]) -> std::io::Result<()> {
        self.cursor.read_exact(buf)
    }

    pub fn position(&self) -> u64 {
        self.cursor.position()
    }

    pub fn read_u8(&mut self) -> std::io::Result<u8> {
        leb128::read_u8(&mut self.cursor)
    }

    pub fn read_u32(&mut self) -> std::io::Result<u32> {
        leb128::read_leb128_u32(&mut self.cursor)
    }

    pub fn read_vec<T: FromReader<'a>>(&mut self) -> std::io::Result<Vec<T>> {
        let len = self.read_u32()?;
        (0..len)
            .map(|_| T::from_reader(self))
            .collect::<std::io::Result<Vec<T>>>()
    }
}

pub trait FromReader<'a>: Sized {
    fn from_reader(reader: &mut Reader<'a>) -> std::io::Result<Self>;
}
