# AGENTS.md

Do TDD. We need Rust core covered with unit tests. Dont duplicated test262 coverage, but cover with unit tests core stability, things not covered with test262.

Dont do debug code. Dont do debug prints. Do unit tests.

Never guess or do printing, write unit tests. Dont just replicate test262 tests as unit tests, never.

Make proper coverage of Rust Runtime Core with unit tests, of all layers and all functions. Changing of this core is always with TDD.

Quench — JavaScript runtime targeting **100% test262 conformance**,
staged to 100% per stage, with the **minimum possible LOC** as a
**small Rust core** plus a **self-hosted JS builtins layer**. Single
crate: `crates/quench-runtime`. Never modify `tests/test262`.

- `docs/architecture.md` — the Rust↔JS split, `%ops%` contract, bootstrap order.
- `tasks/refactor-plan.md` — active queue (R0 self-hosting pivot → R14).
- `tasks/index.json` — 122 test262 stages; each runs to 100% before advancing.

## Commands

```bash
cargo build -p quench-runtime
cargo test -p quench-runtime
cargo fmt -p quench-runtime
cargo clippy -p quench-runtime --all-targets

# Run current stage (see tasks/index.json `current_stage`)
cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
# Specific stage
TEST262_STAGE=N cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
# All stages in order, stop on first failure
ALL_STAGES=1 cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
```

122 stages. No checkpoints. No skips. `src/test262/runner.rs::STAGES` mirrors
`tasks/index.json`. `test/intl402` (ECMA-402) and `test/staging` are
out of scope.

## Linter — enforced, no exceptions

`.cargo/config.toml` sets `-D warnings` (warnings fail the build).
`.clippy.toml` sets the hard limits:

- **500 lines per file** (`too-many-lines-threshold`). Split the file
  before merging if it crosses 500.
- **40 lines per function** (`too-many-lines-threshold` on functions).
  Extract a helper before merging if a function crosses 40.
- **cognitive complexity ≤ 10** (`cognitive-complexity-threshold`).
  Simplify or extract before merging if it crosses 10.
- **≤ 3 boolean params** (`max-fn-params-bools`). Refactor to a flags
  struct or two functions.
- **zero clippy warnings**. `cargo clippy -p quench-runtime --all-targets`
  must print nothing. `-A warnings` anywhere in the repo is a bug.

A diff that lands a file > 500 lines, a function > 40 lines, a function
with complexity > 10, or any clippy warning is rejected at review — no
`#[allow(...)]` exceptions, no deferral to "next refactor". The
refactor-plan splits (e.g. R12 for `eval/object.rs`) exist to bring
existing offenders under these limits.

## Workflow — unit tests, not guesswork (enforced, no exceptions)

**You do not debug. You do not guess. You write a failing unit test
first. Every. Single. Time. No exceptions.** A failing test262 case, a
bug, a new builtin, a parser change — all enter the codebase through
the same gate: a `#[test]` that asserts the exact behavior, committed
*before* any production change. If you cannot express it as a unit
test, you do not understand it yet and are not allowed to touch
production code.

Forbidden: `println!`/`dbg!` archaeology; reading code until it "looks
wrong" and patching; speculative "let me try this" edits; opportunistic
refactors; skipping the failing-test step "just this once"; editing
`tests/test262.rs` or anything under `tests/test262/`.

Cycle (in order):

1. **Reproduce** — `#[test]` in the relevant module's `mod tests` (or
   `crates/quench-runtime/tests/`) asserting the exact behavior. Mirror
   `src/eval/string_methods.rs`, `src/builtins/map.rs`. For JS builtins
   the test lives in Rust and wraps the JS via `Context::eval`.
2. **Watch it fail** — `cargo test -p quench-runtime <name>` fails with
   the same symptom as the test262 case. If not, delete the test; you
   do not understand the bug yet.
3. **Fix** — minimal change to `src/` or `builtins/*.js` or
   `eval/ops.rs`. Nothing else.
4. **Verify** — re-run unit test, the module's suite, then the relevant
   test262 stage. `cargo fmt` + `cargo clippy --all-targets` clean.
   Linter warnings block the fix from being "done".
5. **Leave the test in.**

test262 output signals *what* to test; the reproduction lives as a
unit test next to the code. The conformance run in `tests/test262.rs`
is never edited.

## Minimum-LOC rules

Total LOC across the Rust core *and* JS builtins is what we minimize —
not per-PR diffs. Two compounding levers:

1. **Small Rust core.** Parser/lower/eval/value/env/context + a handful
   of crate-backed primitives in `builtins/core/`. Every pure spec
   algorithm on top of `%ops%` is authored in JS (`builtins/*.js`). JS
   is ~1/3 the LOC of equivalent Rust; that is the entire reason the
   split exists.
