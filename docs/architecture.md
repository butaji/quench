# Architecture

**Goal:** 100% of the pinned test262 revision with zero failures and zero
skips, across script, module, async, and negative tests, using the smallest
practical Rust core. Test262 runs are the sole authority for conformance.

**Conformance-first gate:** Until a complete configured Test262 run reaches
zero failures and zero skips, only minimal targeted fixes for observed
conformance failures may change the runtime. Refactors, migrations, new
architecture, new abstractions, and performance or execution-model work are
deferred. Once 100% is reached, each such change must preserve it with a fresh
complete passing run before it can advance.

**Shape:** OXC parser + type-fact sidecar + lowering + interpreter-first IR +
an optional self-hosted JS builtins layer. The existing tree walker remains the
semantic reference while the IR is introduced incrementally:

1. **OXC parse** — `oxc` parses JS source into a spec-compliant AST
   (~20k LOC of parser logic we don't write).
2. **Type facts** — an external TypeScript-compatible checker consumes the
   resolved source and declaration graph. It provides a versioned sidecar of
   facts for TS and JS; facts are optimization hints, never runtime semantics.
3. **Lower** — `lower/` converts oxc AST nodes into our simpler internal
   AST (`ast.rs`), stripping arena lifetimes while preserving sidecar identity.
4. **IR** — a compact interpreter-oriented representation for constants,
   locals, control flow, calls, property operations, throws, and suspension
   points. Complex behavior continues to call canonical runtime operations.
5. **Interpret** — the current AST evaluator remains the reference path while
   an IR interpreter is added and differentially tested. The IR becomes the
   default only after semantic parity is demonstrated.

This is the fastest path to 100% conformance with minimum LOC because:

- **No hand-written parser.** OXC is spec-compliant and covers full
  ECMAScript + TypeScript + JSX. We save ~20k LOC and get correctness
  for free.
- **Interpreter-first IR.** The IR is introduced as a shared semantic boundary
  and a way to measure execution costs, not as a speculative optimizer. It
  must not duplicate abstract operations or make conformance depend on a large
  compiler rewrite.
- **Spec ops in one place.** `eval/ops.rs` owns every canonical
  abstract operation (`ToPrimitive`, `ToObject`, `IteratorNext`, …),
  exposed to JS as `__ops__`. Every eval node and every builtin routes
  through them — no duplicate implementations.
- **Selected builtins can be self-hosted in JS.** The embedded layer is loaded
  only when a host calls `bootstrap_js_builtins`; `Context::new` initializes
  Rust builtins and `__ops__` but does not load it. JS migration remains a
  direction, not a statement that all builtins are already JS-owned.

- **Builtin ownership:** Rust registration is the normal initialization path.
  A host may then load the optional self-hosted layer and use it to replace
  selected observable algorithms. Migration work and intentional direct
  bindings are tracked in `tasks/builtin-migration.md` and
  `tasks/builtin-direct-bindings.txt`.
- **Builtin ownership rule:** every algorithm, public method, constructor
  wiring, and property descriptor that can be authored in `builtins/*.js`
  belongs there. Rust is reserved for interpreter/core operations and
  implementations requiring performance, native memory, crate-backed
  functionality, or engine integration such as timers, GC hooks,
  synchronization, and scheduling. Rust may also retain an implementation
  when the equivalent JS builtin would materially increase total maintained
  LOC; that exception must be recorded in `tasks/builtin-migration.md`.
  A one-line JS forwarding proxy is not considered a migration: it increases
  maintained LOC without moving an ECMAScript algorithm.
- **`__ops__` must remain aligned with canonical ops:** `SameValueZero` calls
  `same_value`, `HasProperty` implements `has_own`, `IsCallable` misses
  callable objects, and `DefineProp`/`SealObject`/`FreezeObject` duplicate
  descriptor logic in `builtins/object_static/descriptors.rs`.

## Phased delivery

Complexity is added only after the complete configured Test262 corpus is at
100%, and each later change must preserve that result with a complete rerun.
Phase 0 is the current evaluator and canonical operations with the pinned
test262 zero-failure, zero-skip gate. Phase 1 adds measurement,
rooted-handle, and isolate boundaries. Phase 2 adds the type-fact sidecar and
IR interpreter with differential parity. Phase 3 introduces shapes/slots and
array layouts only when profiles justify them. Phase 4 is the bounded
single-isolate MMTk spike. Phase 5 is Cranelift entry-guarded specialization.
Mid-function deoptimization and OSR are later work, not Phase 5 scope.

## Rust core

Smallest set the builtins cannot be written without.

```
src/
├── parser.rs        # oxc → internal AST
├── lower/           # AST lowering
├── ast.rs           # internal AST
├── interpreter.rs   # eval entry points
├── eval/
│   └── ops.rs       # canonical spec abstract ops, exposed as __ops__
├── env/             # lexical environments
├── value/           # Value, Object (one canonical property store), JsError
├── context/         # Context, Realm
└── builtins/
    ├── core/            # __ops__ wrapper (ops_wrapper.rs)
    ├── regex.rs         # regress-backed (crate)
    ├── date.rs          # chrono-backed (crate)
    ├── bigint.rs        # num-bigint-backed (crate)
    ├── json.rs          # serde_json-backed (crate)
    ├── uri.rs           # urlencoding-backed (crate)
    ├── array/, object/, string/, …  # per-type modules
    ├── error/, promise/, map/, …
    └── bootstrap.rs     # loads JS builtins from builtins/*.js
```

Remainder of `eval/` is eval nodes only — no spec-op re-implementations.

## JS builtins

36 `.js` files are present in `builtins/` (root of repo), while
`bootstrap_js_builtins` embeds and loads the subset listed in
`builtins/bootstrap.rs` when explicitly called.
`__ops__` is scaffolded in
`builtins/core/ops_wrapper.rs`. Order:

```
builtins/
├── Object.js, Array.js, Iterator.js, Symbol.js,
├── Number.js, Boolean.js, String.js, Math.js,
├── Map.js, Set.js, WeakMap.js, WeakSet.js,
├── Promise.js, Reflect.js, Proxy.js,
├── RegExp.js, Date.js, BigInt.js,
├── TypedArray.js, ArrayBuffer.js, DataView.js, Atomics.js,
├── AsyncIterator.js, AsyncGenerator.js, AsyncFunction.js,
├── DisposableStack.js, AsyncDisposableStack.js,
├── FinalizationRegistry.js, WeakRef.js,
└── Host integration remains Rust-owned where a JS proxy would add LOC.
```

Every `.prototype.*`, intrinsic iterator prototype, `Object.*`,
`Reflect.*`, `Promise.prototype.*`, etc. authored here. Embedded via
`include_str!`; parsed once per `Realm` by `bootstrap.rs`.

Migration rule: a public builtin algorithm belongs in this JS layer whenever
it can be expressed over `__ops__`. This includes observable coercion,
validation, ordering, iteration, constructor, prototype, and descriptor
behavior. Rust remains only for interpreter/core operations, canonical
`__ops__` primitives, storage and native-memory operations, performance-
sensitive work, crate-backed functionality, engine integration, and direct
bindings whose JS wrapper would be a one-line proxy or would increase total
maintained LOC. Each exception is recorded in `tasks/builtin-migration.md` or
`tasks/builtin-direct-bindings.txt`.

## `__ops__` — the only Rust↔JS bridge for spec ops

Frozen object exposed at realm init. Each property is a canonical spec
abstract op, implemented once in `eval/ops.rs` and bound as a
`NativeFunction`. JS destructures it at parse time (never user-visible):

```js
// builtins/Array.js (excerpt)
const { IsCallable, ToObject, ThrowTypeError } = __ops__;

Array.prototype.map = function (callback, thisArg) {
  const O = ToObject(this);
  const len = O.length >>> 0;
  if (!IsCallable(callback)) throw ThrowTypeError("not a function");
  const A = new Array(len);
  for (let k = 0; k < len; k++) if (k in O)
    A[k] = callback.call(thisArg, O[k], k, O);
  return A;
};
```

New op → add to `eval/ops.rs` with a failing test → expose on `__ops__` →
JS callsite. No second copy anywhere.

## Object model — one canonical semantic path

Ordinary objects use immutable shapes from property keys to compact slot
offsets and a slot vector. Dynamic objects use dictionary mode. Arrays use
dense and holey vectors, with dictionary fallback for sparse or exotic cases.
Eval nodes, builtins, and `__ops__` all route through one canonical property
operation path — no parallel lookup paths or shadow stores. Descriptor,
prototype, accessor, proxy, and indexed-property behavior stays on that path:
`defineProperty` defaults absent attributes to `false`, non-configurable
invariants are enforced (ValidateAndApply), and writes to
non-writable/non-extensible targets throw in strict mode.

## Execution and memory boundaries

Use concrete isolate-local heap, rooted-handle, execution-frame, and job-queue
boundaries. The current `Rc<RefCell<...>>` representation is a conformance
reference, not the future JIT heap ABI. A bounded MMTk spike must prove roots,
write barriers, weak edges/ephemerons, host handles, cleanup-job ordering, and
a native-code safepoint before a collector is selected.

The AST evaluator and IR interpreter share canonical values, objects,
environments, scheduler state, and abstract operations. Each isolate owns one
heap and OS thread; realms, pending jobs, thrown values, and host handles are
isolate-local. `quench-node` distributes work between isolates and serializes
cross-isolate messages.

The Rust host API is narrow and versioned: realm lifecycle, module
registration, host calls, opaque rooted handles, promise/job scheduling, and
metrics hooks. It does not expose object layouts or evaluator internals.
Exceptions model normal values, throws, returns, breaks, continues, and
suspension. Keep runtime helpers concrete; do not introduce generic strategy
parameters before a measured need exists.

## Crate-backed primitives stay in Rust

| Spec area      | Crate         | Rust file                |
|----------------|---------------|--------------------------|
| RegExp exec    | `regress`     | `builtins/regex.rs`      |
| Date math      | `chrono`      | `builtins/date.rs`        |
| BigInt         | `num-bigint`  | `builtins/bigint.rs`      |
| JSON parse/str | `serde_json`  | `builtins/json.rs`        |
| URI            | `url::percent_encoding` (+ `url` for resolution) | `builtins/uri.rs` |
| Parsing        | `oxc`         | `parser.rs`              |

Each exposes a tiny primitive; the surrounding `.prototype.*` is JS.
Hand-rolled copies — including `chrono_*` helpers that never import
`chrono` — are forbidden.

## Future considerations

| Target | Ref plan | Timeline |
|--------|----------|----------|
| NaN-boxed values, arenas, string interning | Future consideration | After measurements and conformance |
| MMTk collector spike | Active design gate | Before choosing the production collector |
| Moving or generational GC | Future consideration | After the collector spike succeeds |
| Multiple execution models | Future consideration | After IR interpreter parity |
| Cranelift entry-guarded tier | Future consideration | After IR parity and benchmarks justify it |
| Mid-function deopt / OSR | Future consideration | After entry-guarded specialization is measured |

Runner phase instrumentation and RSS profiling are Phase 1 work because
they shorten the conformance loop. They do not authorize performance claims
without reproducible benchmarks.

The conformance runner should use persistent workers, configurable bounded
parallelism, and immutable parsed/bootstrap caches. Mutable contexts, pending
jobs, thrown values, and realm state remain worker-local until reset hygiene is
proved by tests. Stage-level concurrency requires isolated result files and a
serialized merge. Optimize scheduling by expected failures cleared per hour,
not by stage number alone.

## Bootstrap order

`Context::new` builds the Rust realm (intrinsic prototypes +
`%ThrowTypeError%`), then `bootstrap.rs` evaluates `builtins/*.js` in
dependency order (see `bootstrap.rs` for current implementation):
`Object` → `Function` → `Error` → `Symbol` → `Number`/`Boolean`/`String`
→ `Array`/`Iterator` → `Map`/`Set`/`Weak*` → `Promise`/`JSON`/`Reflect`/
`Proxy`/`Math` → `RegExp`/`Date`/`BigInt`/`TypedArray`/… → URI.

## Workflow

Same `AGENTS.md` cycle for both languages: failing `#[test]` first (in
Rust, wrapping the JS via `Context::eval` if needed), watch it fail,
minimal fix in Rust core *or* `builtins/*.js` *or* `eval/ops.rs`,
verify, leave the test in.

## File / function limits — enforced

`.clippy.toml` + `.cargo/config.toml` (`-D warnings`) gate every build.
No file > 500 lines, no function > 40 lines, no function complexity >
10, no `#[allow(...)]`, no deferrals. Split any offender before adding
to it. Run `cargo clippy -p quench-runtime --all-targets` to see current offenders.
Split offenders before adding more code.
JS files have no enforced limit but should stay under 500 too — split
per builtin category.
