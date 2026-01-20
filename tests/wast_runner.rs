use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use libtest_mimic::{Arguments, Failed, Trial};
use std::collections::HashMap;
use wast::core::{NanPattern, WastArgCore, WastRetCore};
use wast::parser::{self, ParseBuffer};
use wast::{QuoteWatTest, Wast, WastArg, WastDirective, WastExecute, WastRet};

use wasmrs::Error;
use wasmrs::runtime::Runtime;
use wasmrs::runtime::value::Value;
use wasmrs::{parse_module, validate_module};

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

/// A single assertion expecting success
struct ReturnAssertion {
    line: usize,
    func_name: String,
    args: Vec<TestArg>,
    expected: Vec<TestRet>,
}

/// A single assertion expecting a trap
struct TrapAssertion {
    line: usize,
    func_name: String,
    args: Vec<TestArg>,
    message: String,
}

/// Union of assertion types
enum Assertion {
    Return(ReturnAssertion),
    Trap(TrapAssertion),
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
    #[allow(dead_code)] // TODO: Enable once validate_module is implemented
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
    let mut detailed = false;
    let args: Vec<String> = std::env::args()
        .filter(|arg| {
            if arg == "--detailed" {
                detailed = true;
                false
            } else {
                true
            }
        })
        .collect();
    let args = Arguments::from_iter(args);
    let tests = collect_tests(detailed);
    libtest_mimic::run(&args, tests).exit();
}

/// A collected test - either runnable or ignored
enum CollectedTest {
    Run(TestCase),
    Ignored,
}

