use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use libtest_mimic::{Arguments, Failed, Trial};
use std::collections::HashMap;
use wast::core::{NanPattern, WastArgCore, WastRetCore};
use wast::lexer::Lexer;
use wast::parser::{self, ParseBuffer};
use wast::{QuoteWatTest, Wast, WastArg, WastDirective, WastExecute, WastRet};

use wasmrs::Error;
use wasmrs::RefType;
use wasmrs::runtime::Runtime;
use wasmrs::runtime::instance::ModuleHandle;
use wasmrs::runtime::value::{Ref, Value};
use wasmrs::{parse_module, validate_module};

mod spectest;

/// Wast files to skip by default. Use --all to include them.
const EXCLUDED: &[&str] = &["elem.wast"];

/// Owned argument value (to avoid lifetime issues with wast's borrowed types)
#[derive(Debug, Clone)]
enum TestArg {
    I32(i32),
    I64(i64),
    F32(u32), // stored as bits
    F64(u64), // stored as bits
    RefNull(RefType),
    RefExtern(u32),
}

/// Owned expected return value with NaN pattern support
#[derive(Debug, Clone)]
enum TestRet {
    I32(i32),
    I64(i64),
    F32(TestF32Pattern),
    F64(TestF64Pattern),
    RefNull,
    RefExtern(u32),
    RefFunc,
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
            WastArgCore::RefNull(heap_type) => {
                use wast::core::HeapType;
                match heap_type {
                    HeapType::Abstract {
                        ty: wast::core::AbstractHeapType::Func,
                        ..
                    } => Some(TestArg::RefNull(RefType::Func)),
                    HeapType::Abstract {
                        ty: wast::core::AbstractHeapType::Extern,
                        ..
                    } => Some(TestArg::RefNull(RefType::Extern)),
                    _ => None,
                }
            }
            WastArgCore::RefExtern(v) => Some(TestArg::RefExtern(*v)),
            _ => None, // V128, etc. not yet supported
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
            WastRetCore::RefNull(_) => Some(TestRet::RefNull),
            WastRetCore::RefExtern(Some(v)) => Some(TestRet::RefExtern(*v)),
            WastRetCore::RefFunc(_) => Some(TestRet::RefFunc),
            _ => None, // V128, etc. not yet supported
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
        TestArg::RefNull(rt) => Value::Ref(Ref::Null(*rt)),
        TestArg::RefExtern(v) => Value::Ref(Ref::Extern(*v)),
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
        (Value::Ref(r), TestRet::RefNull) => r.is_null(),
        (Value::Ref(Ref::Extern(v)), TestRet::RefExtern(e)) => v == e,
        (Value::Ref(Ref::Func(_)), TestRet::RefFunc) => true,
        _ => false,
    }
}

/// A single assertion expecting success
struct ReturnAssertion {
    line: usize,
    module_name: Option<String>,
    func_name: String,
    args: Vec<TestArg>,
    expected: Vec<TestRet>,
}

/// A single assertion expecting a trap
struct TrapAssertion {
    line: usize,
    module_name: Option<String>,
    func_name: String,
    args: Vec<TestArg>,
    message: String,
}

/// A bare invoke (side-effecting, no assertion on return value)
struct InvokeAction {
    line: usize,
    module_name: Option<String>,
    func_name: String,
    args: Vec<TestArg>,
}

/// A single test action extracted from a WAST directive
enum TestAction {
    /// Module that should load successfully
    LoadModule {
        wasm_bytes: Vec<u8>,
        name: Option<String>,
    },
    /// Register the current module under a name
    Register {
        name: String,
        module_name: Option<String>,
    },
    /// Assert a function returns expected values
    AssertReturn(ReturnAssertion),
    /// Assert a function traps
    AssertTrap(TrapAssertion),
    /// Assert a function exhausts the stack
    AssertExhaustion(TrapAssertion),
    /// Invoke a function (no assertion on return value)
    Invoke(InvokeAction),
    /// Module whose instantiation should trap
    AssertModuleTrap {
        wasm_bytes: Vec<u8>,
        message: String,
    },
    /// Module that should fail to parse
    AssertMalformed {
        wasm_bytes: Vec<u8>,
        message: String,
    },
    /// Module that should fail validation
    AssertInvalid {
        wasm_bytes: Vec<u8>,
        message: String,
    },
}

