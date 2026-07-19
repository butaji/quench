# AGENTS.md

Quench — JavaScript runtime targeting **100% test262 conformance**,
staged to 100% per stage, implemented with the **minimum possible LOC**
as a **small Rust core** plus a **self-hosted JS builtins layer**. Single
crate: `crates/quench-runtime`. Never modify `tests/test262`.

See `docs/architecture.md` for the full split and `tasks/refactor-plan.md`
for the active migration queue (R0 self-hosting pivot through R14).

## Commands

```bash
cargo build -p quench-runtime
cargo test -p quench-runtime
cargo fmt -p quench-runtime
cargo clippy -p quench-runtime --all-targets

# Run one stage
cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
# Specific stage
TEST262_STAGE=1 cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
# Run every stage in order, stop on first failure
ALL_STAGES=1 cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
```

93 stages. No checkpoints. No skips. Each stage runs to 100% passing
before advancing. The stage list lives in
`crates/quench-runtime/src/test262/runner.rs::STAGES` and must mirror
`tasks/index.json` exactly. `test/intl402` (ECMA-402) and `test/staging`
are intentionally out of scope.

## Workflow: unit tests, not guesswork — enforced, no exceptions

**You do not debug. You do not guess. You write a failing unit test first.
Every. Single. Time. No exceptions.**

This is the contract. A failing test262 case, a bug, a new builtin, a parser
or evaluator change — all of them enter the codebase through the same gate:
a `#[test]` that asserts the exact behavior, committed *before* any production
change. If you cannot express the behavior as a unit test, you do not
understand it yet, and you are not allowed to touch production code.

### Forbidden

- `println!` / `dbg!` archaeology. **Never.**
- Reading code until it "looks wrong" and patching. **Never.**
- "Let me try this" speculative edits. **Never.**
- Refactors done "while I'm here" without a test. **Never.**
- Skipping the failing-test step "just this once". **Never.**
- Editing `tests/test262.rs` or anything under `tests/test262/`. **Never.**

### Mandatory cycle, in order

1. **Reproduce** — add a `#[test]` in the relevant module's `mod tests` (or in
   `crates/quench-runtime/tests/`) that exercises the exact JS or Rust behavior
   under inspection. Mirror the surrounding test style (see
   `src/eval/string_methods.rs`, `src/builtins/map.rs` for the established
   pattern).
2. **Watch it fail** — `cargo test -p quench-runtime <name>` must fail with the
   same symptom as the test262 case. If it does not fail, or fails for the wrong
   reason, delete the test, write a better one. Do not proceed. You do not
   understand the bug yet.
3. **Fix** — make the minimal change to production code so the unit test
   passes. Nothing else. No opportunistic refactors.
4. **Verify** — re-run the unit test, the module's full test suite, then the
   relevant test262 stage. Run `cargo fmt` / `cargo clippy --all-targets`
   until both are clean. Linter warnings are not optional polish — they block
   the fix from being "done".
5. **Leave the test in** — regression coverage stays in the tree. A fix
   without a committed test is not done.

test262 output is a signal for *what* to test, not a substitute for the test
itself — the conformance run lives in `tests/test262.rs` only and is never
edited; the reproductions live next to the code under `src/.../mod tests`.

## Minimum-LOC strategy

The goal is the smallest correct runtime that passes every enumerated
test262 stage. Reaching 100% covers ~40k tests; total implementation LOC
across the Rust core *and* the JS builtins layer is what we minimize,
not per-PR diffs. Two compounding levers:

1. **Small Rust core.** Only the parser/lower/eval/value/env/context
   surface plus a handful of crate-backed primitives in
   `builtins/core/` live in Rust. Every pure spec algorithm on top of
   `%ops%` is authored in JS under `builtins/*.js`. JS is ~1/3 the LOC
   of the equivalent Rust; exploiting that is the entire reason the
   split exists.