/// Collect test cases from all wast files, grouped by file
fn collect_file_test_cases() -> HashMap<String, Vec<(String, CollectedTest)>> {
    let mut file_tests: HashMap<String, Vec<(String, CollectedTest)>> = HashMap::new();

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
        let tests = file_tests.entry(file_name.clone()).or_default();

        // State for grouping Module + AssertReturns
        let mut pending_module: Option<(usize, usize, Vec<u8>)> = None; // (idx, line, bytes)
        let mut pending_assertions: Vec<Assertion> = Vec::new();

        // Helper to flush pending module + assertions as a test
        let flush_pending = |tests: &mut Vec<(String, CollectedTest)>,
                             file_name: &str,
                             pending_module: &mut Option<(usize, usize, Vec<u8>)>,
                             pending_assertions: &mut Vec<Assertion>| {
            if let Some((idx, line, wasm_bytes)) = pending_module.take() {
                let assertions = std::mem::take(pending_assertions);
                if assertions.is_empty() {
                    let test_name = format!("{}::[{}]line_{}::Module", file_name, idx, line);
                    let tc = TestCase::Module { wasm_bytes };
                    tests.push((test_name, CollectedTest::Run(tc)));
                } else {
                    let return_count = assertions
                        .iter()
                        .filter(|a| matches!(a, Assertion::Return(_)))
                        .count();
                    let trap_count = assertions
                        .iter()
                        .filter(|a| matches!(a, Assertion::Trap(_)))
                        .count();
                    let test_name = format!(
                        "{}::[{}]line_{}::ModuleWithAssertions({} return, {} trap)",
                        file_name, idx, line, return_count, trap_count
                    );
                    let tc = TestCase::ModuleWithAssertions {
                        wasm_bytes,
                        assertions,
                    };
                    tests.push((test_name, CollectedTest::Run(tc)));
                }
            }
        };

        for (idx, directive) in wast.directives.into_iter().enumerate() {
            let span = directive.span();
            let (line, _col) = span.linecol_in(&contents);
            let line = line + 1; // 1-indexed

            match directive {
                WastDirective::Module(mut module) => {
                    flush_pending(
                        tests,
                        &file_name,
                        &mut pending_module,
                        &mut pending_assertions,
                    );

                    let wasm_bytes = module.encode().expect("failed to encode module");
                    pending_module = Some((idx, line, wasm_bytes));
                }

                WastDirective::AssertReturn { exec, results, .. } => {
                    if let WastExecute::Invoke(invoke) = exec {
                        let args: Option<Vec<_>> =
                            invoke.args.iter().map(convert_wast_arg).collect();
                        let expected: Option<Vec<_>> =
                            results.iter().map(convert_wast_ret).collect();

                        match (args, expected) {
                            (Some(args), Some(expected)) => {
                                pending_assertions.push(Assertion::Return(ReturnAssertion {
                                    line,
                                    func_name: invoke.name.to_string(),
                                    args,
                                    expected,
                                }));
                            }
                            _ => {
                                let test_name =
                                    format!("{}::[{}]line_{}::AssertReturn", file_name, idx, line);
                                tests.push((test_name, CollectedTest::Ignored));
                            }
                        }
                    } else {
                        let test_name = format!(
                            "{}::[{}]line_{}::AssertReturn(Get/Wat)",
                            file_name, idx, line
                        );
                        tests.push((test_name, CollectedTest::Ignored));
                    }
                }

                WastDirective::AssertMalformed {
                    mut module,
                    message,
                    span: _,
                } => {
                    flush_pending(
                        tests,
                        &file_name,
                        &mut pending_module,
                        &mut pending_assertions,
                    );

                    match module.to_test() {
                        Ok(QuoteWatTest::Binary(wasm_bytes)) => {
                            let test_name =
                                format!("{}::[{}]line_{}::AssertMalformed", file_name, idx, line);
                            let tc = TestCase::AssertMalformed {
                                wasm_bytes,
                                message: message.to_string(),
                            };
                            tests.push((test_name, CollectedTest::Run(tc)));
                        }
                        Ok(QuoteWatTest::Text(_)) => {
                            let test_name = format!(
                                "{}::[{}]line_{}::AssertMalformed (text)",
                                file_name, idx, line
                            );
                            tests.push((test_name, CollectedTest::Ignored));
                        }
                        Err(_) => {
                            let test_name = format!(
                                "{}::[{}]line_{}::AssertMalformed (unparseable)",
                                file_name, idx, line
                            );
                            tests.push((test_name, CollectedTest::Ignored));
                        }
                    }
                }

                WastDirective::AssertInvalid {
                    mut module,
                    message,
                    span: _,
                } => {
                    flush_pending(
                        tests,
                        &file_name,
                        &mut pending_module,
                        &mut pending_assertions,
                    );

                    match module.to_test() {
                        Ok(QuoteWatTest::Binary(wasm_bytes)) => {
                            let test_name =
                                format!("{}::[{}]line_{}::AssertInvalid", file_name, idx, line);
                            // TODO: Enable once validate_module is implemented
                            let _ = (wasm_bytes, &message);
                            tests.push((test_name, CollectedTest::Ignored));
                        }
                        Ok(QuoteWatTest::Text(_)) => {
                            let test_name = format!(
                                "{}::[{}]line_{}::AssertInvalid (text)",
                                file_name, idx, line
                            );
                            tests.push((test_name, CollectedTest::Ignored));
                        }
                        Err(_) => {
                            let test_name = format!(
                                "{}::[{}]line_{}::AssertInvalid (unparseable)",
                                file_name, idx, line
                            );
                            tests.push((test_name, CollectedTest::Ignored));
                        }
                    }
                }

                WastDirective::AssertTrap { exec, message, .. } => {
                    if let WastExecute::Invoke(invoke) = exec {
                        let args: Option<Vec<_>> =
                            invoke.args.iter().map(convert_wast_arg).collect();
                        if let Some(args) = args {
                            pending_assertions.push(Assertion::Trap(TrapAssertion {
                                line,
                                func_name: invoke.name.to_string(),
                                args,
                                message: message.to_string(),
                            }));
                        } else {
                            let test_name =
                                format!("{}::[{}]line_{}::AssertTrap", file_name, idx, line);
                            tests.push((test_name, CollectedTest::Ignored));
                        }
                    } else {
                        let test_name = format!(
                            "{}::[{}]line_{}::AssertTrap(non-invoke)",
                            file_name, idx, line
                        );
                        tests.push((test_name, CollectedTest::Ignored));
                    }
                }

                other => {
                    let kind = match other {
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
                    tests.push((test_name, CollectedTest::Ignored));
                }
            }
        }

        // Flush any remaining module at end of file
        flush_pending(
            tests,
            &file_name,
            &mut pending_module,
            &mut pending_assertions,
        );
    }

    file_tests
}

fn collect_tests(detailed: bool) -> Vec<Trial> {
    let file_tests = collect_file_test_cases();

    if detailed {
        // Detailed mode: one Trial per test case
        let mut trials = Vec::new();
        for (_file, tests) in file_tests {
            for (name, collected) in tests {
                match collected {
                    CollectedTest::Run(tc) => {
                        trials.push(Trial::test(name, move || run_test_case(tc)));
                    }
                    CollectedTest::Ignored => {
                        trials.push(Trial::test(name, || Ok(())).with_ignored_flag(true));
                    }
                }
            }
        }
        trials
    } else {
        // File-level mode: one Trial per file
        let mut trials = Vec::new();
        for (file_name, tests) in file_tests {
            let trial_name = file_name.clone();
            trials.push(Trial::test(trial_name, move || run_file_tests(&file_name, tests)));
        }
        trials
    }
}

