# Architecture

**Goal:** 100% of the in-scope test262 suite with the smallest practical Rust
core. Test262 runs are the sole authority for conformance.

**Shape:** OXC parser + lowering + interpreter-first IR + an optional
self-hosted JS builtins layer. The existing tree walker remains the semantic
reference while the IR is introduced incrementally:

1. **OXC parse** — `oxc` parses JS source into a spec-compliant AST
   (~20k LOC of parser logic we don't write).
2. **Lower** — `lower/` converts oxc AST nodes into our simpler internal
   AST (`ast.rs`), stripping arena lifetimes and collapsing nodes the
   walker doesn't need to distinguish.
3. **IR** — a compact interpreter-oriented representation for constants,
   locals, control flow, calls, property operations, throws, and suspension
   points. Complex behavior continues to call canonical runtime operations.
4. **Interpret** — the current AST evaluator remains the reference path while
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

## Object model — one canonical store

`Object` has a single own-property store (R5 target:
`IndexMap<Key, Prop>`, `Key::Sym` carrying unique symbol identity).
eval nodes, builtins, and `__ops__` all route through it — no parallel
lookup paths, no per-callsite prototype walks, no shadow stores (the
dead `props`/`VTable` layer was removed in R4). Descriptor semantics
follow the spec: `defineProperty` defaults absent attributes to `false`,
non-configurable invariants are enforced (ValidateAndApply), and writes
to non-writable/non-extensible targets throw in strict mode.

## Execution and memory boundaries

Phase 3 may introduce explicit `Heap`, object handles, execution frames, and
root accounting where they simplify ownership, reset hygiene, or allocation
measurement. The first goal is a clear boundary around the existing
`Rc<RefCell<...>>` representation; a garbage collector is not required.

The AST evaluator and IR interpreter share canonical values, objects,
environments, scheduler state, and abstract operations. Mutable realms,
pending jobs, thrown values, and host state remain worker-local.

The runtime is intended to make these policies configurable through one
composition boundary:

```text
Runtime<Heap, Collector, Allocator, Frames, Executor, Exceptions, Environments>
```

Each parameter supplies a strategy for one concern. `Heap` owns identity and
storage; `Allocator` creates heap values; `Collector` accounts for roots and
reclaims storage; `Frames` owns call locals and suspension state; `Executor`
runs the AST or IR; `Exceptions` carries JavaScript completion/control-flow
states; and `Environments` owns lexical, global, module, and closure bindings.
The first implementation may adapt the existing reference-counted storage
and use a no-op collector. The interfaces must not expose that representation
to the evaluator.

`Exceptions` should model completion states, not only thrown errors: normal
values, throws, returns, breaks, continues, and suspension. Heap and allocator
strategies should share stable, copyable handles so later storage strategies
do not require an evaluator rewrite. Keep generic composition at the runtime
boundary; internal helpers should remain concrete where that preserves the
minimum-LOC goal.

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
| Moving or generational GC | Future consideration | After heap handles and root accounting |
| Multiple execution models | Future consideration | After IR interpreter parity |
| Cranelift JIT/AOT, inline caches, deoptimization | Future consideration | After IR and profiling justify it |

Runner phase instrumentation and RSS profiling are active Phase 3 work because
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
