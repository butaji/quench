# VM core migration

`quench-runtime` owns the migrated VM core. The complete source implementation
(bytecode compiler, dynamic JIT, copy-and-patch stencils, numeric-region
planner, raw values, builtins, and tests) is linked as the private
`quench-runtime-core` library in `crates/quench-runtime/core/`. It has no binary
target or executable entry point. The public runtime surface is
`quench_runtime::vm_core`.

The source implementation was copied as-is, with only the package/library
boundary and internal symbol prefix renamed to Quench-native names. Its AOT
build script compiles the copied stencil catalog as part of the runtime build.
`quench-node` remains the Node API compatibility host; it invokes the runtime
core for the V8V7 driver through `QUENCH_USE_NATIVE_CORE=1` and retains the
compatibility path for Node surface tests.

The 430 task documents remain under `crates/quench-runtime/tasks/` as the
lossless migration ledger.

Verification completed:

- `cargo check -p quench-runtime`
- `cargo test -p quench-runtime-core --lib` (161 tests)
- production `quench-node` build and Node smoke test
- canonical V8V7 exact driver, all eight fixtures valid

The matched one-second/32-run V8V7 measurements (both thin-LTO builds) were
source VM geomean 2,512.49 and runtime-owned migrated-core geomean 2,524.36. Raw output is
recorded in `reports/v8v7-source-exact-20260911.log` and
`reports/v8v7-migrated-core-exact-20260911.log`; the small difference is normal
wall-clock benchmark variance.