2. **One canonical spec-abstract-operation path.** `ToPrimitive`,
   `ToPropertyKey`, `ToObject`, `IteratorNext`, `IteratorClose`,
   `CreateDataPropertyOrThrow`, `OrdinaryHasProperty`, `IsCallable`,
   `SameValueZero`, … live in **one** place: `src/eval/ops.rs`,
   exposed to JS as a frozen `%ops%` object. Every builtin (Rust or
   JS) and every eval node routes through them. Re-implementing a spec
   op anywhere else — including a thinly-disguised copy in JS — is
   forbidden. Before writing a private `to_*` / `same_value*` /
   `is_callable` / `native_fn` / `iterator_*` helper, grep
   `src/eval/ops.rs`. If it exists there, use it. If it doesn't, add
   it there with a failing-test-first cycle and reuse it.

Strategic rules:

- **One iterator protocol.** `%IteratorPrototype%` is implemented once.
  Array / String / RegExp / Map / Set iterators and `%GeneratorPrototype%`
  inherit via the prototype chain instead of carrying per-type method
  tables. Same for `%AsyncIteratorPrototype%`. No eager materialization:
  iterators are streamed through `iterator_next` / `iterator_step` /
  `iterator_close` from `eval/ops.rs`.
- **Prefer a crate over hand-rolling** when the crate already implements
  the spec algorithm verbatim. Confirmed dependency choices live in
  `DEPENDENCIES.md`; adding a hand-rolled copy of `regress`, `num-bigint`,
  or `chrono` semantics is forbidden — including thinly-disguised copies
  under helpers named after the crate (a `chrono_*` function that never
  imports `chrono` is the bug). Adding a *new* crate requires a
  `DEPENDENCIES.md` row in the same diff.
- **Share intrinsic prototypes across realms.** `ThrowTypeError`,
  `%IteratorPrototype%`, intrinsic error constructors — wire once onto a
  `Realm` and clone, never rebuild per `Context::new` / `Context::reset`.
  `Context::reset` must clear *every* thread-local proto pointer
  consistently (ideally there are none to clear because they live on
  `Realm`).
- **No speculative generality.** Don't add slots, flags, hooks, enum
  variants, vtables, or storage maps that no current test262 case
  exercises. They cost LOC now and create drift later. Add them when the
  stage that needs them lands, with a failing unit test. "I'll wire it up
  next stage" is not a sufficient reason to land dead scaffolding — if
  it has zero call sites, it gets deleted in the same PR.

### Zero duplication — enforceable, no exceptions

Duplication is the single largest LOC driver and the single largest
source of spec-drift bugs in this crate's history. The contract:

- `grep -R` for the symbol you're about to define. If a spec-abstract op
  (any name in ECMA-262's "Abstract Operations" section) already exists
  anywhere under `src/`, your job is to delete the duplicate and route
  through the canonical one — not to add a third copy.
- Two structurally identical `fn`s in two `builtins/*.rs` files that
  operate on `Value`/`Object` must be hoisted to `value/` or `eval/ops.rs`
  in the same PR, with a unit test for the hoisted version.
- "I only need it in one file for now" is not a reason to private-copy a
  spec op. The canonical home exists; put it there.
- `#[allow(dead_code)]` on a function, struct, enum variant, or field is
  a `TODO(delete)` marker, not a permanent state. A diff that *adds* an
  `#[allow(dead_code)]` is rejected at review unless it deletes the
  annotated symbol in the same diff.

### Dead code is a bug, not polish

- A `pub fn` with zero callers outside its own module is deleted. Adding
  `pub` "in case someone needs it" is forbidden.
- An `enum` variant that is constructed nowhere is deleted in the same
  PR that notices.
- A struct field that is written but never read (`vtable`, `slots`,
  `data`, `props`) is deleted in the same PR that notices.
- A `thread_local!` pointer that is rebuilt by `register_builtins` on
  every realm is restructured onto `Realm` (see R5 in
  `tasks/refactor-plan.md`).
- The fixture for catching dead code is `cargo test -p quench-runtime` +
  `cargo clippy -p quench-runtime --all-targets` clean. `cargo +nightly
  udeps` or a manual `grep` for the symbol across `src/` is the check.

### Builtins throw `JsError`, never panic — and never via `JsError::from(&str)`

- Use `value::error::throw_type_error(msg) -> JsError` (one line,
  performs both `create_js_error_with_type` and `set_thrown_value`).
  Other error classes get the same helper if a second one shows up.
- `JsError::from("TypeError: ...")` is forbidden — it produces no JS
  error object, loses `stack`/subclass identity, and is only catchable
  because `eval_try_catch` happens to scrape the string.
- `panic!` / `unwrap()` / `expect()` in `builtins/` or `eval/` is
  forbidden (the only exceptions are `unreachable!` in pattern arms the
  spec rules out, and `tests/`).

Verifying minimum-LOC: when a builtin lands, ask "could this be 3 fewer
lines by calling an existing spec op?" If yes, do that; if the spec op
does not exist yet, extract it (with a test) and reuse it. The active
cross-cutting cleanup queue is `tasks/refactor-plan.md` (R0–R14); new
spec-op extractions should be added there if they don't fit an existing
item.

## Architecture

```
crates/quench-runtime/
├── src/
│   ├── parser.rs        # OXC → internal AST (TS/TSX/JSX)
│   ├── lower/           # AST lowering
│   ├── ast.rs           # internal AST
│   ├── interpreter.rs   # eval entry points
│   ├── eval/
│   │   └── ops.rs       # canonical spec abstract operations + %ops% bridge
│   ├── env.rs           # lexical environments
│   ├── value/           # Value, Object, Function, NativeFunction, JsError
│   ├── context/         # Context, Realm (intrinsic prototypes owned here)
│   ├── builtins/
│   │   ├── core/        # crate-backed primitives only
│   │   │   ├── regex.rs    # regress
│   │   │   ├── date.rs     # chrono
│   │   │   ├── bigint.rs   # num-bigint
│   │   │   ├── json.rs     # serde_json
│   │   │   └── uri.rs      # urlencoding
│   │   ├── bootstrap.rs # parses + evaluates builtins/*.js at realm init
│   │   └── mod.rs
│   └── test262/         # staged runner (no checkpoints, no skips)
└── builtins/            # self-hosted JS builtins, embedded via include_str!
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

See `docs/architecture.md` for the full split and the `%ops%` contract.

The active cross-cutting cleanup queue is `tasks/refactor-plan.md`
(R0–R14), led by the self-hosting pivot (R0) and the canonical `%ops%`
home (R1). All non-trivial refactors route through that list; if a
spec-op extraction doesn't fit an existing item, add a new Rn row.

## Conventions

- **Self-hosted builtins** live as JS in `crates/quench-runtime/builtins/*.js`,
  embedded via `include_str!`, parsed once at realm init by
  `builtins/bootstrap.rs`. They never reach into `Object` storage
  directly — they call `%ops%` (a frozen object exposed from
  `eval/ops.rs`). A new spec op goes into `eval/ops.rs` with a failing
  test, then a `%ops%` property, then a JS callsite.
- **Crate-backed primitives** (regress / chrono / num-bigint /
  serde_json / urlencoding) live in `builtins/core/` as small Rust
  functions; the surrounding `.prototype.*` and constructor wiring is
  JS. No hand-rolled reimplementations of these crates under
  crate-named helpers.
- **Builtins throw `JsError`**, never panic. Use
  `value::error::throw_type_error(msg)` (a one-line helper that
  performs `create_js_error_with_type` + `set_thrown_value` together).
  Other error classes get the same treatment if a second helper shows
  up. `JsError::from("TypeError: …")` and `panic!`/`unwrap()`/`expect()`
  in `builtins/` or `eval/` are forbidden (only `unreachable!` in
  spec-impossible pattern arms is allowed).
- **Minimal diffs** — match surrounding style, no opportunistic refactors.
- **Symbols**: `Value::Symbol` payload is raw `desc\0id` string; used as property key directly.
- **Boxed primitives**: stored via `builtins::object::set_boxed_value` as `_value` property.
- **Function strictness** captured at definition, never inherited from call site.
  Class bodies are always strict.
- **Accessor properties**: use `Object::define_accessor`; `GetterStorage.func` takes precedence.
- **`CURRENT_CONTEXT`** (context/mod.rs): `thread_local` raw pointer set for the duration of eval.
- New Rust-backed primitives registered in `builtins/mod.rs::register_builtins`;
  self-hosted JS builtins registered in `builtins/bootstrap.rs` in
  dependency order (see `docs/architecture.md`).