fn run_file_tests(file_name: &str, tests: Vec<(String, CollectedTest)>) -> Result<(), Failed> {
    let mut failures = Vec::new();
    let mut run_count = 0;
    let mut ignored_count = 0;

    for (name, collected) in tests {
        match collected {
            CollectedTest::Run(tc) => {
                run_count += 1;
                if let Err(e) = run_test_case(tc) {
                    // Extract short name (remove file prefix)
                    let short_name = name.strip_prefix(&format!("{}::", file_name)).unwrap_or(&name);
                    failures.push(format!("{}: {:?}", short_name, e));
                }
            }
            CollectedTest::Ignored => {
                ignored_count += 1;
            }
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(Failed::from(format!(
            "{}/{} failed ({} ignored):\n{}",
            failures.len(),
            run_count,
            ignored_count,
            failures.join("\n")
        )))
    }
}

fn run_test_case(test_case: TestCase) -> Result<(), Failed> {
    let mut runtime = Runtime::default();

    match test_case {
        TestCase::Module { wasm_bytes } => {
            let result = catch_unwind(AssertUnwindSafe(|| {
                let module = parse_module(&wasm_bytes)?;
                validate_module(&module)
            }));
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
            let result = catch_unwind(AssertUnwindSafe(|| parse_module(&wasm_bytes)));
            match result {
                Ok(Ok(_)) => Err(Failed::from(format!(
                    "expected malformed error '{}', got Ok",
                    message
                ))),
                Ok(Err(Error::Malformed(_))) => Ok(()), // Expected malformed error
                Ok(Err(e)) => Err(Failed::from(format!(
                    "expected malformed error '{}', got different error: {}",
                    message, e
                ))),
                Err(_) => Err(Failed::from(format!(
                    "expected malformed error '{}', got PANIC",
                    message
                ))),
            }
        }

        TestCase::AssertInvalid {
            wasm_bytes,
            message,
        } => {
            let result = catch_unwind(AssertUnwindSafe(|| {
                let module = parse_module(&wasm_bytes)?;
                validate_module(&module)
            }));
            match result {
                Ok(Ok(_)) => Err(Failed::from(format!(
                    "expected validation error '{}', got Ok",
                    message
                ))),
                Ok(Err(Error::Invalid(_))) => Ok(()), // Expected invalid error
                Ok(Err(e)) => Err(Failed::from(format!(
                    "expected validation error '{}', got different error: {}",
                    message, e
                ))),
                Err(_) => Err(Failed::from(format!(
                    "expected validation error '{}', got PANIC",
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
                match assertion {
                    Assertion::Return(a) => {
                        let runtime_args: Vec<_> = a.args.iter().map(test_arg_to_value).collect();

                        let result = catch_unwind(AssertUnwindSafe(|| {
                            runtime.invoke(mh, &a.func_name, &runtime_args)
                        }));

                        let actual = match result {
                            Ok(Ok(values)) => values,
                            Ok(Err(e)) => {
                                failures.push(format!(
                                    "line {}: '{}' trapped: {}",
                                    a.line, a.func_name, e
                                ));
                                continue;
                            }
                            Err(_) => {
                                failures
                                    .push(format!("line {}: '{}' panicked", a.line, a.func_name));
                                continue;
                            }
                        };

                        // Check result count
                        if actual.len() != a.expected.len() {
                            failures.push(format!(
                                "line {}: '{}' returned {} values, expected {}",
                                a.line,
                                a.func_name,
                                actual.len(),
                                a.expected.len()
                            ));
                            continue;
                        }

                        // Check each value
                        for (i, (act, exp)) in actual.iter().zip(a.expected.iter()).enumerate() {
                            if !value_matches(act, exp) {
                                failures.push(format!(
                                    "line {}: '{}' result[{}] mismatch: got {:?}, expected {:?}",
                                    a.line, a.func_name, i, act, exp
                                ));
                            }
                        }
                    }
                    Assertion::Trap(a) => {
                        let runtime_args: Vec<_> = a.args.iter().map(test_arg_to_value).collect();

                        let result = catch_unwind(AssertUnwindSafe(|| {
                            runtime.invoke(mh, &a.func_name, &runtime_args)
                        }));

                        match result {
                            Ok(Ok(_)) => {
                                failures.push(format!(
                                    "line {}: '{}' expected trap '{}', got success",
                                    a.line, a.func_name, a.message
                                ));
                            }
                            Ok(Err(_)) => {} // Expected trap - success
                            Err(_) => {
                                failures.push(format!(
                                    "line {}: '{}' expected trap '{}', got panic",
                                    a.line, a.func_name, a.message
                                ));
                            }
                        }
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
