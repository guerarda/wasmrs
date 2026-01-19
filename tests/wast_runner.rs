use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use libtest_mimic::{Arguments, Failed, Trial};
use wast::core::{NanPattern, WastArgCore, WastRetCore};
use wast::parser::{self, ParseBuffer};
use wast::{QuoteWatTest, Wast, WastArg, WastDirective, WastExecute, WastRet};

use wasmrs::{Runtime, Value};

/// Owned argument value (to avoid lifetime issues with wast's borrowed types)
#[derive(Debug, Clone)]
enum TestArg {
    I32(i32),
    I64(i64),
    F32(u32), // stored as bits
    F64(u64), // stored as bits
}

/// Owned expected return value with NaN pattern support
#[derive(Debug, Clone)]
enum TestRet {
    I32(i32),
    I64(i64),
    F32(TestF32Pattern),
    F64(TestF64Pattern),
}

#[derive(Debug, Clone)]
enum TestF32Pattern {
    Value(u32),
    CanonicalNan,
    ArithmeticNan,
}

#[derive(Debug, Clone)]
enum TestF64Pattern {
    Value(u64),
    CanonicalNan,
    ArithmeticNan,
}

fn convert_wast_arg(arg: &WastArg) -> Option<TestArg> {
    match arg {
        WastArg::Core(core) => match core {
            WastArgCore::I32(v) => Some(TestArg::I32(*v)),
            WastArgCore::I64(v) => Some(TestArg::I64(*v)),
            WastArgCore::F32(f) => Some(TestArg::F32(f.bits)),
            WastArgCore::F64(f) => Some(TestArg::F64(f.bits)),
            _ => None, // V128, RefNull, etc. not yet supported
        },
        _ => None, // Component model not supported
    }
}

fn convert_wast_ret(ret: &WastRet) -> Option<TestRet> {
    match ret {
        WastRet::Core(core) => match core {
            WastRetCore::I32(v) => Some(TestRet::I32(*v)),
            WastRetCore::I64(v) => Some(TestRet::I64(*v)),
            WastRetCore::F32(pattern) => Some(TestRet::F32(match pattern {
                NanPattern::CanonicalNan => TestF32Pattern::CanonicalNan,
                NanPattern::ArithmeticNan => TestF32Pattern::ArithmeticNan,
                NanPattern::Value(f) => TestF32Pattern::Value(f.bits),
            })),
            WastRetCore::F64(pattern) => Some(TestRet::F64(match pattern {
                NanPattern::CanonicalNan => TestF64Pattern::CanonicalNan,
                NanPattern::ArithmeticNan => TestF64Pattern::ArithmeticNan,
                NanPattern::Value(f) => TestF64Pattern::Value(f.bits),
            })),
            _ => None, // V128, refs, etc. not yet supported
        },
        _ => None, // Component model not supported
    }
}

fn test_arg_to_value(arg: &TestArg) -> Value {
    match arg {
        TestArg::I32(v) => Value::I32(*v),
        TestArg::I64(v) => Value::I64(*v),
        TestArg::F32(bits) => Value::F32(f32::from_bits(*bits)),
        TestArg::F64(bits) => Value::F64(f64::from_bits(*bits)),
    }
}

fn value_matches(actual: &Value, expected: &TestRet) -> bool {
    match (actual, expected) {
        (Value::I32(a), TestRet::I32(e)) => a == e,
        (Value::I64(a), TestRet::I64(e)) => a == e,
        (Value::F32(a), TestRet::F32(pattern)) => match pattern {
            TestF32Pattern::Value(e) => a.to_bits() == *e,
            TestF32Pattern::CanonicalNan => {
                a.is_nan() && (a.to_bits() & 0x7fff_ffff) == 0x7fc0_0000
            }
            TestF32Pattern::ArithmeticNan => a.is_nan() && (a.to_bits() & 0x0040_0000) != 0,
        },
        (Value::F64(a), TestRet::F64(pattern)) => match pattern {
            TestF64Pattern::Value(e) => a.to_bits() == *e,
            TestF64Pattern::CanonicalNan => {
                a.is_nan() && (a.to_bits() & 0x7fff_ffff_ffff_ffff) == 0x7ff8_0000_0000_0000
            }
            TestF64Pattern::ArithmeticNan => {
                a.is_nan() && (a.to_bits() & 0x0008_0000_0000_0000) != 0
            }
        },
        _ => false,
    }
}

/// A single assertion to run against a loaded module
struct Assertion {
    line: usize,
    func_name: String,
    args: Vec<TestArg>,
    expected: Vec<TestRet>,
}

