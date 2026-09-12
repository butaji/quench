# quench-wasm

`quench-wasm` is Quench's Wasm loader and spec-script adapter. It uses
third-party `wast` / `wasmparser` for syntax and validation, then hands
validated modules to `quench-runtime` for instantiation and execution in the
single stencil VM.

Keep format handling and directive scoring here; keep Wasm semantics in the
shared runtime.
