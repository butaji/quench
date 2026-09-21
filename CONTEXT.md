# Runtime map

- `crates/quench-runtime`: JavaScript semantics and interpreter execution.
- `crates/quench-node`: Node-compatible APIs and host effects.
- `crates/quench-wasm`: Wasm decoding, validation, and host adaptation.
- `crates/quench-test262`, `crates/quench-node-test`, and
  `crates/quench-wasm-test`: conformance discovery and execution.
- `quench-bench`: measurement fixtures and runners; never a production input.

[The task queue](tasks/index.json) defines the rewrite order and gates.
[Documentation](docs/README.md) indexes stable commands and contracts.
