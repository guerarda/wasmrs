use core::fmt;
use std::{
    fs,
    io::{Cursor, Error, ErrorKind, Read, Seek, SeekFrom},
};

const WASM_MAGIC: [u8; 4] = *b"\0asm";
const WASM_VERSION: [u8; 4] = [0x01, 0x00, 0x00, 0x00];

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
enum SectionId {
    Custom = 0x00,
    Type = 0x01,
    Import = 0x02,
    Function = 0x03,
    Table = 0x04,
    Memory = 0x05,
    Global = 0x06,
    Export = 0x07,
    Start = 0x08,
    Element = 0x09,
    Code = 0x0a,
    Data = 0x0b,
    DataCount = 0x0c,
}

impl fmt::Display for SectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match *self {
            SectionId::Custom => "Custom(0)",
            SectionId::Type => "Type(1)",
            SectionId::Import => "Import(2)",
            SectionId::Function => "Function(3)",
            SectionId::Table => "Table(4)",
            SectionId::Memory => "Memory(5)",
            SectionId::Global => "Global(6)",
            SectionId::Export => "Export(7)",
            SectionId::Start => "Start(8)",
            SectionId::Element => "Element(9)",
            SectionId::Code => "Code(10)",
            SectionId::Data => "Data(11)",
            SectionId::DataCount => "Data Count(12)",
        };
        f.pad(s)
    }
}

impl TryFrom<u8> for SectionId {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(SectionId::Custom),
            0x01 => Ok(SectionId::Type),
            0x02 => Ok(SectionId::Import),
            0x03 => Ok(SectionId::Function),
            0x04 => Ok(SectionId::Table),
            0x05 => Ok(SectionId::Memory),
            0x06 => Ok(SectionId::Global),
            0x07 => Ok(SectionId::Export),
            0x08 => Ok(SectionId::Start),
            0x09 => Ok(SectionId::Element),
            0x0a => Ok(SectionId::Code),
            0x0b => Ok(SectionId::Data),
            0x0c => Ok(SectionId::DataCount),
            _ => Err("unknown section id"),
        }
    }
}

struct Module {
    bytes: Vec<u8>,
}

#[derive(Debug)]
struct SectionInfo {
    id: SectionId,
    start: u64,
    end: u64,
    size: u32,
}

fn read_u8<R: std::io::Read>(reader: &mut R) -> std::io::Result<u8> {
    let mut buf = [0u8; 1];
    reader.read_exact(&mut buf)?;
    Ok(buf[0])
}

fn read_leb128_u32<R: std::io::Read>(reader: &mut R) -> std::io::Result<u32> {
    const MAX_BYTES: u32 = u32::BITS / 7 + 1;
    const MAX_LAST_BYTE: u8 = (1 << (u32::BITS % 7)) - 1;

    let mut x = 0;
    let mut s = 0;
    let mut i = 0;

    while let Ok(v) = read_u8(reader) {
        if i == MAX_BYTES {
            return Err(Error::new(ErrorKind::Other, "too many bytes"));
        }
        if v < 0x80 {
            if i == MAX_BYTES - 1 && v > MAX_LAST_BYTE {
                return Err(Error::new(ErrorKind::Other, "overflow"));
            }
            x |= u32::from(v) << s;
            return Ok(x);
        }
        x |= u32::from(v & 0x7f) << s;
        s += 7;
        i += 1;
    }

    return Err(Error::new(ErrorKind::Other, "unexpected end"));
}

#[allow(dead_code)]
fn read_leb128_i32<R: std::io::Read>(reader: &mut R) -> std::io::Result<i32> {
    const MAX_BYTES: u32 = u32::BITS / 7 + 1;

    let mut x = 0;
    let mut s = 0;
    let mut i = 0;

    while let Ok(v) = read_u8(reader) {
        if i == MAX_BYTES {
            return Err(Error::new(ErrorKind::Other, "too many bytes"));
        }
        if v < 0x80 {
            if i == MAX_BYTES - 1 {
                const MASK: u8 = ((-1i8 << ((u32::BITS % 7) - 1)) & 0x7f) as u8;
                if v & MASK != 0 && v < MASK {
                    return Err(Error::new(ErrorKind::Other, "overflow"));
                }
            }

            x |= i32::from(v) << s;
            if i < MAX_BYTES - 1 && v >= 0x40 {
                x |= !0 << (s + 7);
            }

            return Ok(x);
        }
        x |= i32::from(v & 0x7f) << s;
        s += 7;
        i += 1;
    }

    return Err(Error::new(ErrorKind::Other, "unexpected end"));
}

struct Reader<'a> {
    cursor: Cursor<&'a [u8]>,
}

impl<'a> Reader<'a> {
    fn from_module(module: &'a Module, offset: u64) -> Self {
        let mut r = Reader {
            cursor: Cursor::new(&module.bytes),
        };
        r.cursor.set_position(offset);
        r
    }

    fn read_u8(&mut self) -> std::io::Result<u8> {
        read_u8(&mut self.cursor)
    }

    fn read_u32(&mut self) -> std::io::Result<u32> {
        read_leb128_u32(&mut self.cursor)
    }
}

struct ModuleReader<'a> {
    reader: Reader<'a>,
}

impl<'a> ModuleReader<'a> {
    fn from_module(module: &'a Module) -> Self {
        ModuleReader {
            reader: Reader::from_module(module, 0),
        }
    }

