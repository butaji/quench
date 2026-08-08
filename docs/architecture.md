# Architecture

**North star (ADR 0003):** two parallel planes — an **ECMAScript
execution plane** (always correct, dynamic, test262-compatible; the only
plane that runs code) and a **TypeScript semantic plane** (persistent,
reflectable `TypeGraph` metadata used for optimization and opt-in runtime
validation). The planes meet only through **guards**, never by trusting
annotations. `TypeId` (semantic type), `ShapeId` (runtime layout), and
`Rep` (machine representation) are never merged. Long-term pipeline:
`OXC → binder/module graph → typed HIR (temporary) → {TypeGraph
(persistent, mmap-able), compact bytecode (canonical exec format)} →
interpreter + guards → typed specialization → baseline/JIT`. Crate
boundaries, heap design (tagged `Value`, `HeapRef(u32)`, shapes), frozen
module images, runtime type modes, and JSX backends:
`docs/adr/0003-two-plane-architecture.md`.

**Goal:** a pure JavaScript engine in `quench-runtime` and an isolated
`quench-test262` conformance runner, converging on 100% ECMA-262 test262
with minimum memory/RSS, V8-grade performance, and no undocumented skips
(`GOAL.md`). The engine pipeline is
explicit: `OXC AST -> Quench IR -> interpreter`.

`quench-runtime` owns parsing, compact IR, execution, values, environments,
and builtins. `quench-test262` owns frontmatter, harness loading, stage
selection, isolation, metrics, and reporting. The runner communicates with
the engine only through its host execution interface.

**Shape (target, R0/R1 — not yet landed):** small Rust core + self-hosted
JS builtins. Today **all builtins are Rust**
(`crates/quench-runtime/src/builtins/*.rs`); there are no JS builtins yet.
The governing rule (ADR 0001): everything that can be done in JS must be
done in JS, keeping the Rust core at the minimum possible size. JS is ~1/3
the LOC of equivalent Rust and easier to keep spec-faithful.

**Boundaries:** the runtime stays generic over replaceable
implementations so subsystems evolve independently:

```rust
Runtime<Heap, Collector, Allocator, Frames, Executor, Exceptions, Environments>
```

Each parameter is a trait boundary with one production implementation;
swapping an implementation (e.g. arena `Allocator`, NaN-boxed `Heap`)
must not touch the others.

## Quench IR storage contract

`QuenchIr` is the owned post-frontend representation. Legacy parser helpers
may still return the lowered runtime `Program` while callers migrate to the
`*_ir` entry points.

**Decision (ADR 0002, scoped by ADR 0003):** the IR is a compact,
index-addressed **instruction IR** executed by a pc-based IR interpreter —
not an AST walker, and not a bytecode VM *yet* (no opcode encoding, no
register allocation at this milestone; compact accumulator+register
bytecode is the north-star encoding per ADR 0003). `IrProgram`
owns arenas (`funcs`, `consts`, `atoms`, legacy AST side tables); each
`IrFunction` holds a `Box<[Instr]>` of high-level ops with `u32` operands
plus a try-handler table. An `src/ir/compile/` pass lowers `crate::ast` →
IR; uncompiled constructs use `LegacyStmt`/`LegacyExpr` escape hatches into
owned AST subtrees so coverage grows stage-gated. Generators/async stay on
the AST replay engine until resumable IR frames land as a separate measured
step. TS/JSX/TSX support grows through `lower/` + the compiler only — the
IR and executor are frontend-agnostic. Details and milestones:
`docs/adr/0002-compact-ir-interpreter.md`.

The target representation is an owned, compact, index-addressed IR:

- OXC owns parse-time nodes only. `parser.rs` lowers while the OXC allocator
  is live, then drops it before interpretation.
- The IR owns one contiguous instruction/node arena and refers to children
  with `u32` indices. No IR node stores an OXC reference or `Rc` edge.
- Identifiers and property names use interned atom IDs; repeated source text
  is stored once. Constants use a per-program pool and preserve source spans
  only when diagnostics require them.
- Hot interpreter dispatch uses dense tags plus parallel payload arrays;
  cold metadata (spans, source names, debug information) stays out of the
  hot node path. This keeps RSS predictable while retaining cache locality.
- The first migration target is statement/expression lowering behind an
  `IrProgram` conversion API. Each conversion keeps the existing interpreter
  behavior pinned by a reproducer test before storage changes land.
- The first storage step is now landed: top-level statements are packed into
  an owned boxed slice and the interpreter walks that slice directly.
  Recursive statement and expression nodes remain legacy storage until an
  indexed arena conversion has measured a real RSS benefit.

The design deliberately avoids a general graph allocator, per-node trait
objects, and speculative JIT metadata. Those increase RSS and indirection
before test262 measurements prove they help.

## Rust core

The workspace boundary is:

