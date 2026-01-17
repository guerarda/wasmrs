use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use libtest_mimic::{Arguments, Failed, Trial};
use wast::parser::{self, ParseBuffer};
use wast::{Wast, WastDirective};

use wasmrs::Runtime;

/// Represents a test case extracted from a WAST directive
enum TestCase {
    /// Module that should load successfully
    Module { wasm_bytes: Vec<u8> },
    /// Module that should fail to load with a malformed error
    AssertMalformed {
        wasm_bytes: Vec<u8>,
        message: String,
    },
}

fn main() {
    let args = Arguments::from_args();
    let tests = collect_tests();
    libtest_mimic::run(&args, tests).exit();
}

fn collect_tests() -> Vec<Trial> {
    let mut tests = Vec::new();

    let spec_dir = Path::new("tests/spec");
    let wast_files: Vec<_> = std::fs::read_dir(spec_dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|ext| ext == "wast"))
        .collect();

    for path in wast_files {
        let contents = std::fs::read_to_string(&path).expect("failed to read wast file");
        let buf = ParseBuffer::new(&contents).expect("failed to create parse buffer");
        let wast: Wast = parser::parse(&buf).expect("failed to parse wast file");

        let file_name = path.file_name().unwrap().to_str().unwrap().to_string();

        for (idx, directive) in wast.directives.into_iter().enumerate() {
            let span = directive.span();
            let (line, _col) = span.linecol_in(&contents);
            let line = line + 1; // 1-indexed

            let (kind, test_case): (&str, Option<TestCase>) = match directive {
                WastDirective::Module(mut module) => {
                    let wasm_bytes = module.encode().expect("failed to encode module");
                    ("Module", Some(TestCase::Module { wasm_bytes }))
                }
                WastDirective::AssertMalformed {
                    mut module,
                    message,
                    span: _,
                } => {
                    let wasm_bytes = module.encode().expect("failed to encode module");
                    (
                        "AssertMalformed",
                        Some(TestCase::AssertMalformed {
                            wasm_bytes,
                            message: message.to_string(),
                        }),
                    )
                }
                WastDirective::AssertInvalid { .. } => ("AssertInvalid", None),
                WastDirective::AssertReturn { .. } => ("AssertReturn", None),
                WastDirective::AssertTrap { .. } => ("AssertTrap", None),
                WastDirective::AssertExhaustion { .. } => ("AssertExhaustion", None),
                WastDirective::AssertUnlinkable { .. } => ("AssertUnlinkable", None),
                WastDirective::AssertException { .. } => ("AssertException", None),
                WastDirective::AssertSuspension { .. } => ("AssertSuspension", None),
                WastDirective::Invoke(_) => ("Invoke", None),
                WastDirective::Register { .. } => ("Register", None),
                WastDirective::Wait { .. } => ("Wait", None),
                WastDirective::Thread(_) => ("Thread", None),
                WastDirective::ModuleDefinition(_) => ("ModuleDefinition", None),
                WastDirective::ModuleInstance { .. } => ("ModuleInstance", None),
            };

            let test_name = format!("{}::[{}]line_{}::{}", file_name, idx, line, kind);

            let trial = match test_case {
                Some(tc) => Trial::test(test_name, move || run_test_case(tc)),
                None => Trial::test(test_name, || Ok(())).with_ignored_flag(true),
            };

            tests.push(trial);
        }
    }

    tests
}

fn run_test_case(test_case: TestCase) -> Result<(), Failed> {
    let mut runtime = Runtime::default();

    match test_case {
        TestCase::Module { wasm_bytes } => {
            let result = catch_unwind(AssertUnwindSafe(|| runtime.load_module(&wasm_bytes)));
            match result {
                Ok(Ok(_)) => Ok(()),
                Ok(Err(e)) => Err(Failed::from(format!("expected Ok, got {}", e))),
                Err(_) => Err(Failed::from("expected Ok, got PANIC in runtime")),
            }
        }
        TestCase::AssertMalformed {
            wasm_bytes,
            message,
        } => {
            let result = catch_unwind(AssertUnwindSafe(|| runtime.load_module(&wasm_bytes)));
            match result {
                Ok(Ok(_)) => Err(Failed::from(format!(
                    "expected error '{}', got Ok",
                    message
                ))),
                Ok(Err(_)) => Ok(()),
                Err(_) => Err(Failed::from(format!(
                    "expected error '{}', got PANIC in runtime",
                    message
                ))),
            }
        }
    }
}
