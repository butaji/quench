# WebAssembly boundary

`quench-wasm` owns decoding, validation and spec-script adaptation. The shared
runtime owns Wasm execution through the same values, heap, roots and dispatch as
JavaScript; the canonical Wasm suite uses this path. Third-party
decoding/validation is allowed; a separate guest executor is not.

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

The shared scalar API lowers validated module functions with a selected export
using `quench_wasm::Module::lower_shared(export)`, then executes through
`quench_runtime::Runtime::execute_wasm(&function, args)`. `WasmSignature` owns parameter
and result types. `WasmValue` carries i32/i64 values and exact f32/f64 IEEE bits,
preserving signed zero and NaN payloads. Constants, locals (with typed zero
initialization), calls, branches and select preserve all four scalar forms.
Arithmetic supports the full i32/i64 and f32/f64 operator families. Scalar
conversions cover integer width changes, trapping and saturating float-to-integer
conversion, signed/unsigned integer-to-float rounding, float promotion/demotion
and bit reinterpretation. Floating-point min/max propagate NaNs and order signed
zero; nearest rounds ties to even; abs, neg and copysign preserve payload bits.
References, multiple results and stateful module sections fail explicitly.
Blocks support zero or one scalar result without block parameters.

32-bit payloads use existing immediate Value slots. Exact 64-bit payloads use
an immutable leaf cell in the shared heap because they cannot fit the tagged
Value payload. Both forms use existing frames, locals, root maps and dispatch;
there is no second heap or executor. ScalarBits is the shared encoding authority
for constants and execution views. Raw 64-bit constants survive bytecode
serialization, and the same GC root lifecycle retains/releases scalar cells.

The i32 convenience boundaries `lower_shared_i32` and `execute_wasm_i32` delegate
to this typed path and reject incompatible entry signatures. `WasmI32Function`
is a compatibility name for the same artifact, not another program representation.
Integer division and remainder retain signed/unsigned rules; traps distinguish
division by zero, signed division overflow, unreachable and call-stack exhaustion.
Ordinary Wasm calls retain their callers and use the shared stack budget.
Execution starts fresh and invalidates previous host roots, like JavaScript
`Runtime::execute`. The WAST adapter applies the syntax normalization required
by pinned legacy-format inputs before it submits execution to the shared VM;
that parser compatibility step is not a second executor.

Run the pinned Wasm suite through the shared VM with:

```sh
WASM_FILE_TIMEOUT_MS=60000 cargo run --profile iteration -p quench-wasm-test --bin run -- --report target/iteration/wasm-shared.json crates/quench-wasm-test/testsuite
```

Score CoreMark on the shared VM against the pinned
[wasm-coremark-rs](https://github.com/wasmi-labs/wasm-coremark-rs) module
([task 91](../tasks/91.md) owns comparators and evidence):

```sh
cargo build --profile production -p quench-wasm-test --bin coremark
target/production/coremark path/to/coremark-minimal-mvp.wasm
```
