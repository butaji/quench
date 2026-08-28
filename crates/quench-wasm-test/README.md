# quench-wasm-test

This crate runs the upstream WebAssembly specification testsuite. The
testsuite is tracked as the `testsuite/` git submodule and is discovered by
`.wast` extension. `TestSuite::run_file` and `TestSuite::run_all` delegate
execution to `quench-wasm` and return structured pass/fail reports.

Run the checked-out suite with:

```text
cargo run -p quench-wasm-test --bin run
```

An alternate suite directory can be supplied as the first argument.
