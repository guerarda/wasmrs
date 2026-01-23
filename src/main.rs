use std::fs;
use wasmrs::runtime::Runtime;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <input.wasm>", args[0]);
        std::process::exit(1);
    }
    let bytes = fs::read(&args[1])?;

    let mut runtime = Runtime::default();
    let _ = runtime.load_module(&bytes)?;

    dbg!(runtime);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasmrs::{
        Error,
        runtime::{TrapError, value::Value},
    };

    #[test]
    fn test_add_wasm() -> anyhow::Result<()> {
        let bytes = fs::read("tests/fixtures/add.wasm")?;
        let mut runtime = Runtime::default();
        let mh = runtime.load_module(&bytes)?;

        let cases = [
            // (a + b = c)
            (1, 2, 3),
            (2, 2, 4),
            (-2, 2, 0),
            // wrap-around
            (0x7fffffff, 1, 0x80000000u32 as i32),
            (0x80000000u32 as i32, -1, 0x7fffffff),
            (0x80000000u32 as i32, 0x80000000u32 as i32, 0),
            (0x3fffffff, 1, 0x40000000),
        ];

        for (a, b, c) in cases {
            let r = runtime.invoke(mh, "add", &[Value::I32(a), Value::I32(b)])?;

            assert_eq!(r.len(), 1);
            match r[0] {
                Value::I32(v) => assert_eq!(v, c, "{} + {} = {}", a, b, c),
                _ => panic!(),
            }
        }

        Ok(())
    }

    #[test]
    fn test_fib_wasm() -> anyhow::Result<()> {
        let bytes = fs::read("tests/fixtures/fib.wasm")?;
        let mut runtime = Runtime::default();
        let mh = runtime.load_module(&bytes)?;

        let cases = [
            // fib(a) = b
            (0, 0),
            (1, 1),
            (7, 13),
            (10, 55),
            (20, 6765),
        ];

        for (a, b) in cases {
            let r = runtime.invoke(mh, "fib", &[Value::I32(a)])?;

            assert_eq!(r.len(), 1);
            match r[0] {
                Value::I32(v) => assert_eq!(v, b, "F({}) = {}", a, b),
                _ => panic!(),
            }
        }

        Ok(())
    }

    #[test]
    fn test_mul_wasm() -> anyhow::Result<()> {
        let bytes = fs::read("tests/fixtures/mul.wasm")?;
        let mut runtime = Runtime::default();
        let mh = runtime.load_module(&bytes)?;

        let cases = [
            // (a * b = c)
            (1, 2, 2),
            (2, 2, 4),
            (-2, 2, -4),
        ];

        for (a, b, c) in cases {
            let r = runtime.invoke(mh, "mul", &[Value::I32(a), Value::I32(b)])?;

            assert_eq!(r.len(), 1);
            match r[0] {
                Value::I32(v) => assert_eq!(v, c, "{} * {} = {}", a, b, c),
                _ => panic!(),
            }
        }

        Ok(())
    }

    #[test]
    fn test_divs_wasm() -> anyhow::Result<()> {
        let bytes = fs::read("tests/fixtures/div_s.wasm")?;
        let mut runtime = Runtime::default();
        let mh = runtime.load_module(&bytes)?;

        let cases = [
            // (a / b = c)
            (1, 1, 1),
            (0, 1, 0),
            (0, -1, 0),
            (-1, -1, 1),
            (0x80000000u32 as i32, 2, 0xc0000000u32 as i32),
            (0x80000001u32 as i32, 1000, 0xffdf3b65u32 as i32),
        ];

        for (a, b, c) in cases {
            let r = runtime.invoke(mh, "div", &[Value::I32(a), Value::I32(b)])?;

            assert_eq!(r.len(), 1);
            match r[0] {
                Value::I32(v) => assert_eq!(v, c, "{} / {} = {}", a, b, c),
                _ => panic!(),
            }
        }

        Ok(())
    }

    #[test]
    fn test_divs_wasm_trap() -> anyhow::Result<()> {
        let bytes = fs::read("tests/fixtures/div_s.wasm")?;
        let mut runtime = Runtime::default();
        let mh = runtime.load_module(&bytes)?;

        let cases = [
            // (a / b)
            (1, 0),
            (0, 0),
            (0x80000000u32 as i32, -1),
            (0x80000000u32 as i32, 0),
        ];

        for (a, b) in cases {
            let r = runtime.invoke(mh, "div", &[Value::I32(a), Value::I32(b)]);

            assert!(matches!(r, Err(Error::Trap(TrapError::Unexpected))));
        }

        Ok(())
    }
}
