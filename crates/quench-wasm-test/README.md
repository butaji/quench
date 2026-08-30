# quench-wasm-test

This crate runs the vendored WebAssembly specification testsuite. Discover all
`.wast` files, including proposals, and score every directive through
`quench-wasm` and the shared runtime. There is no skip list and totals are
directive counts, not file counts.