```text
crates/quench-runtime/   pure engine: parser, Quench IR, interpreter
crates/quench-test262/   conformance client: host dispatch and runner policy
```

`quench-test262` may depend on the public runtime host contract, but
`quench-runtime` does not depend on runner policy.

Smallest set the builtins cannot be written without.

```
src/  (current layout — see the repo tree)
├── parser.rs        # oxc → internal AST
├── lower/           # AST lowering
├── ast.rs           # internal AST
├── interpreter.rs   # eval entry points
├── eval/
│   └── ops.rs       # canonical spec abstract ops, to be exposed as `__ops__`
├── env/             # lexical environments
├── value/           # Value, Object (one canonical property store), JsError
├── context/         # Context, Realm
└── builtins/        # all builtins, currently pure Rust
```

Target additions under R0/R1 (do not exist yet): `builtins/core/` reduced
to crate-backed primitives only, plus `builtins/bootstrap.rs` parsing and
evaluating `builtins/*.js` at realm init.

Remainder of `eval/` is eval nodes only — no spec-op re-implementations.

## JS builtins *(target — R0, none exist yet)*

```
crates/quench-runtime/builtins/
├── _intrinsics.js   # __ops__ destructure (resolved at parse time)
├── Object.js, Function.js, Error.js, Symbol.js,
├── Number.js, Boolean.js, String.js, Math.js,
├── Array.js, Iterator.js,
├── Map.js, Set.js, WeakMap.js, WeakSet.js,
├── Promise.js, JSON.js, Reflect.js, Proxy.js,
├── RegExp.js, Date.js, BigInt.js,
├── TypedArray.js, ArrayBuffer.js, DataView.js, Atomics.js,
└── decodeURI.js, encodeURI.js
```

All `*.prototype.*`, intrinsic iterator prototypes, `Object.*`,
`Reflect.*`, `Promise.prototype.*`, etc. are authored here. Pure spec
algorithms on top of `__ops__`. Embedded via `include_str!`; parsed once
per `Realm` by `builtins/bootstrap.rs`.

## `__ops__` — the only Rust↔JS bridge for spec ops

(The current Rust scaffold is registered as `%ops%`; it will be renamed
to `__ops__` when R1 lands.)

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

New op → add to `eval/ops.rs` → expose on `__ops__` →
JS callsite. No second copy anywhere. Correctness is gated by the test262
stage run; a unit test is added only if a bug is later found here
(regression guard).

## Object model — one canonical store

`Object` has a single own-property store (R5 target:
`IndexMap<Key, Prop>`, `Key::Sym` carrying unique symbol identity).
eval nodes, builtins, and `__ops__` all route through it — no parallel
lookup paths, no per-callsite prototype walks, no shadow stores (the
dead `props`/`VTable` layer was removed in R4). Descriptor semantics
follow the spec: `defineProperty` defaults absent attributes to `false`,
non-configurable invariants are enforced (ValidateAndApply), and writes
to non-writable/non-extensible targets throw in strict mode.

## Crate-backed primitives stay in Rust

| Spec area      | Crate         | Rust file                |
|----------------|---------------|--------------------------|
| RegExp exec    | `regress`     | `builtins/core/regex.rs` |
| Date math      | `chrono`      | `builtins/core/date.rs`  |
| BigInt         | `num-bigint`  | `builtins/core/bigint.rs`|
| JSON parse/str | `serde_json`  | `builtins/core/json.rs`  |
| URI            | `urlencoding` | `builtins/core/uri.rs`    |
| Parsing        | `oxc`         | `parser.rs`              |

Each exposes a tiny primitive; the surrounding `.prototype.*` is JS.
Hand-rolled copies — including `chrono_*` helpers that never import
`chrono` — are forbidden.

## Value representation — NaN boxing *(R11 target)*

*(Subsumed long-term by ADR 0003's heap design: 64-bit tagged `Value`,
`HeapRef(u32)` instead of raw pointers, shape-based objects, generational
GC. NaN boxing is the near-term step toward the tagged `Value`.)*

JS values (`JsValue`) fit in a single `u64` via NaN boxing — the same
technique used by QuickJS, JSC, V8, and Boa.

```
u64 bits:
  [sign:1][exp:11][mantissa:52]
NaN payload (all exp=1, mantissa≠0 for quiet NaN):
  [1][11111111111][tag:16][payload:36]
  tag=0xFFFF  →  Integer(i32)  (mantissa low 32 bits)
  tag=0x0000  →  Pointer to heap Value
  tag=0x0001  →  Undefined
  tag=0x0002  →  Null
  tag=0x0003  →  Boolean
  tag=0x0004  →  BigInt
Special doubles pass through untouched (infinity, -0.0).
```

**Do NOT start this until R0 is complete.** The Value representation
change touches every call site; R0 gives us a clean JS-layer boundary to
verify correctness afterward. Boa did the same: NaN boxing was added as a
feature-flagged feature alongside the existing enum (`jsvalue-enum`).

