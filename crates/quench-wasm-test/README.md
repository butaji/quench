# quench-wasm-test

This crate runs the upstream WebAssembly specification testsuite. The
testsuite is tracked as the `testsuite/` git submodule and is discovered by
`.wast` extension, including `proposals/`. There is no skip list.

`TestSuite::run_file` and `TestSuite::run_all` score **each wast directive**
through `quench-wasm`. Validator directives (`assert_malformed`,
`assert_invalid`) decide via parse then validate. Execute-class directives
fail as unimplemented until the runtime interprets Wasm.

Run the checked-out suite with:

```text
cargo run -p quench-wasm-test --bin run
```

An alternate suite directory can be supplied as the first argument. The process
exits non-zero while execute directives remain unimplemented. Printed totals
are directive counts, not file counts.
