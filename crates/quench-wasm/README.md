# quench-wasm

`quench-wasm` is Quench's WebAssembly execution boundary. It compiles and
instantiates Wasm modules with Wasmi, invokes typed numeric exports through an
owned store, and runs WAST directives through the same Wasmi conformance
implementation used by the test runner.

JavaScript host adapters can use `Engine::javascript()` to evaluate source
through `quench-runtime`; the two machines remain separate and the bridge is
explicit at the host boundary.
