use std::io::{Error, ErrorKind};

pub fn read_u8<R: std::io::Read>(reader: &mut R) -> std::io::Result<u8> {
    let mut buf = [0u8; 1];
    reader.read_exact(&mut buf)?;
    Ok(buf[0])
}

pub fn read_leb128_u32<R: std::io::Read>(reader: &mut R) -> std::io::Result<u32> {
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
pub fn read_leb128_i32<R: std::io::Read>(reader: &mut R) -> std::io::Result<i32> {
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