fn main() {
    let mut detailed = false;
    let mut run_assert_invalid = true;
    let mut run_all = false;
    let args: Vec<String> = std::env::args()
        .filter(|arg| {
            if arg == "--detailed" {
                detailed = true;
                false
            } else if arg == "--no-assert" {
                run_assert_invalid = false;
                false
            } else if arg == "--all" {
                run_all = true;
                false
            } else {
                true
            }
        })
        .collect();
    let args = Arguments::from_iter(args);
    let tests = collect_tests(detailed, run_assert_invalid, run_all);
    libtest_mimic::run(&args, tests).exit();
}

/// A collected test - either runnable or ignored
enum CollectedTest {
    Run(TestAction),
    Ignored,
}

/// Collect test actions from all wast files, grouped by file
fn collect_file_test_actions(
    run_assert_invalid: bool,
    run_all: bool,
) -> HashMap<String, Vec<(String, CollectedTest)>> {
    let mut file_tests: HashMap<String, Vec<(String, CollectedTest)>> = HashMap::new();

    let spec_dir = Path::new("tests/wasm-spec/test/core");
    let wast_files: Vec<_> = std::fs::read_dir(spec_dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|ext| ext == "wast"))
        .filter(|p| {
            let name = p.file_name().unwrap().to_str().unwrap();
            if !run_all && EXCLUDED.contains(&name) {
                return false;
            }
            true
        })
        .collect();

    for path in wast_files {
        let contents = std::fs::read_to_string(&path).expect("failed to read wast file");
        let mut lexer = Lexer::new(&contents);
        lexer.allow_confusing_unicode(true);
        let buf = ParseBuffer::new_with_lexer(lexer).expect("failed to create parse buffer");
        let wast: Wast = parser::parse(&buf).expect(&format!("failed to parse wast file {path:?}"));

        let file_name = path.file_name().unwrap().to_str().unwrap().to_string();
        let tests = file_tests.entry(file_name.clone()).or_default();

        for directive in wast.directives {
            let span = directive.span();
            let (line, _col) = span.linecol_in(&contents);
            let line = line + 1; // 1-indexed

            match directive {
                WastDirective::Module(mut module) => {
                    let mod_name = module.name().map(|id| id.name().to_string());
                    let wasm_bytes = module.encode().expect("failed to encode module");
                    let test_name =
                        format!("{}::[{}]line_{}::Module", file_name, tests.len(), line);
                    tests.push((
                        test_name,
                        CollectedTest::Run(TestAction::LoadModule {
                            wasm_bytes,
                            name: mod_name,
                        }),
                    ));
                }

                WastDirective::Register { name, module, .. } => {
                    let mod_name = module.map(|id| id.name().to_string());
                    let test_name =
                        format!("{}::[{}]line_{}::Register", file_name, tests.len(), line);
                    tests.push((
                        test_name,
                        CollectedTest::Run(TestAction::Register {
                            name: name.to_string(),
                            module_name: mod_name,
                        }),
                    ));
                }

                WastDirective::AssertReturn { exec, results, .. } => {
                    if let WastExecute::Invoke(invoke) = exec {
                        let module_name = invoke.module.map(|id| id.name().to_string());
                        let args: Option<Vec<_>> =
                            invoke.args.iter().map(convert_wast_arg).collect();
                        let expected: Option<Vec<_>> =
                            results.iter().map(convert_wast_ret).collect();

                        match (args, expected) {
                            (Some(args), Some(expected)) => {
                                let test_name = format!(
                                    "{}::[{}]line_{}::AssertReturn",
                                    file_name,
                                    tests.len(),
                                    line
                                );
                                tests.push((
                                    test_name,
                                    CollectedTest::Run(TestAction::AssertReturn(ReturnAssertion {
                                        line,
                                        module_name,
                                        func_name: invoke.name.to_string(),
                                        args,
                                        expected,
                                    })),
                                ));
                            }
                            _ => {
                                let test_name = format!(
                                    "{}::[{}]line_{}::AssertReturn",
                                    file_name,
                                    tests.len(),
                                    line
                                );
                                tests.push((test_name, CollectedTest::Ignored));
                            }
                        }
                    } else {
                        let test_name = format!(
                            "{}::[{}]line_{}::AssertReturn(Get/Wat)",
                            file_name,
                            tests.len(),
                            line
                        );
                        tests.push((test_name, CollectedTest::Ignored));
                    }
                }

                WastDirective::AssertMalformed {
                    mut module,
                    message,
                    span: _,
                } => match module.to_test() {
                    Ok(QuoteWatTest::Binary(wasm_bytes)) => {
                        let test_name = format!(
                            "{}::[{}]line_{}::AssertMalformed",
                            file_name,
                            tests.len(),
                            line
                        );
                        tests.push((
                            test_name,
                            CollectedTest::Run(TestAction::AssertMalformed {
                                wasm_bytes,
                                message: message.to_string(),
                            }),
                        ));
                    }
                    Ok(QuoteWatTest::Text(_)) => {
                        let test_name = format!(
                            "{}::[{}]line_{}::AssertMalformed (text)",
                            file_name,
                            tests.len(),
                            line
                        );
                        tests.push((test_name, CollectedTest::Ignored));
                    }
                    Err(_) => {
                        let test_name = format!(
                            "{}::[{}]line_{}::AssertMalformed (unparseable)",
                            file_name,
                            tests.len(),
                            line
                        );
                        tests.push((test_name, CollectedTest::Ignored));
                    }
                },

                WastDirective::AssertInvalid {
                    span: _,
                    mut module,
                    message,
                } => {
                    if run_assert_invalid {
                        let wasm_bytes = module.encode().expect("failed to encode module");
                        let test_name = format!(
                            "{}::[{}]line_{}::AssertInvalid",
                            file_name,
                            tests.len(),
                            line
                        );
                        tests.push((
                            test_name,
                            CollectedTest::Run(TestAction::AssertInvalid {
                                wasm_bytes,
                                message: message.to_string(),
                            }),
                        ));
                    } else {
                        let test_name = format!(
                            "{}::[{}]line_{}::AssertInvalid (disabled)",
                            file_name,
                            tests.len(),
                            line
                        );
                        tests.push((test_name, CollectedTest::Ignored));
                    }
                }

                WastDirective::AssertTrap { exec, message, .. } => {
                    if let WastExecute::Invoke(invoke) = exec {
                        let module_name = invoke.module.map(|id| id.name().to_string());
                        let args: Option<Vec<_>> =
                            invoke.args.iter().map(convert_wast_arg).collect();
                        if let Some(args) = args {
                            let test_name = format!(
                                "{}::[{}]line_{}::AssertTrap",
                                file_name,
                                tests.len(),
                                line
                            );
                            tests.push((
                                test_name,
                                CollectedTest::Run(TestAction::AssertTrap(TrapAssertion {
                                    line,
                                    module_name,
                                    func_name: invoke.name.to_string(),
                                    args,
                                    message: message.to_string(),
                                })),
                            ));
                        } else {
                            let test_name = format!(
                                "{}::[{}]line_{}::AssertTrap",
                                file_name,
                                tests.len(),
                                line
                            );
                            tests.push((test_name, CollectedTest::Ignored));
                        }
                    } else if let WastExecute::Wat(mut wat) = exec {
                        let wasm_bytes = wat.encode().expect("failed to encode module");
                        let test_name = format!(
                            "{}::[{}]line_{}::AssertModuleTrap",
                            file_name,
                            tests.len(),
                            line
                        );
                        tests.push((
                            test_name,
                            CollectedTest::Run(TestAction::AssertModuleTrap {
                                wasm_bytes,
                                message: message.to_string(),
                            }),
                        ));
                    } else {
                        let test_name = format!(
                            "{}::[{}]line_{}::AssertTrap(non-invoke)",
                            file_name,
                            tests.len(),
                            line
                        );
                        tests.push((test_name, CollectedTest::Ignored));
                    }
                }

                WastDirective::Invoke(invoke) => {
                    let module_name = invoke.module.map(|id| id.name().to_string());
                    let args: Option<Vec<_>> = invoke.args.iter().map(convert_wast_arg).collect();
                    if let Some(args) = args {
                        let test_name =
                            format!("{}::[{}]line_{}::Invoke", file_name, tests.len(), line);
                        tests.push((
                            test_name,
                            CollectedTest::Run(TestAction::Invoke(InvokeAction {
                                line,
                                module_name,
                                func_name: invoke.name.to_string(),
                                args,
                            })),
                        ));
                    }
                }

                WastDirective::AssertExhaustion { call, message, .. } => {
                    let module_name = call.module.map(|id| id.name().to_string());
                    let args: Option<Vec<_>> = call.args.iter().map(convert_wast_arg).collect();
                    if let Some(args) = args {
                        let test_name = format!(
                            "{}::[{}]line_{}::AssertExhaustion",
                            file_name,
                            tests.len(),
                            line
                        );
                        tests.push((
                            test_name,
                            CollectedTest::Run(TestAction::AssertExhaustion(TrapAssertion {
                                line,
                                module_name,
                                func_name: call.name.to_string(),
                                args,
                                message: message.to_string(),
                            })),
                        ));
                    }
                }

                other => {
                    let kind = match other {
                        WastDirective::AssertUnlinkable { .. } => "AssertUnlinkable",
                        WastDirective::AssertException { .. } => "AssertException",
                        WastDirective::AssertSuspension { .. } => "AssertSuspension",
                        WastDirective::Wait { .. } => "Wait",
                        WastDirective::Thread(_) => "Thread",
                        WastDirective::ModuleDefinition(_) => "ModuleDefinition",
                        WastDirective::ModuleInstance { .. } => "ModuleInstance",
                        _ => unreachable!(),
                    };
                    let test_name =
                        format!("{}::[{}]line_{}::{}", file_name, tests.len(), line, kind);
                    tests.push((test_name, CollectedTest::Ignored));
                }
            }
        }
    }

    file_tests
}

