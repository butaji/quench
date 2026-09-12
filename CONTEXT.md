# Runtime map

- `crates/quench-runtime`: JavaScript reduction, canonical operations, compact execution, values, ownership and native execution.
- `crates/quench-node`: Node APIs, process effects, module registration and host integration.
- `crates/quench-wasm`: Wasm decoding/validation and adaptation to shared runtime execution.
- `crates/quench-test262`, `crates/quench-node-test`, `crates/quench-wasm-test`: test discovery, execution and classification.
- `quench-bench`: measurement fixtures and runners; never a production input.

[Architecture](docs/architecture.md) describes current mechanisms and shared contracts.
[Tasks](tasks/index.json) define remaining work.
[Docs](docs/README.md) index the implementation and measurement manuals.
