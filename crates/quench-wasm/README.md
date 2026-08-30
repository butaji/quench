# quench-wasm

`quench-wasm` is Quench's Wasm frontend and spec-script adapter. It uses
third-party `wast` / `wasmparser` for syntax and validation, then delegates
instantiation and execution to `quench-runtime`.

Keep format handling and directive scoring here; keep Wasm semantics in the
shared runtime.
