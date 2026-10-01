# WebAssembly boundary

`quench-wasm` owns decoding, validation and spec-script adaptation;
`quench-runtime` owns execution, memory, tables, exceptions and host calls.
Third-party decoding/validation is allowed; a separate guest executor is not.

Use the shared typed register machinery and preserve distinct Wasm traps,
tagged exceptions and JavaScript throw behavior at their shared boundaries.
Instance memory/table lifetime and reference roots must follow actual runtime
ownership; do not substitute a scratch arena for escaping state.

The spectest adapter and Node's `WebAssembly` API are different host surfaces.
Measure every directive in the vendored specification suite, including proposals:
validity, linking, instantiation, values, traps, exhaustion and host effects.
No fixture recognizers or skip-list-based claims. See
[the runner](../crates/quench-wasm-test/README.md) and [repository rules](../AGENTS.md).
These are requirements, not an assertion of complete conformance.

The initial shared execution API lowers standalone i32 function exports from a
validated `quench_wasm::Module` with `lower_shared_i32(export)`, then executes
with `rqj::Runtime::execute_wasm_i32(&function, args)`. It supports constants,
locals (including zero initialization), drop/nop, and all i32 numeric operators.
Integer division and remainder preserve signed/unsigned rules and report typed
`WasmTrap` values for division by zero and signed division overflow. Unsupported operators and stateful module sections are rejected.
Decoding and validation stay in `quench-wasm`; lowering uses the next runtime's
`Engine`, residual instructions, root maps, activation frames and dispatch loop.
Like JavaScript `Runtime::execute`, execution starts fresh and invalidates
previous host roots. This API is an initial migration slice; the legacy spec
harness still owns the remaining Wasm coverage.

Run the focused shared execution regressions with:

```sh
cargo test -p quench-wasm --lib shared:: -- --nocapture
```