2. **One canonical spec-op path.** `ToPrimitive`, `ToPropertyKey`,
   `ToObject`, `IteratorNext`, `IteratorClose`,
   `CreateDataPropertyOrThrow`, `OrdinaryHasProperty`, `IsCallable`,
   `SameValueZero`, … live in exactly one place: `src/eval/ops.rs`,
   exposed to JS as a frozen `%ops%` object. Every builtin (Rust or
   JS) and every eval node routes through them. Before writing any
   `to_*` / `same_value*` / `is_callable` / `native_fn` / `iterator_*`
   helper, grep `src/eval/ops.rs`. Use it if it exists; add it there
   (with a failing test) if it doesn't.

Strategic rules:

- **One iterator protocol.** `%IteratorPrototype%` once; Array / String
  / RegExp / Map / Set iterators and `%GeneratorPrototype%` inherit via
  the prototype chain. No eager materialization — stream via
  `iterator_next` / `iterator_step` / `iterator_close`.
- **Prefer a crate over hand-rolling.** Confirmed in `DEPENDENCIES.md`:
  `regress`, `chrono`, `num-bigint`, `serde_json`, `urlencoding`, `oxc`.
  A hand-rolled copy — including a thinly-disguised `chrono_*` helper
  that never imports `chrono` — is forbidden. A new crate needs a
  `DEPENDENCIES.md` row in the same diff.
- **Share intrinsic prototypes across realms.** `ThrowTypeError`,
  `%IteratorPrototype%`, intrinsic error constructors — wire once onto a
  `Realm`, clone per `Context::new`. `Context::reset` clears *every*
  thread-local proto pointer consistently (ideally zero — they live on
  `Realm`).
- **No speculative generality.** No slots, flags, hooks, enum variants,
  vtables, or storage maps that no current stage exercises. Cost now,
  drift later. If a refactor scaffolds something with zero call sites,
  it's deleted in the same PR.
- **Zero duplication.** `grep -R` before defining any symbol. Two
  structurally identical `fn`s across files must be hoisted to `value/`
  or `eval/ops.rs` in the same PR, with a unit test for the hoisted
  version. "I only need it in one file" is not a reason to private-copy
  a spec op.
- **Dead code is a bug.** A `pub fn` with zero callers outside its
  module is deleted. An `enum` variant constructed nowhere is deleted
  in the same PR that notices. A struct field written but never read is
  deleted in the same PR. `#[allow(dead_code)]` is a `TODO(delete)`
  marker — a diff that adds one without deleting the symbol in the same
  diff is rejected. Fixture: `cargo test` + `cargo clippy --all-targets`
  clean; `cargo +nightly udeps` or `grep` across `src/`.
- **Builtins throw, never panic.** `JsError::from("TypeError: …")` and
  `panic!`/`unwrap()`/`expect()` in `builtins/` or `eval/` are forbidden
  (`unreachable!` in spec-impossible pattern arms is allowed; `tests/`
  can panic). Use `value::error::throw_type_error(msg) -> JsError`
  (one-line helper performing `create_js_error_with_type` +
  `set_thrown_value`).

Before landing a builtin, ask: "could this be 3 fewer lines by calling
an existing spec op?" If yes, do that; if the op doesn't exist yet,
extract it (with a test) and reuse it. New spec-op extractions go into
`tasks/refactor-plan.md` if they don't fit an existing Rn.

## Conventions

- **Self-hosted builtins** live as JS in `builtins/*.js`, embedded via
  `include_str!`, parsed once per realm by `builtins/bootstrap.rs`.
  They never reach into `Object` storage directly — they call `%ops%`.
  New op: `eval/ops.rs` + failing test → `%ops%` property → JS callsite.
- **Crate-backed primitives** (regress / chrono / num-bigint /
  serde_json / urlencoding) live in `builtins/core/` as small Rust
  fns; the `.prototype.*` and constructor wiring is JS.
- **Symbols**: `Value::Symbol` payload is raw `desc\0id` string; used
  as a property key directly.
- **Boxed primitives**: stored via `builtins::object::set_boxed_value`
  as `_value` property.
- **Function strictness** captured at definition, never inherited from
  call site. Class bodies are always strict.
- **Accessor properties**: use `Object::define_accessor`;
  `GetterStorage.func` takes precedence.
- **`CURRENT_CONTEXT`** (`context/mod.rs`): `thread_local` raw pointer
  set for the duration of eval.
- New Rust primitives in `builtins/mod.rs::register_builtins`; JS
  builtins in `builtins/bootstrap.rs` in dependency order
  (see `docs/architecture.md`).
