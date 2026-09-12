# VM core migration

`quench-runtime` owns the migrated VM core. The complete source implementation
(bytecode compiler, dynamic JIT, copy-and-patch stencils, numeric-region
planner, raw values, builtins, and tests) is linked as the private
`quench-runtime-core` library in `crates/quench-runtime/core/`. It has no binary
target or executable entry point. The public runtime surface is
`quench_runtime::vm_core`.

## One-VM invariant

`quench-runtime-core` is the authoritative JavaScript execution engine and is
selected for every file-backed `quench-node` run. The legacy
`quench-runtime/src/vm` implementation is migration residue: it must not be
selected as a production fallback and is scheduled for removal after its Node
host and WebAssembly boundaries are lowered into this core.

The compatibility layers are intentionally preserved. `quench-node` owns Node
compatibility APIs (argv, output, modules, timers, and exit handling), while
`quench-runtime` owns JavaScript-facing semantic helpers. `quench-wasm` owns
Wasm format loading and spec-suite adaptation. These layers pass host data into
the core; they do not create a second VM.

WebAssembly must enter this same core through a lowering-only frontend. A
Wasm-specific interpreter, MIR executor, or native dispatch loop is not an
acceptable implementation path.

The Wasm lowering is still an active migration boundary: the existing
`quench-runtime::instance` API remains compiled for compatibility tests until
its typed module lowering is hosted by `quench-runtime-core`. It is not used by
the JavaScript file runner, and this temporary boundary must be removed before
the migration is declared complete.

The source implementation was copied as-is, with only the package/library
boundary and internal symbol prefix renamed to Quench-native names. Its AOT
build script compiles the copied stencil catalog as part of the runtime build.
`quench-node` remains the Node API compatibility host; file-backed execution
now invokes the runtime core directly. The compatibility host path remains
only for eval-mode and host-surface migration tests and is not a file-execution
fallback.

The stencil compiler now lowers common object/array destructuring, arrow
functions, array `for-of`, and a single spread-only call through the core.
Small `assert`, `buffer`, `util`, and `path` Node-compatible surfaces are also
VM-owned, including Blob validation, Buffer allocation, signal exit-code
conversion, and POSIX path operations. The core owns FIFO `process.nextTick`,
`setTimeout`/`clearTimeout`, and `setImmediate` queues for file runs. These are
incremental language slices, not a claim that the Node surface is complete;
unsupported syntax remains an explicit migration error until its semantics are
lowered.

The 430 task documents remain under `crates/quench-runtime/tasks/` as the
lossless migration ledger.

## Compatibility gates

The legacy executor is removable only when each boundary below has a core
implementation and its existing oracle suite is green. This keeps the host and
semantic layers stable while the execution core changes underneath them.

| Boundary | Current entry point | Core migration gate |
| --- | --- | --- |
| Plain JavaScript files | `vm_core::run_source_with_argv_and_output_status` | stencil execution, output, argv, and exit status match the Node oracle |
| CJS/Node modules | `vm_core::run_source_with_argv_and_output_status` + core `require` | `require`, module cache/identity, timers, and host effects run in the same core context |
| `node -e` / eval | `eval_script_with_exec_argv` | eval and file execution share one core context contract |
| Test262 harness | `quench-test262::runtime_host` (compatibility) or `QUENCH_TEST262_ENGINE=stencil` | all realm, descriptor, identity, ordering, and error checks pass through the selected core |
| WebAssembly | `quench-runtime::instance` | Wasm lowering, typed calls, memory/tables, traps, exceptions, and imports execute in the core |

Until every row is green, deleting `crates/quench-runtime/src/vm` or the Wasm
interpreter would be a compatibility regression, not a migration.

Verification completed:

- `cargo check -p quench-runtime`
- `cargo test -p quench-runtime-core --lib` (175 tests)
- `cargo test -p quench-node --lib` (18 tests)
- `cargo test -p quench-wasm --lib` (16 tests)
- production `quench-node` build and a core-backed Node smoke test
- core-backed `tests/node-compat/stage-2235` (2/2 fixtures)
- core-backed `tests/node-compat/stage-2650/buffer-tostring-range.js`
- core-backed file timer smoke (`sync` before `timer`)
- core-backed `process.nextTick` ordering smoke (`sync`, `tick`, then `timer`)
- full core-backed `tests/node-compat` audit currently reports 16/863; the
  remaining failures identify host-module and syntax migration work still
  required before the compatibility-host path can be removed
- full compatibility-host Test262 stage sweep (stages 0..113) ran 51,653/51,900
  fixtures successfully; 247 failures remain in Atomics, TypedArray, and one
  Intl fallback case
- full stencil-host Test262 stage sweep (stages 0..113), invoked with
  `QUENCH_TEST262_ENGINE=stencil`, ran 4,240/51,900 fixtures successfully;
  47,660 failures remain as explicit missing-stencil or missing-built-in
  diagnostics, proving the corpus reaches the new VM without silently falling
  back
- after that aggregate run, the Array stage was rerun against the extended
  callback/index/reduction/flattening methods and descriptor guards and reached
  1,165/3,081; the
  aggregate total above is intentionally left as the last complete-corpus
  measurement
- the Object stage was rerun after adding boxed primitive identity, prototype
  inheritance, constructor metadata, sparse-array holes, and `Object.assign`
  descriptor guards and reached 514/3,411; the remaining failures are recorded as missing semantics,
  not a fallback to the legacy VM
- the String stage was rerun after restoring historical conversion behavior,
  array `toString`, ordinary-object prototypes, and boxed-string own properties
  and reached 287/1,223; the remaining failures include UTF-16 surrogate,
  Symbol, and unsupported-stencil cases
- the Function stage was rerun after restoring dynamic `Function` source
  compilation, strict early-error checks, callable metadata, and restricted
  property guards and reached 210/509; caller-stack propagation remains open
- historical commits advertising “100% Test262” used `test262/skip.rs` to skip
  entire built-in families (including Object, Array, String, TypedArray, and
  Promise); those results are not equivalent to executing the full corpus
- canonical V8V7 exact driver, all eight fixtures valid
- focused stencil probes now cover `Array.from`/`Array.of`/`Array.isArray`,
  `Object.preventExtensions` + `defineProperty`, bound function calls, and
  native error-constructor identity; the full stencil sweep remains the
  authoritative migration gate and is still not green

The matched one-second/32-run V8V7 measurements (both thin-LTO builds) were
source VM geomean 2,512.49 and runtime-owned migrated-core geomean 2,524.36. Raw output is
recorded in `reports/v8v7-source-exact-20260911.log` and
`reports/v8v7-migrated-core-exact-20260911.log`; the small difference is normal
wall-clock benchmark variance.