    fn read_preamble(&mut self) -> std::io::Result<()> {
        let mut buf = [0u8; 4];

        self.reader.cursor.read_exact(&mut buf)?;
        if buf != WASM_MAGIC {
            return Err(Error::new(ErrorKind::Other, "bad magic"));
        }

        self.reader.cursor.read_exact(&mut buf)?;
        if buf != WASM_VERSION {
            return Err(Error::new(ErrorKind::Other, "bad version"));
        }
        Ok(())
    }

    fn read_section(&mut self) -> std::io::Result<SectionInfo> {
        let id = self.reader.read_u8()?;
        let size = self.reader.read_u32()?;

        Ok(SectionInfo {
            id: id.try_into().unwrap(), // TODO: Errors
            start: self.reader.cursor.position(),
            end: self.reader.cursor.seek(SeekFrom::Current(size as i64))?,
            size,
        })
    }

    fn read_all_sections(&mut self) -> std::io::Result<Vec<SectionInfo>> {
        let mut sections = Vec::new();

        while let Ok(section) = self.read_section() {
            sections.push(section);
        }

        Ok(sections)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <input.wasm>", args[0]);
        std::process::exit(1);
    }
    let bytes = fs::read(&args[1])?;
    let m = Module { bytes };

    let mut r = ModuleReader::from_module(&m);

    r.read_preamble()?;
    let sections = r.read_all_sections()?;

    println!("Sections:");
    for s in sections {
        println!(
            "{:>15}, start={:#010x}, end={:#010x} (size={:#010x})",
            s.id, s.start, s.end, s.size
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_leb128_decode_u32() {
        let cases: &[(&[u8], u32)] = &[
            (&[0x00], 0),
            (&[0x01], 1),
            (&[0x7F], 127),
            (&[0x83, 0x00], 3),
            (&[0x80, 0x01], 128),
            (&[0xFF, 0x01], 255),
            (&[0x80, 0x02], 256),
            (&[0xE5, 0x8E, 0x26], 624485),
            (&[0xFF, 0xFF, 0xFF, 0xFF, 0x0F], u32::MAX),
            (&[0x80, 0x80, 0x80, 0x80, 0x08], 2147483648),
        ];

        for (bytes, expected) in cases {
            let mut slice = &bytes[..];
            let result = read_leb128_u32(&mut slice)
                .unwrap_or_else(|e| panic!("failed to decode {bytes:02X?}: {e}"));
            assert_eq!(result, *expected);
        }
    }

    #[test]
    fn test_leb128_u32_overflow() {
        let cases: &[&[u8]] = &[
            &[0x80, 0x80, 0x80, 0x80, 0x10],
            &[0xFF, 0xFF, 0xFF, 0xFF, 0x1F],
            &[0x80, 0x80, 0x80, 0x80, 0x80, 0x01],
        ];

        for bytes in cases {
            let mut slice = &bytes[..];
            assert!(
                read_leb128_u32(&mut slice).is_err(),
                "should overflow: {bytes:02X?}"
            );
        }
    }

    #[test]
    fn test_leb128_u32_truncated() {
        let cases: &[&[u8]] = &[&[0x80], &[0x80, 0x80]];

        for bytes in cases {
            let mut slice = &bytes[..];
            assert!(read_leb128_u32(&mut slice).is_err());
            assert!(read_leb128_u32(&mut slice).is_err());
        }
    }

    #[test]
    fn test_leb128_u32_empty() {
        let mut slice = &[][..];
        assert!(read_leb128_u32(&mut slice).is_err());
    }
    #[test]
    fn test_leb128_decode_i32() {
        let cases: &[(&[u8], i32)] = &[
            (&[0x00], 0),
            (&[0x01], 1),
            (&[0x7F], -1),
            (&[0x80, 0x01], 128),
            (&[0xFF, 0x00], 127),
            (&[0x80, 0x7F], -128),
            (&[0x81, 0x7F], -127),
            (&[0xC0, 0x00], 64),
            (&[0xC0, 0x7F], -64),
            (&[0xBF, 0x7F], -65),
            (&[0xFF, 0xFF, 0xFF, 0xFF, 0x07], i32::MAX),
            (&[0x80, 0x80, 0x80, 0x80, 0x78], i32::MIN),
        ];

        for (bytes, expected) in cases {
            let mut slice = &bytes[..];
            let result = read_leb128_i32(&mut slice)
                .unwrap_or_else(|e| panic!("failed to decode {bytes:02X?}: {e}"));
            assert_eq!(result, *expected);
        }
    }

    #[test]
    fn test_leb128_i32_overflow() {
        let cases: &[&[u8]] = &[
            &[0x80, 0x80, 0x80, 0x80, 0x08],
            &[0x80, 0x80, 0x80, 0x80, 0x70],
        ];

        for bytes in cases {
            let mut slice = &bytes[..];
            assert!(
                read_leb128_i32(&mut slice).is_err(),
                "should overflow: {bytes:02X?}"
            );
        }
    }

    #[test]
    fn test_leb128_i32_truncated() {
        let mut slice = &[0x80][..];
        assert!(read_leb128_i32(&mut slice).is_err());
    }

    #[test]
    fn test_leb128_i32_empty() {
        let mut slice = &[][..];
        assert!(read_leb128_i32(&mut slice).is_err());
    }
}
