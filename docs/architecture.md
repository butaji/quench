# Architecture

**Goal:** 100% of test262, staged, minimum implementation LOC.

**Shape:** small Rust core + self-hosted JS builtins. JS is ~1/3 the
LOC of equivalent Rust and easier to keep spec-faithful, so anything
that can be JS is JS.

## Rust core

Smallest set the builtins cannot be written without.

```
src/
├── parser.rs        # oxc → internal AST
├── lower/           # AST lowering
├── ast.rs           # internal AST
├── interpreter.rs   # eval entry points
├── eval/
│   └── ops.rs       # canonical spec abstract ops, exposed as %ops%
├── env.rs           # lexical environments
├── value/           # Value, Object (one canonical property store), JsError
├── context/         # Context, Realm
└── builtins/
    ├── core/        # crate-backed primitives only
    │   ├── regex.rs     # regress
    │   ├── date.rs      # chrono
    │   ├── bigint.rs    # num-bigint
    │   ├── json.rs       # serde_json
    │   └── uri.rs        # urlencoding
    └── bootstrap.rs # parses + evaluates builtins/*.js at realm init
```

Remainder of `eval/` is eval nodes only — no spec-op re-implementations.

## JS builtins

```
crates/quench-runtime/builtins/
├── _intrinsics.js   # %ops% destructure (resolved at parse time)
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
algorithms on top of `%ops%`. Embedded via `include_str!`; parsed once
per `Realm` by `builtins/bootstrap.rs`.

## `%ops%` — the only Rust↔JS bridge for spec ops

Frozen object exposed at realm init. Each property is a canonical spec
abstract op, implemented once in `eval/ops.rs` and bound as a
`NativeFunction`. JS destructures it at parse time (never user-visible):

```js
// builtins/Array.js (excerpt)
const { IsCallable, ToObject, ThrowTypeError } = %ops%;

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

New op → add to `eval/ops.rs` with a failing test → expose on `%ops%` →
JS callsite. No second copy anywhere.

## Object model — one canonical store

`Object` has a single own-property store (R5 target:
`IndexMap<Key, Prop>`, `Key::Sym` carrying unique symbol identity).
eval nodes, builtins, and `%ops%` all route through it — no parallel
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

JS values (`JsValue`) fit in a single `u64` via NaN boxing — the same
technique used by QuickJS (C), JSC, V8, and Boa v0.21+.

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

Boa v0.21 (October 2025) switched to NaN boxing and reports measurable
runtime and memory improvements over their older enum approach. Reference:
<https://boajs.dev/blog/2025/10/22/boa-release-21>.

**Do NOT start this until R0 is complete.** The Value representation
change touches every call site; R0 gives us a clean JS-layer boundary to
verify correctness afterward. Boa did the same: NaN boxing was added as a
feature-flagged feature alongside the existing enum (`jsvalue-enum`).

Rust `unsafe` is confined to the `value/` module. No unsafe leaks into
`eval/` or `builtins/`.

## Memory — bumpalo arena allocation *(R10 target)*

`bumpalo` (244M+ crate downloads) is the standard arena allocator for Rust
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

Add `DEPENDENCIES.md` row in the same diff as the first arena use.

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

Add `DEPENDENCIES.md` row in the same diff.

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

Same `AGENTS.md` cycle for both languages: failing `#[test]` first (in
Rust, wrapping the JS via `Context::eval` if needed), watch it fail,
minimal fix in Rust core *or* `builtins/*.js` *or* `eval/ops.rs`,
verify, leave the test in.

## File / function limits — enforced

`.clippy.toml` + `.cargo/config.toml` (`-D warnings`) gate every build.
No file > 500 lines, no function > 40 lines, no function complexity >
10, no `#[allow(...)]`, no deferrals. Split any offender before adding
to it (current offenders are tracked in R15, `tasks/refactor-plan.md`).
JS files have no enforced limit but should stay under 500 too — split
per builtin category.