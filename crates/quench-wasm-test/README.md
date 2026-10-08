# quench-wasm-test

This crate discovers the pinned WebAssembly `.wast` files, including proposals,
and scores directives through `quench-wasm`. Totals count directives, not files;
there is no skip list.

The canonical `run` target executes every file in an isolated worker on the
shared VM and records per-directive outcomes. Commands and report contracts are owned by
[docs/README.md](../../docs/README.md#shared-wasm-scopes). Current migration gaps
and completion evidence live in [task 23](../../tasks/23.md).