/// Represents a test case extracted from a WAST directive
enum TestCase {
    /// Module that should load successfully
    Module { wasm_bytes: Vec<u8> },
    /// Module that should fail to load with a malformed error
    AssertMalformed {
        wasm_bytes: Vec<u8>,
        message: String,
    },
    /// Module that should fail validation (type errors, etc.)
    AssertInvalid {
        wasm_bytes: Vec<u8>,
        message: String,
    },
    /// Module with assertions to run against it
    ModuleWithAssertions {
        wasm_bytes: Vec<u8>,
        assertions: Vec<Assertion>,
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

        // State for grouping Module + AssertReturns
        let mut pending_module: Option<(usize, usize, Vec<u8>)> = None; // (idx, line, bytes)
        let mut pending_assertions: Vec<Assertion> = Vec::new();

        // Helper to flush pending module + assertions as a test
        let flush_pending = |tests: &mut Vec<Trial>,
                             file_name: &str,
                             pending_module: &mut Option<(usize, usize, Vec<u8>)>,
                             pending_assertions: &mut Vec<Assertion>| {
            if let Some((idx, line, wasm_bytes)) = pending_module.take() {
                let assertions = std::mem::take(pending_assertions);
                if assertions.is_empty() {
                    // No assertions, just test module loading
                    let test_name = format!("{}::[{}]line_{}::Module", file_name, idx, line);
                    let tc = TestCase::Module { wasm_bytes };
                    tests.push(Trial::test(test_name, move || run_test_case(tc)));
                } else {
                    // Module with assertions
                    let count = assertions.len();
                    let test_name = format!(
                        "{}::[{}]line_{}::ModuleWithAssertions({})",
                        file_name, idx, line, count
                    );
                    let tc = TestCase::ModuleWithAssertions {
                        wasm_bytes,
                        assertions,
                    };
                    tests.push(Trial::test(test_name, move || run_test_case(tc)));
                }
            }
        };

        for (idx, directive) in wast.directives.into_iter().enumerate() {
            let span = directive.span();
            let (line, _col) = span.linecol_in(&contents);
            let line = line + 1; // 1-indexed

            match directive {
                WastDirective::Module(mut module) => {
                    // Flush any pending module + assertions first
                    flush_pending(
                        &mut tests,
                        &file_name,
                        &mut pending_module,
                        &mut pending_assertions,
                    );

                    let wasm_bytes = module.encode().expect("failed to encode module");
                    pending_module = Some((idx, line, wasm_bytes));
                }

                WastDirective::AssertReturn { exec, results, .. } => {
                    // Try to convert to an assertion
                    if let WastExecute::Invoke(invoke) = exec {
                        let args: Option<Vec<_>> =
                            invoke.args.iter().map(convert_wast_arg).collect();
                        let expected: Option<Vec<_>> =
                            results.iter().map(convert_wast_ret).collect();

                        match (args, expected) {
                            (Some(args), Some(expected)) => {
                                pending_assertions.push(Assertion {
                                    line,
                                    func_name: invoke.name.to_string(),
                                    args,
                                    expected,
                                });
                            }
                            _ => {
                                // Unsupported types - create ignored test
                                let test_name =
                                    format!("{}::[{}]line_{}::AssertReturn", file_name, idx, line);
                                tests.push(
                                    Trial::test(test_name, || Ok(())).with_ignored_flag(true),
                                );
                            }
                        }
                    } else {
                        // WastExecute::Get or Wat not yet supported
                        let test_name = format!(
                            "{}::[{}]line_{}::AssertReturn(Get/Wat)",
                            file_name, idx, line
                        );
                        tests.push(Trial::test(test_name, || Ok(())).with_ignored_flag(true));
                    }
                }

                WastDirective::AssertMalformed {
                    mut module,
                    message,
                    span: _,
                } => {
                    // Flush pending first
                    flush_pending(
                        &mut tests,
                        &file_name,
                        &mut pending_module,
                        &mut pending_assertions,
                    );

                    // Handle as before
                    match module.to_test() {
                        Ok(QuoteWatTest::Binary(wasm_bytes)) => {
                            let test_name =
                                format!("{}::[{}]line_{}::AssertMalformed", file_name, idx, line);
                            let tc = TestCase::AssertMalformed {
                                wasm_bytes,
                                message: message.to_string(),
                            };
                            tests.push(Trial::test(test_name, move || run_test_case(tc)));
                        }
                        Ok(QuoteWatTest::Text(_)) => {
                            let test_name = format!(
                                "{}::[{}]line_{}::AssertMalformed (text)",
                                file_name, idx, line
                            );
                            tests.push(Trial::test(test_name, || Ok(())).with_ignored_flag(true));
                        }
                        Err(_) => {
                            let test_name = format!(
                                "{}::[{}]line_{}::AssertMalformed (unparseable)",
                                file_name, idx, line
                            );
                            tests.push(Trial::test(test_name, || Ok(())).with_ignored_flag(true));
                        }
                    }
                }

                // WastDirective::AssertInvalid {
                //     mut module,
                //     message,
                //     span: _,
                // } => {
                //     // Flush pending first
                //     flush_pending(
                //         &mut tests,
                //         &file_name,
                //         &mut pending_module,
                //         &mut pending_assertions,
                //     );

                //     match module.to_test() {
                //         Ok(QuoteWatTest::Binary(wasm_bytes)) => {
                //             let test_name =
                //                 format!("{}::[{}]line_{}::AssertInvalid", file_name, idx, line);
                //             let tc = TestCase::AssertInvalid {
                //                 wasm_bytes,
                //                 message: message.to_string(),
                //             };
                //             tests.push(Trial::test(test_name, move || run_test_case(tc)));
                //         }
                //         Ok(QuoteWatTest::Text(_)) => {
                //             let test_name = format!(
                //                 "{}::[{}]line_{}::AssertInvalid (text)",
                //                 file_name, idx, line
                //             );
                //             tests.push(Trial::test(test_name, || Ok(())).with_ignored_flag(true));
                //         }
                //         Err(_) => {
                //             let test_name = format!(
                //                 "{}::[{}]line_{}::AssertInvalid (unparseable)",
                //                 file_name, idx, line
                //             );
                //             tests.push(Trial::test(test_name, || Ok(())).with_ignored_flag(true));
                //         }
                //     }
                // }

                // Other directives remain ignored
                other => {
                    let kind = match other {
                        WastDirective::AssertTrap { .. } => "AssertTrap",
                        WastDirective::AssertInvalid { .. } => "AssertInvalid",
                        WastDirective::AssertExhaustion { .. } => "AssertExhaustion",
                        WastDirective::AssertUnlinkable { .. } => "AssertUnlinkable",
                        WastDirective::AssertException { .. } => "AssertException",
                        WastDirective::AssertSuspension { .. } => "AssertSuspension",
                        WastDirective::Invoke(_) => "Invoke",
                        WastDirective::Register { .. } => "Register",
                        WastDirective::Wait { .. } => "Wait",
                        WastDirective::Thread(_) => "Thread",
                        WastDirective::ModuleDefinition(_) => "ModuleDefinition",
                        WastDirective::ModuleInstance { .. } => "ModuleInstance",
                        _ => unreachable!(),
                    };
                    let test_name = format!("{}::[{}]line_{}::{}", file_name, idx, line, kind);
                    tests.push(Trial::test(test_name, || Ok(())).with_ignored_flag(true));
                }
            }
        }

        // Flush any remaining module at end of file
        flush_pending(
            &mut tests,
            &file_name,
            &mut pending_module,
            &mut pending_assertions,
        );
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

        TestCase::AssertInvalid {
            wasm_bytes,
            message,
        } => {
            let result = catch_unwind(AssertUnwindSafe(|| runtime.load_module(&wasm_bytes)));
            match result {
                Ok(Ok(_)) => Err(Failed::from(format!(
                    "expected error '{}', got Ok",
                    message
                ))),
                Ok(Err(_)) => Ok(()), // Any error (not panic) is acceptable
                Err(_) => Err(Failed::from(format!(
                    "expected error '{}', got PANIC in runtime",
                    message
                ))),
            }
        }

        TestCase::ModuleWithAssertions {
            wasm_bytes,
            assertions,
        } => {
            // Load the module
            let result = catch_unwind(AssertUnwindSafe(|| runtime.load_module(&wasm_bytes)));
            let mh = match result {
                Ok(Ok(mh)) => mh,
                Ok(Err(e)) => return Err(Failed::from(format!("module load failed: {}", e))),
                Err(_) => return Err(Failed::from("module load panicked")),
            };

            // Run each assertion, collecting all failures
            let mut failures = Vec::new();
            let total = assertions.len();

            for assertion in assertions {
                let runtime_args: Vec<_> = assertion.args.iter().map(test_arg_to_value).collect();

                let result = catch_unwind(AssertUnwindSafe(|| {
                    runtime.invoke(mh, &assertion.func_name, &runtime_args)
                }));

                let actual = match result {
                    Ok(values) => values,
                    Err(_) => {
                        failures.push(format!(
                            "line {}: invoke '{}' panicked",
                            assertion.line, assertion.func_name
                        ));
                        continue;
                    }
                };

                // Check result count
                if actual.len() != assertion.expected.len() {
                    failures.push(format!(
                        "line {}: '{}' returned {} values, expected {}",
                        assertion.line,
                        assertion.func_name,
                        actual.len(),
                        assertion.expected.len()
                    ));
                    continue;
                }

                // Check each value
                for (i, (act, exp)) in actual.iter().zip(assertion.expected.iter()).enumerate() {
                    if !value_matches(act, exp) {
                        failures.push(format!(
                            "line {}: '{}' result[{}] mismatch: got {:?}, expected {:?}",
                            assertion.line, assertion.func_name, i, act, exp
                        ));
                    }
                }
            }

            if failures.is_empty() {
                Ok(())
            } else {
                let failed_count = failures.len();
                Err(Failed::from(format!(
                    "{}/{} assertions failed:\n{}",
                    failed_count,
                    total,
                    failures.join("\n")
                )))
            }
        }
    }
}
