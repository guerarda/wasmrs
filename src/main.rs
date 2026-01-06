use std::fs;
use wasmrs::{Runtime, Value};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <input.wasm>", args[0]);
        std::process::exit(1);
    }
    let bytes = fs::read(&args[1])?;

    let mut runtime = Runtime::default();
    let mh = runtime.load_module(&bytes)?;

    let r = runtime.invoke(mh, "fib", &[Value::I32(20)]);
    dbg!(r);

    Ok(())
}