Rust `unsafe` is confined to the `value/` module. No unsafe leaks into
`eval/` or `builtins/`.

## Memory — bumpalo arena allocation *(R10 target)*

`bumpalo` is the standard arena allocator for Rust
JS engines. It provides fast short-lived allocations without per-object
overhead:

- **Parsing** (oxc → internal AST): nodes die after lowering — arena
  perfect fit.
- **Eval frames** (per-expression allocations): short-lived, high volume.
- **No Drop calls** on freed objects — `bumpalo` does not run destructors.
  Use `bumpalo::boxed::Box` for types that need Drop.

```toml
# Cargo.toml
bumpalo = "3"
```

See <https://nickb.dev/blog/the-serde-optimization-gauntlet-wasm-and-arenas/>
for benchmarks. `bumpalo` is battle-tested (Boa, many WASM engines);
`bump-scope` benches ~2x faster but is less proven. Land `bumpalo` first,
optimize later.

Add `docs/DEPENDENCIES.md` row in the same diff as the first arena use.

## Strings — atom table interning *(R12 target)*

Identifier strings, keywords, property names, and spec-intrinsic strings are
interned: stored once, compared by pointer equality. The `fnv` crate
provides a high-quality FnvHashMap for the atom table.

```toml
# Cargo.toml
fnv = "2"
```

- `string_interner` crate is an alternative (handles arbitrary strings,
  not just identifiers).
- `rustc-hash` (FxHashMap) is the fastest but lower-quality hash — use
  only for hot-path property lookups where collision risk is acceptable.
- `stringcache` crate is unmaintained; do not use.

String interning is especially valuable during parsing (oxc produces
interned identifier strings) and in `eval/ops.rs` (all spec op names are
static atoms). A single atom table means `ToPropertyKey` on an identifier
string is O(1) pointer comparison, not O(n) string compare.

Add `docs/DEPENDENCIES.md` row in the same diff.

## Profiling on macOS

Throughput-sensitive workloads (test262 runner) benefit from profiling.
Tools confirmed working on macOS (Darwin):

### cargo-flamegraph

```bash
brew install dtrace         # required on macOS
cargo install cargo-flamegraph
cargo flamegraph --bin run-test -- tests/test262/.../test.js
```

On macOS, `cargo-flamegraph` uses `xctrace` (Apple Instruments CLI) under
the hood. Output is a `.perfetto` file viewable in Chrome
(`chrome://tracing`) or `xctrace` viewer.

Reference: <https://docs.rs/crate/flamegraph/latest>

### samply (better macOS support)

```bash
cargo install samply
samply record -- ./target/release/quench test.js
samply record -- samples "cargo test -p quench-runtime --test test262"
```

`samply` uses the macOS `timed` backend (superior to `dtrace` on Darwin).

Reference: <https://github.com/mstange/samply>

### xctrace (Apple Instruments CLI)

Apple's native profiling tool, available via Xcode:

```bash
xctrace record --template 'Time Profiler' --output trace.trace \
  --launch -- /path/to/quench -- test.js
# View:
xctrace show trace.trace
```

### Which to use

| Tool | macOS support | Best for |
|---|---|---|
| `cargo-flamegraph` | ✅ (xctrace) | Flame graphs, CI regression |
| `samply` | ✅ (native) | CPU hotspots, wall-clock time |
| `xctrace` | ✅ (native) | Deep Apple tooling, Instruments users |

Profile **before** adding NaN boxing or bumpalo to measure baseline, then
measure after to confirm the optimization actually helps. Premature
optimization is a trap — let the profiler guide the changes.

## Bootstrap order

`Context::new` builds the Rust realm (intrinsic prototypes +
`%ThrowTypeError%`), then `bootstrap.rs` evaluates `builtins/*.js` in
dependency order: `_intrinsics` → `Object` → `Function` → `Error` →
`Symbol` → `Number`/`Boolean`/`String` → `Array`/`Iterator` →
`Map`/`Set`/`Weak*` → `Promise`/`JSON`/`Reflect`/`Proxy`/`Math` →
`RegExp`/`Date`/`BigInt`/`TypedArray`/… → URI.

## Workflow

Same `AGENTS.md` cycle for both languages: minimal fix in Rust core *or*
`builtins/*.js` *or* `eval/ops.rs`, gated by the test262 stage run. No
TDD — unit tests are written only as regression guards when fixing a bug,
or as refactor pins.

## File / function limits — enforced

`.clippy.toml` + `.cargo/config.toml` (`-D warnings`) gate every build.
No file > 500 lines, no function > 40 lines, no function complexity >
10, no `#[allow(...)]`, no deferrals. Split any offender before adding
to it (current offenders are tracked in R15, `tasks/refactor-plan.md`).
JS files have no enforced limit but should stay under 500 too — split
per builtin category.
