# quench-wasm

`quench-wasm` is Quench's Wasm frontend. It parses and validates modules with
third-party `wast` / `wasmparser`, and scores each wast directive. It does not
instantiate or execute Wasm; that belongs to `quench-runtime`.

This crate does not execute Wasm and does not depend on `quench-runtime`.
