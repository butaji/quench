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
- `cargo test -p quench-runtime-core --lib` (177 tests)
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
- after initializing the pinned upstream WebAssembly testsuite submodule,
  `cargo run -p quench-wasm-test --bin run` executes 67,124 directives with
  67,124 passed and 0 failed; this validates the current Wasm frontend/runner,
  while migration of its execution backend into `quench-runtime-core` remains
  an explicit gate above
- after that aggregate run, the Array stage was rerun against the extended
  callback/index/reduction/flattening methods and descriptor guards and reached
  1,165/3,081; the
  aggregate total above is intentionally left as the last complete-corpus
  measurement
- the Object stage was rerun after adding boxed primitive identity, prototype
  inheritance, constructor metadata, sparse-array holes, and `Object.assign`
  descriptor guards, integrity levels, and Object static collections and reached 904/3,411; the remaining failures are recorded as missing semantics,
  not a fallback to the legacy VM
- the String stage was rerun after restoring historical conversion behavior,
  array `toString`, ordinary-object prototypes, and boxed-string own properties
  and reached 299/1,223; the remaining failures include UTF-16 surrogate,
  Symbol, and unsupported-stencil cases
- the Function stage was rerun after restoring dynamic `Function` source
  compilation, strict early-error checks, callable metadata, and restricted
  property guards and reached 210/509; caller-stack propagation remains open
- the Number stage was rerun after restoring numeric constructor constants,
  static predicates/parsers, prototype metadata, and numeric string coercion
  and reached 199/340; BigInt, Realm, and constructor-reflection cases remain
- the stencil Number stage now reaches 340/340 after deriving numeric
  predicates from one Rust macro, sharing exact number formatting helpers,
  preserving error prototypes, validating constructor/radix behavior, and
  retaining primitive `prototype` overrides for cross-realm construction
- the stencil NativeErrors stage now reaches 94/94 after deriving shared
  error prototypes, constructor identity, and non-enumerable message/cause
  properties, plus constructor-realm fallback; this stage is now fully green
- the stencil Math stage now reaches 325/327 after deriving unary and binary
  native wrappers, exact `sumPrecise` accumulation, binary16 rounding, and the
  complete intrinsic projection; the two remaining cases require generator /
  iterator lowering in the stencil compiler
- the stencil String stage now reaches 424/1,223 after routing constructor
  coercion and global binding projection through the shared stencil VM; the
  remaining failures are unsupported syntax, accessors, and Unicode details
- the stencil Object stage now reaches 2,170/3,411 after enforcing
  `Object.create` prototype validation, applying its property descriptors with
  correct defaults, and routing descriptor/accessor fields through the shared
  property model; the remaining failures are mostly arrays, proxies, and
  unsupported stencil features
- the stencil BigInt stage now reaches 71/77 after deriving one marker-based
  conversion path, native metadata, computed Symbol keys, wrapper prototypes,
  and radix formatting; remaining failures are numeric edge errors and
  prototype/accessor details
- the stencil Boolean stage now reaches 51/51 after projecting the constructor
  onto `globalThis` and carrying strict-mode deletion through generated
  bytecode; this stage is fully green
- the stencil Error stage now reaches 79/93 after adding shared `Error.isError`
  detection, preserving constructor-call prototypes, and modeling the shared
  `Error.prototype.stack` accessor contract; proxy/realm and accessor-descriptor
  edge cases remain open
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
