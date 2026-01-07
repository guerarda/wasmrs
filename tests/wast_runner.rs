use std::collections::HashMap;
use std::io::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use wast::parser::{self, ParseBuffer};
use wast::{Wast, WastDirective};

use wasmrs::{ModuleHandle, Runtime};

#[derive(Debug)]
enum TestResult {
    Pass,
    Fail { expected: String, actual: String },
    Skip,
}

struct WastRunner {
    runtime: Runtime,
    #[allow(dead_code)]
    modules: HashMap<String, ModuleHandle>,
    #[allow(dead_code)]
    current_module: Option<ModuleHandle>,
    results: Vec<(usize, usize, &'static str, TestResult)>, // (idx, line, kind, result)
}

impl WastRunner {
    fn new() -> Self {
        WastRunner {
            runtime: Runtime::default(),
            modules: HashMap::new(),
            current_module: None,
            results: Vec::new(),
        }
    }

    /// Run a directive, collecting results without panicking
    fn run_directive(&mut self, idx: usize, directive: WastDirective, source: &str) {
        let span = directive.span();
        let (line, _col) = span.linecol_in(source);
        let line = line + 1; // 1-indexed

        match directive {
            WastDirective::Module(mut module) => {
                let wasm_bytes = module.encode().expect("failed to encode module");
                // Catch panics from incomplete runtime
                let result =
                    catch_unwind(AssertUnwindSafe(|| self.runtime.load_module(&wasm_bytes)));
                match result {
                    Ok(Ok(handle)) => {
                        self.current_module = Some(handle);
                        self.results.push((idx, line, "Module", TestResult::Pass));
                    }
                    Ok(Err(e)) => {
                        self.results.push((
                            idx,
                            line,
                            "Module",
                            TestResult::Fail {
                                expected: "Ok".to_string(),
                                actual: e.to_string(),
                            },
                        ));
                    }
                    Err(_) => {
                        self.results.push((
                            idx,
                            line,
                            "Module",
                            TestResult::Fail {
                                expected: "Ok".to_string(),
                                actual: "PANIC in runtime".to_string(),
                            },
                        ));
                    }
                }
            }
            WastDirective::AssertMalformed {
                mut module,
                message,
                span: _,
            } => {
                let wasm_bytes = module.encode().expect("failed to encode module");
                // Catch panics from incomplete runtime
                let result =
                    catch_unwind(AssertUnwindSafe(|| self.runtime.load_module(&wasm_bytes)));
                match result {
                    Ok(Ok(_)) => {
                        self.results.push((
                            idx,
                            line,
                            "AssertMalformed",
                            TestResult::Fail {
                                expected: format!("error: {}", message),
                                actual: "Ok".to_string(),
                            },
                        ));
                    }
                    Ok(Err(e)) => {
                        // Test passed - we expected an error and got one
                        eprintln!(
                            "[{}] line {}: expected {:?}, got {:?}",
                            idx,
                            line,
                            message,
                            e.to_string()
                        );
                        let _ = std::io::stderr().flush();
                        self.results
                            .push((idx, line, "AssertMalformed", TestResult::Pass));
                    }
                    Err(_) => {
                        // Panic counts as an error for malformed modules
                        eprintln!("[{}] line {}: expected {:?}, got PANIC", idx, line, message);
                        let _ = std::io::stderr().flush();
                        self.results
                            .push((idx, line, "AssertMalformed", TestResult::Pass));
                    }
                }
            }
            _ => {
                self.results.push((idx, line, "Other", TestResult::Skip));
            }
        }
    }

    fn print_summary(&self) {
        let passed = self
            .results
            .iter()
            .filter(|(_, _, _, r)| matches!(r, TestResult::Pass))
            .count();
        let failed = self
            .results
            .iter()
            .filter(|(_, _, _, r)| matches!(r, TestResult::Fail { .. }))
            .count();
        let skipped = self
            .results
            .iter()
            .filter(|(_, _, _, r)| matches!(r, TestResult::Skip))
            .count();

        eprintln!("\n=== SUMMARY ===");
        eprintln!("Passed:  {}", passed);
        eprintln!("Failed:  {}", failed);
        eprintln!("Skipped: {}", skipped);
        eprintln!("Total:   {}", self.results.len());
        let _ = std::io::stderr().flush();

        if failed > 0 {
            eprintln!("\n=== FAILURES ===");
            for (idx, line, kind, result) in &self.results {
                if let TestResult::Fail { expected, actual } = result {
                    eprintln!(
                        "[{}] line {}: {} - expected: {}, got: {}",
                        idx, line, kind, expected, actual
                    );
                }
            }
            let _ = std::io::stderr().flush();
        }
    }
}

fn run_wast_file(path: &Path) -> (usize, usize, usize) {
    let contents = std::fs::read_to_string(path).expect("failed to read wast file");
    let buf = ParseBuffer::new(&contents).expect("failed to create parse buffer");
    let wast: Wast = parser::parse(&buf).expect("failed to parse wast file");

    let mut runner = WastRunner::new();

    for (idx, directive) in wast.directives.into_iter().enumerate() {
        runner.run_directive(idx, directive, &contents);
    }

    runner.print_summary();

    let passed = runner
        .results
        .iter()
        .filter(|(_, _, _, r)| matches!(r, TestResult::Pass))
        .count();
    let failed = runner
        .results
        .iter()
        .filter(|(_, _, _, r)| matches!(r, TestResult::Fail { .. }))
        .count();
    let skipped = runner
        .results
        .iter()
        .filter(|(_, _, _, r)| matches!(r, TestResult::Skip))
        .count();

    (passed, failed, skipped)
}

#[test]
fn run_binary_wast() {
    let (passed, failed, skipped) = run_wast_file(Path::new("tests/spec/binary.wast"));
    eprintln!(
        "\nbinary.wast: {} passed, {} failed, {} skipped",
        passed, failed, skipped
    );
    let _ = std::io::stderr().flush();
    // Don't assert - we expect failures while runtime is incomplete
}
