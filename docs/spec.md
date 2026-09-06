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