fn collect_tests(detailed: bool, run_assert_invalid: bool, run_all: bool) -> Vec<Trial> {
    let file_tests = collect_file_test_actions(run_assert_invalid, run_all);

    if detailed {
        // Detailed mode: one Trial per file, but with per-action reporting
        // (actions must run sequentially with shared runtime)
        let mut trials = Vec::new();
        for (file_name, tests) in file_tests {
            let trial_name = file_name.clone();
            trials.push(Trial::test(trial_name, move || {
                run_file_actions(&file_name, tests)
            }));
        }
        trials
    } else {
        // File-level mode: one Trial per file
        let mut trials = Vec::new();
        for (file_name, tests) in file_tests {
            let trial_name = file_name.clone();
            trials.push(Trial::test(trial_name, move || {
                run_file_actions(&file_name, tests)
            }));
        }
        trials
    }
}

fn resolve_module(
    module_name: &Option<String>,
    named_modules: &HashMap<String, ModuleHandle>,
    current_module: Option<ModuleHandle>,
) -> Option<ModuleHandle> {
    module_name
        .as_ref()
        .and_then(|n| named_modules.get(n).copied())
        .or(current_module)
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "(non-string panic)".to_string()
    }
}

fn run_file_actions(file_name: &str, actions: Vec<(String, CollectedTest)>) -> Result<(), Failed> {
    let mut runtime = Runtime::default();
    let mut current_module = None;
    let mut named_modules: HashMap<String, ModuleHandle> = HashMap::new();
    let mut failures = Vec::new();
    let mut run_count = 0;
    let mut ignored_count = 0;

    spectest::setup_spectest(&mut runtime);

    for (name, collected) in actions {
        let short_name = name
            .strip_prefix(&format!("{}::", file_name))
            .unwrap_or(&name);

        match collected {
            CollectedTest::Ignored => {
                ignored_count += 1;
            }
            CollectedTest::Run(action) => {
                run_count += 1;
                match action {
                    TestAction::LoadModule { wasm_bytes, name } => {
                        let result =
                            catch_unwind(AssertUnwindSafe(|| runtime.load_module(&wasm_bytes)));
                        match result {
                            Ok(Ok(mh)) => {
                                current_module = Some(mh);
                                if let Some(mod_name) = name {
                                    named_modules.insert(mod_name, mh);
                                }
                            }
                            Ok(Err(e)) => {
                                current_module = None;
                                failures.push(format!("{}: load failed: {}", short_name, e));
                            }
                            Err(p) => {
                                failures.push(format!(
                                    "{}: load panicked: {}",
                                    short_name,
                                    panic_message(p)
                                ));
                                break;
                            }
                        }
                    }

                    TestAction::Register {
                        name: reg_name,
                        module_name,
                    } => {
                        let target = resolve_module(&module_name, &named_modules, current_module);
                        if let Some(mh) = target {
                            let _ = runtime.register_module(reg_name, mh);
                        }
                    }

                    TestAction::AssertReturn(a) => {
                        let Some(mh) =
                            resolve_module(&a.module_name, &named_modules, current_module)
                        else {
                            failures.push(format!(
                                "{}: no module loaded for assert_return",
                                short_name
                            ));
                            continue;
                        };

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
                            Err(p) => {
                                failures.push(format!(
                                    "line {}: '{}' panicked: {}",
                                    a.line,
                                    a.func_name,
                                    panic_message(p)
                                ));
                                break;
                            }
                        };

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

                        for (i, (act, exp)) in actual.iter().zip(a.expected.iter()).enumerate() {
                            if !value_matches(act, exp) {
                                failures.push(format!(
                                    "line {}: '{}' result[{}] mismatch: got {:?}, expected {:?}",
                                    a.line, a.func_name, i, act, exp
                                ));
                            }
                        }
                    }

                    TestAction::AssertTrap(a) => {
                        let Some(mh) =
                            resolve_module(&a.module_name, &named_modules, current_module)
                        else {
                            failures
                                .push(format!("{}: no module loaded for assert_trap", short_name));
                            continue;
                        };

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
                            Err(p) => {
                                failures.push(format!(
                                    "line {}: '{}' expected trap '{}', got panic: {}",
                                    a.line,
                                    a.func_name,
                                    a.message,
                                    panic_message(p)
                                ));
                                break;
                            }
                        }
                    }

                    TestAction::AssertExhaustion(a) => {
                        let Some(mh) =
                            resolve_module(&a.module_name, &named_modules, current_module)
                        else {
                            failures.push(format!(
                                "{}: no module loaded for assert_exhaustion",
                                short_name
                            ));
                            continue;
                        };

                        let runtime_args: Vec<_> = a.args.iter().map(test_arg_to_value).collect();
                        let result = catch_unwind(AssertUnwindSafe(|| {
                            runtime.invoke(mh, &a.func_name, &runtime_args)
                        }));

                        match result {
                            Ok(Ok(_)) => {
                                failures.push(format!(
                                    "line {}: '{}' expected exhaustion '{}', got success",
                                    a.line, a.func_name, a.message
                                ));
                            }
                            Ok(Err(_)) => {} // Expected exhaustion - success
                            Err(p) => {
                                failures.push(format!(
                                    "line {}: '{}' expected exhaustion '{}', got panic: {}",
                                    a.line,
                                    a.func_name,
                                    a.message,
                                    panic_message(p)
                                ));
                                break;
                            }
                        }
                    }

                    TestAction::Invoke(a) => {
                        let Some(mh) =
                            resolve_module(&a.module_name, &named_modules, current_module)
                        else {
                            failures.push(format!("{}: no module loaded for invoke", short_name));
                            continue;
                        };

                        let runtime_args: Vec<_> = a.args.iter().map(test_arg_to_value).collect();
                        let result = catch_unwind(AssertUnwindSafe(|| {
                            runtime.invoke(mh, &a.func_name, &runtime_args)
                        }));

                        match result {
                            Ok(Ok(_)) => {}
                            Ok(Err(e)) => {
                                failures.push(format!(
                                    "line {}: invoke '{}' trapped: {}",
                                    a.line, a.func_name, e
                                ));
                            }
                            Err(p) => {
                                failures.push(format!(
                                    "line {}: invoke '{}' panicked: {}",
                                    a.line,
                                    a.func_name,
                                    panic_message(p)
                                ));
                                break;
                            }
                        }
                    }

                    TestAction::AssertModuleTrap {
                        wasm_bytes,
                        message,
                    } => {
                        let result =
                            catch_unwind(AssertUnwindSafe(|| runtime.load_module(&wasm_bytes)));
                        match result {
                            Ok(Ok(_)) => {
                                failures.push(format!(
                                    "{}: expected trap '{}', got Ok",
                                    short_name, message
                                ));
                            }
                            Ok(Err(_)) => {} // Expected trap
                            Err(p) => {
                                failures.push(format!(
                                    "{}: expected trap '{}', got PANIC: {}",
                                    short_name,
                                    message,
                                    panic_message(p)
                                ));
                                break;
                            }
                        }
                    }

                    TestAction::AssertMalformed {
                        wasm_bytes,
                        message,
                    } => {
                        let result = catch_unwind(AssertUnwindSafe(|| parse_module(&wasm_bytes)));
                        match result {
                            Ok(Ok(_)) => {
                                failures.push(format!(
                                    "{}: expected malformed error '{}', got Ok",
                                    short_name, message
                                ));
                            }
                            Ok(Err(Error::Malformed(_))) => {} // Expected
                            Ok(Err(e)) => {
                                failures.push(format!(
                                    "{}: expected malformed error '{}', got: {}",
                                    short_name, message, e
                                ));
                            }
                            Err(p) => {
                                failures.push(format!(
                                    "{}: expected malformed error '{}', got PANIC: {}",
                                    short_name,
                                    message,
                                    panic_message(p)
                                ));
                            }
                        }
                    }

                    TestAction::AssertInvalid {
                        wasm_bytes,
                        message,
                    } => {
                        let result = catch_unwind(AssertUnwindSafe(|| {
                            let module = parse_module(&wasm_bytes)?;
                            validate_module(&module)
                        }));
                        match result {
                            Ok(Ok(_)) => {
                                failures.push(format!(
                                    "{}: expected validation error '{}', got Ok",
                                    short_name, message
                                ));
                            }
                            Ok(Err(Error::Invalid(_))) => {} // Expected
                            Ok(Err(e)) => {
                                failures.push(format!(
                                    "{}: expected validation error '{}', got: {}",
                                    short_name, message, e
                                ));
                            }
                            Err(p) => {
                                failures.push(format!(
                                    "{}: expected validation error '{}', got PANIC: {}",
                                    short_name,
                                    message,
                                    panic_message(p)
                                ));
                            }
                        }
                    }
                }
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
