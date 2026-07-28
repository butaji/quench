# Architecture

**Goal:** 100% of test262 (all 50k+ tests, no skips), staged to 100% per
stage, with the **minimum possible LOC**.

**Shape:** OXC parser + tree-walking interpreter + self-hosted JS
builtins. Three layers, each minimal:

1. **OXC parse** — `oxc` parses JS source into a spec-compliant AST
   (~20k LOC of parser logic we don't write).
2. **Lower** — `lower/` converts oxc AST nodes into our simpler internal
   AST (`ast.rs`), stripping arena lifetimes and collapsing nodes the
   walker doesn't need to distinguish.
3. **Walk** — `eval/` walks the internal AST node by node, evaluating
   expressions and statements directly. No bytecode, no JIT, no
   intermediate compilation step.

This is the fastest path to 100% conformance with minimum LOC because:

- **No hand-written parser.** OXC is spec-compliant and covers full
  ECMAScript + TypeScript + JSX. We save ~20k LOC and get correctness
  for free.
- **No bytecode layer.** A tree-walking interpreter is the simplest
  possible evaluator — each AST node maps to one `eval_*` function.
  Bytecode compilation would add a compiler pass + a VM loop (~5–10k
  LOC) with no conformance benefit — test262 tests behavior, not speed.
- **Spec ops in one place.** `eval/ops.rs` owns every canonical
  abstract operation (`ToPrimitive`, `ToObject`, `IteratorNext`, …),
  exposed to JS as `__ops__`. Every eval node and every builtin routes
  through them — no duplicate implementations.
- **Builtins self-hosted in JS.** JS is ~1/3 the LOC of equivalent
  Rust for spec algorithms. Once `__ops__` is complete, builtins move
  to JS (`builtins/*.js`) and the Rust core shrinks.

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

## JS builtins (R0 in progress)

34 `.js` files in `builtins/` (root of repo), loaded by
`builtins/bootstrap.rs`. `__ops__` is scaffolded in
`builtins/core/ops_wrapper.rs`. Order:

```
builtins/
├── Object.js, Array.js, Iterator.js, Symbol.js,
├── Number.js, Boolean.js, String.js, Math.js,
├── Map.js, Set.js, WeakMap.js, WeakSet.js,
├── Promise.js, JSON.js, Reflect.js, Proxy.js,
├── RegExp.js, Date.js, BigInt.js,
├── TypedArray.js, ArrayBuffer.js, DataView.js, Atomics.js,
├── AsyncIterator.js, AsyncGenerator.js, AsyncFunction.js,
├── DisposableStack.js, AsyncDisposableStack.js,
├── FinalizationRegistry.js, WeakRef.js,
└── decodeURI.js, encodeURI.js
```

Every `.prototype.*`, intrinsic iterator prototype, `Object.*`,
`Reflect.*`, `Promise.prototype.*`, etc. authored here. Embedded via
`include_str!`; parsed once per `Realm` by `bootstrap.rs`.

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

## Crate-backed primitives stay in Rust

| Spec area      | Crate         | Rust file                |
|----------------|---------------|--------------------------|
| RegExp exec    | `regress`     | `builtins/regex.rs`      |
| Date math      | `chrono`      | `builtins/date.rs`        |
| BigInt         | `num-bigint`  | `builtins/bigint.rs`      |
| JSON parse/str | `serde_json`  | `builtins/json.rs`        |
| URI            | `urlencoding` | `builtins/uri.rs`         |
| Parsing        | `oxc`         | `parser.rs`              |

Each exposes a tiny primitive; the surrounding `.prototype.*` is JS.
Hand-rolled copies — including `chrono_*` helpers that never import
`chrono` — are forbidden.

## Future optimization targets

All tracked in `tasks/refactor-plan.md` — not duplicated here to avoid drift:

| Target | Ref plan | Timeline |
|--------|----------|----------|
| NaN-boxed `JsValue` (single `u64`) | R20 | Phase B, after R0 |
| `bumpalo` arena allocation | R19 | Phase B, pairs with R20 |
| String interning / atom table | R21 | Phase B |
| Profiling (flamegraph/samply/xctrace) | R22, `docs/tools.md` | When loop is the bottleneck |

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
Full list in R15, `tasks/refactor-plan.md`.
JS files have no enforced limit but should stay under 500 too — split
per builtin category.