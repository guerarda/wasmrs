# Fuzz regressions

Save raw fuzz inputs here with a descriptive `.wasm` filename. They do not
need to be valid WebAssembly. Subdirectories are supported; each file's
relative path becomes its test name.

The runner decodes each input and, if decoding succeeds, validates the module.
Returned errors are allowed; panics fail the test. This checks for crashes,
not a particular decoding or validation result. Keep focused unit tests for
expected errors and behavior. Ordinary test runs do not impose the fuzzer's
memory/time limits or enable its sanitizers.

```sh
cargo test --test regression_runner
cargo test --test regression_runner -- --list
cargo test --test regression_runner invalid-name-length
```

These tests also run as part of `cargo test`. Only `.wasm` files are discovered,
so documentation files are ignored.
