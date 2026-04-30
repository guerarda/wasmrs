use std::fs;
use std::path::PathBuf;
use std::process::{self, Command};

use anyhow::{Context, Result, bail};
use wast::lexer::Lexer;
use wast::parser::{self, ParseBuffer};
use wast::{Wast, WastDirective, WastExecute};

fn main() -> Result<()> {
    let argv: Vec<String> = std::env::args().collect();
    let mut no_run = false;
    let positionals: Vec<&String> = argv
        .iter()
        .enumerate()
        .filter_map(|(i, a)| {
            if i == 0 {
                Some(a)
            } else if a == "--no-run" {
                no_run = true;
                None
            } else {
                Some(a)
            }
        })
        .collect();

    if positionals.len() != 3 {
        eprintln!(
            "usage: {} [--no-run] <file.wast> <test-number>",
            positionals[0]
        );
        process::exit(2);
    }
    let wast_path = PathBuf::from(positionals[1]);
    let n: usize = positionals[2]
        .parse()
        .with_context(|| format!("invalid test number {:?}", positionals[2]))?;

    let contents = fs::read_to_string(&wast_path)
        .with_context(|| format!("failed to read {}", wast_path.display()))?;
    let mut lexer = Lexer::new(&contents);
    lexer.allow_confusing_unicode(true);
    let buf = ParseBuffer::new_with_lexer(lexer).context("failed to create parse buffer")?;
    let mut wast: Wast = parser::parse(&buf).context("failed to parse wast file")?;

    let total = wast.directives.len();
    if n >= total {
        bail!("test number {n} out of range (file has {total} directives)");
    }

    let (line, _) = wast.directives[n].span().linecol_in(&contents);
    let line = line + 1;
    let kind = directive_kind(&wast.directives[n]);

    let target_idx = resolve(&wast.directives, n)?;
    let bytes = encode_directive(&mut wast.directives[target_idx])?;

    println!("directive [{n}] line {line} kind {kind}");
    if target_idx != n {
        println!("  -> targeting module at directive [{target_idx}]");
    }

    let stem = wast_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("module");
    let out = PathBuf::from(format!("target/wast-debug/{stem}-{n}.wasm"));
    fs::create_dir_all(out.parent().unwrap())?;
    if !out.exists() {
        fs::write(&out, &bytes).with_context(|| format!("failed to write {}", out.display()))?;
        println!("wrote {} ({} bytes)", out.display(), bytes.len());
    } else {
        println!("cached {} ({} bytes)", out.display(), bytes.len());
    }

    if no_run {
        return Ok(());
    }

    let status = Command::new(env!("CARGO"))
        .args([
            "run",
            "--quiet",
            "--bin",
            "wasmrs",
            "--",
            out.to_str().unwrap(),
        ])
        .status()
        .context("failed to spawn `cargo run`")?;
    process::exit(status.code().unwrap_or(1));
}

fn directive_kind(d: &WastDirective<'_>) -> &'static str {
    match d {
        WastDirective::Module(_) => "Module",
        WastDirective::ModuleDefinition(_) => "ModuleDefinition",
        WastDirective::ModuleInstance { .. } => "ModuleInstance",
        WastDirective::Register { .. } => "Register",
        WastDirective::Invoke(_) => "Invoke",
        WastDirective::AssertReturn { .. } => "AssertReturn",
        WastDirective::AssertTrap { .. } => "AssertTrap",
        WastDirective::AssertExhaustion { .. } => "AssertExhaustion",
        WastDirective::AssertMalformed { .. } => "AssertMalformed",
        WastDirective::AssertInvalid { .. } => "AssertInvalid",
        WastDirective::AssertUnlinkable { .. } => "AssertUnlinkable",
        WastDirective::AssertException { .. } => "AssertException",
        WastDirective::AssertSuspension { .. } => "AssertSuspension",
        WastDirective::Wait { .. } => "Wait",
        WastDirective::Thread(_) => "Thread",
    }
}

fn referenced_module<'a>(d: &'a WastDirective<'a>) -> Option<&'a str> {
    match d {
        WastDirective::Register { module, .. } => module.map(|id| id.name()),
        WastDirective::Invoke(inv) => inv.module.map(|id| id.name()),
        WastDirective::AssertReturn { exec, .. }
        | WastDirective::AssertTrap { exec, .. }
        | WastDirective::AssertException { exec, .. }
        | WastDirective::AssertSuspension { exec, .. } => match exec {
            WastExecute::Invoke(inv) => inv.module.map(|id| id.name()),
            WastExecute::Get { module, .. } => module.map(|id| id.name()),
            WastExecute::Wat(_) => None,
        },
        WastDirective::AssertExhaustion { call, .. } => call.module.map(|id| id.name()),
        _ => None,
    }
}

fn resolve(directives: &[WastDirective<'_>], n: usize) -> Result<usize> {
    let d = &directives[n];

    let self_carries_module = matches!(
        d,
        WastDirective::Module(_)
            | WastDirective::ModuleDefinition(_)
            | WastDirective::AssertInvalid { .. }
            | WastDirective::AssertMalformed { .. }
            | WastDirective::AssertUnlinkable { .. }
    ) || matches!(
        d,
        WastDirective::AssertTrap {
            exec: WastExecute::Wat(_),
            ..
        } | WastDirective::AssertReturn {
            exec: WastExecute::Wat(_),
            ..
        }
    );

    if self_carries_module {
        return Ok(n);
    }

    let wanted = referenced_module(d);
    for i in (0..n).rev() {
        if let WastDirective::Module(m) = &directives[i] {
            let have = m.name().map(|id| id.name());
            match (wanted, have) {
                (Some(w), Some(h)) if w == h => return Ok(i),
                (None, _) => return Ok(i),
                _ => continue,
            }
        }
    }

    if let Some(name) = wanted {
        bail!("directive [{n}] references module ${name}; no prior `(module ${name} ...)` found");
    }
    bail!(
        "directive [{n}] ({}) has no prior module in file",
        directive_kind(d)
    );
}

fn encode_directive(d: &mut WastDirective<'_>) -> Result<Vec<u8>> {
    match d {
        WastDirective::Module(m) | WastDirective::ModuleDefinition(m) => {
            m.encode().context("encoding module")
        }
        WastDirective::AssertInvalid { module, .. }
        | WastDirective::AssertMalformed { module, .. } => {
            module.encode().context("encoding asserted module")
        }
        WastDirective::AssertUnlinkable { module, .. } => {
            module.encode().context("encoding AssertUnlinkable module")
        }
        WastDirective::AssertTrap {
            exec: WastExecute::Wat(wat),
            ..
        }
        | WastDirective::AssertReturn {
            exec: WastExecute::Wat(wat),
            ..
        } => wat.encode().context("encoding inline module"),
        other => bail!(
            "internal: cannot encode directive kind {}",
            directive_kind(other)
        ),
    }
}
