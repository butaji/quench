# Architecture

**Goal:** 100% of test262 (no skips), staged, minimum LOC.
**Execution order:** `tasks/plan.md` (the single source — do not
duplicate it here).
**Design principles:** `docs/principles.md` (effects in return types,
functions before macros, tables over code, one canonical path).

**Shape:** OXC parser + tree-walking interpreter + self-hosted JS
builtins. Three minimal layers:

1. **OXC parse** — `oxc` parses JS into a spec-compliant AST;
   `oxc_semantic` (plan A1) reports early errors as SyntaxError.
2. **Lower** — `lower/` converts oxc AST into the internal AST
   (`ast.rs`), stripping arena lifetimes and collapsing nodes the
   walker doesn't distinguish.
3. **Walk** — `eval/` walks the internal AST node by node. No
   bytecode, no JIT — each node maps to one `eval_*` function.

Why shortest: no hand-written parser, no bytecode layer (large LOC
cost, zero conformance value), spec ops written once, builtins in JS
(~1/3 the LOC of Rust).

## Rust core

```
src/
├── parser.rs        # oxc → internal AST (+ oxc_semantic early errors)
├── lower/           # AST lowering
├── ast.rs           # internal AST
├── interpreter.rs   # eval entry points
├── eval/ops.rs      # canonical spec abstract ops, exposed as %ops%
├── env/             # lexical environments
├── value/           # Value, Object (one canonical store), JsError
├── context/         # Context, Realm, intrinsics
└── builtins/        # crate-backed primitives only (see table)
```

## Object model — the elegant end state

- **One canonical property store.** `Object` owns a single
  insertion-ordered map (`IndexMap`), keys are strings or
  `desc\0id` symbol payloads, values carry full descriptors
  (writable/enumerable/configurable, getters/setters). Every eval
  node, builtin, and `%ops%` op routes through it — no parallel
  lookup paths, no shadow stores, no per-callsite prototype walks.
- **Realm owns all intrinsics (target, plan B1).** Intrinsic
  prototypes (`%Object.prototype%`, `%ThrowTypeError%`, iterator
  prototypes, error ctors, …) live on the `Realm`, cloned per
  `Context`. Today they are thread-local caches bridged by
  `context/intrinsics.rs::IntrinsicSnapshot` (added so
  `$262.createRealm` can't clobber the main realm) — B1 deletes the
  thread-locals *and* the snapshot hack. `Context::reset` then has
  zero pointers to clear.
- **Descriptor semantics follow the spec**: `defineProperty` defaults
  absent attributes to `false`, ValidateAndApply enforces
  non-configurable invariants, strict writes to non-writable targets
  throw.

## `%ops%` — the only Rust↔JS bridge

Frozen object exposed at realm init; each property is one canonical
abstract operation (`ToPrimitive`, `ToObject`, `IteratorNext`,
`SameValue`, …) implemented once in `eval/ops.rs`. Every builtin and
eval node routes through it. New op → `eval/ops.rs` + failing test →
`%ops%` property → JS callsite. No second copy anywhere.

## JS builtins (plan B3)

Pure spec algorithms on `%ops%`, authored in `builtins/*.js`,
embedded via `include_str!`, evaluated once per Realm by
`builtins/bootstrap.rs` in dependency order:
`_intrinsics` → `Object` → `Function` → `Error` → `Symbol` →
`Number`/`Boolean`/`String` → `Array`/`Iterator` → `Map`/`Set`/`Weak*` →
`Promise`/`JSON`/`Reflect`/`Proxy`/`Math` →
`RegExp`/`Date`/`BigInt`/`TypedArray`/`ArrayBuffer`/`Atomics` → URI.

## Crate-backed primitives stay in Rust

| Spec area | Crate | Rust file |
|---|---|---|
| Parsing / early errors | `oxc` (+ `oxc_semantic`), latest version — see `DEPENDENCIES.md` policy | `parser.rs` |
| RegExp exec | `regress` | `builtins/core/regex.rs` |
| Date math | `chrono` | `builtins/core/date.rs` |
| BigInt | `num-bigint` | `builtins/core/bigint.rs` |
| JSON | `serde_json` | `builtins/core/json.rs` |
| URL/URI | `url` (plan A3) | `builtins/core/uri.rs` |
| Temporal | `temporal_rs` | planned |

Each exposes a tiny primitive; the surrounding `.prototype.*` is JS.
Hand-rolled copies are forbidden; new crates need a `DEPENDENCIES.md`
row in the same diff.

## Limits — enforced

`.clippy.toml` + `-D warnings` gate every build: file/function size,
cognitive complexity, and warning limits as configured there — this
file does not duplicate the values. No `#[allow]`. Split offenders
before adding to them.

## Workflow

AGENTS.md cycle in both languages: failing `#[test]` first (Rust side,
wrapping JS via `Context::eval` when needed), minimal fix in Rust core
*or* `builtins/*.js` *or* `eval/ops.rs`, verify, leave the test in.
