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
├── env/             # lexical environments
├── value/           # Value, Object (one canonical property store), JsError
├── context/         # Context, Realm
└── builtins/
    ├── core/            # %ops% wrapper infrastructure
    ├── regex/           # regress-backed (crate)
    ├── date/            # chrono-backed (crate)
    ├── bigint.rs        # num-bigint-backed (crate)
    ├── json.rs          # serde_json-backed (crate)
    ├── uri.rs           # urlencoding-backed (crate)
    ├── array/, object/, string/, …  # per-type modules
    ├── error/, promise/, map/, …
    └── mod.rs           # module registration (bootstrap.rs planned for R0)
```

Remainder of `eval/` is eval nodes only — no spec-op re-implementations.

## JS builtins (target — R0 not started)

All builtins are currently Rust (`src/builtins/`). The plan is to
self-host them as JS once `%ops%` is fleshed out (R0 in
`tasks/refactor-plan.md`):

```
builtins/
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

Once built, all `*.prototype.*`, intrinsic iterator prototypes,
`Object.*`, `Reflect.*`, `Promise.prototype.*`, etc. are authored here.
Pure spec algorithms on top of `%ops%`. Embedded via `include_str!`;
parsed once per `Realm` by `builtins/bootstrap.rs`.

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