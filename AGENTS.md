# AGENTS.md

GitHub CI is forbidden in this repository. Do not add or restore GitHub Actions workflows.

Do TDD. Dont do debug code. Dont do comments. Dont do debug prints. Never guess — write a
failing unit test first, every time. Unit test is the best comment and the best debug print you can make.

Unit tests exist to get us to 100% test262 faster, not to duplicate it.
test262 (50k+ cases, run per stage) is the conformance gate for all
JS-observable spec behavior — **never replicate a test262 assertion as
a unit test**. A unit test is admitted in exactly three categories:

1. **Reproducers** — every bug, failing test262 case, or behavior
   change enters via one failing `#[test]` asserting the exact
   behavior, written before any production change and left in after.
2. **Core invariants test262 cannot observe** — panic-freedom of
   builtins, realm/`Context::reset` hygiene, `__ops__` semantics,
   storage/key identity, soundness holes (e.g. `FROZEN_OBJECTS`).
3. **Refactor pins** — behavior locked with a test before a delete,
   move, or storage/prototype migration (R0–R16).

No coverage-for-coverage's-sake: if test262 already checks the
behavior byte-for-byte, the stage run is the test. Test code is LOC
too — every test must earn its maintenance cost against the
minimum-LOC goal.

Quench — JavaScript runtime targeting **100% test262 conformance**,
staged to 100% per stage, with the **minimum possible LOC** as a
**small Rust core** plus a **self-hosted JS builtins layer** *(currently
active migration: `bootstrap_js_builtins` runs during normal context
initialization — decision R22)*. Single
crates: `crates/quench-runtime` (runtime) and `crates/quench-test262` (harness/runner). Never modify `tests/test262`.

Test262 must execute unmodified for conformance evidence. Never override,
replace, bypass, or shadow Test262 harness code or assertions in the runtime;
doing so is not pure 100% conformance. Fix the engine or owned runtime
integration instead.

- `docs/architecture.md` — the Rust↔JS split, `__ops__` contract, bootstrap order.
- `docs/review-2026-08.md` — 2026-08 architecture/code review, ranked findings.
- `tasks/refactor-plan.md` — active queue (R18+ from the 2026-08 review).
- `tasks/meta-analysis-stream.md` — conformance workflow. The test262 digest
  output is the sole conformance/progress SSOT; `tasks/index.json` is only a
  descriptive stage catalog and optional runner default.
- `tasks/index.json` — 122 descriptive test262 stage entries; it is never
  evidence of coverage.

## Commands

```bash
cargo build -p quench-runtime
cargo nextest run -p quench-runtime
cargo nextest run -p quench-runtime                 # fast unit-test suite
cargo fmt -p quench-runtime
cargo clippy -p quench-runtime --all-targets

# Diagnostic tools (see docs/tools.md)
cargo run --bin run-test -- <test.js>        # single-test runner with metadata
TEST262_DIGEST=1 TEST262_STAGE=N cargo nextest run -p quench-test262 --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all --no-capture   # collect ALL failures, grouped by error
TEST262_STAGE=N TEST262_DIGEST=1 cargo nextest run -p quench-test262 --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all
bash tools/run-each.sh                        # process-isolated (survives crashes)

# Run the configured stage (the digest output is the coverage SSOT)
cargo nextest run -p quench-test262 --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all --no-capture
# Specific stage
TEST262_STAGE=N cargo nextest run -p quench-test262 --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all --no-capture
# All stages in order, stop on first failure
ALL_STAGES=1 cargo nextest run -p quench-test262 --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all --no-capture
```

122 stages. No checkpoints. No skips. `crates/quench-test262/src/test262/runner` mirrors
`tasks/index.json`. `test/intl402` (ECMA-402) and `test/staging` are
out of scope.

## Linter — enforced, no exceptions

`.cargo/config.toml` sets `-D warnings` (warnings fail the build).
`.clippy.toml` sets the hard limits:

- **500 lines per file** (repo rule — clippy's `too-many-lines-threshold`
  is a *per-function* lint, it does not enforce a file cap). Split the
  file before merging if it crosses 500.
- **40 lines per function** (`too-many-lines-threshold`). Extract a
  helper before merging if a function crosses 40.
- **cognitive complexity ≤ 10** (`cognitive-complexity-threshold`).
  Simplify or extract before merging if it crosses 10.
- **≤ 3 boolean params** (`max-fn-params-bools`). Refactor to a flags
  struct or two functions.
- **zero clippy warnings**. `cargo clippy -p quench-runtime --all-targets`
  must print nothing. `-A warnings` anywhere in the repo is a bug.

Current state (2026-08): clippy prints 16+2 warnings and exits 0 — the
`-D warnings` gate is not wired into clippy runs. Enforcing the gate and
splitting the >500-line files (`eval/class/helpers.rs` 3863,
`early_errors.rs` 3264, `eval/statement.rs` 2053, `eval/iteration.rs`
1859, `builtins/typed_array.rs` 2255, `object_static/descriptors.rs`
1468, `builtins/bootstrap.rs` 1479) is **R27**. Until then the limits
above are enforced at review, not by the build.

A diff that lands a file > 500 lines, a function > 40 lines, a function
with complexity > 10, or any clippy warning is rejected at review — no
`#[allow(...)]` exceptions, no deferral to "next refactor". The
refactor-plan splits exist to bring existing offenders under these
limits.

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
2. **Watch it fail** — `cargo nextest run -p quench-runtime -E 'test(<name>)'` fails with
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
is never edited. Every test written here must fit one of the three
admitted categories at the top of this file — reproducer,
core-invariant, or refactor pin.

## Minimum-LOC rules

Total LOC across the Rust core *and* JS builtins is what we minimize —
not per-PR diffs. Two compounding levers:

1. **Small Rust core.** Parser/lower/eval/value/env/context + a handful
   of crate-backed primitives in `builtins/core/`. Every pure spec
   algorithm on top of `__ops__` is authored in JS (`builtins/*.js`). JS
   is ~1/3 the LOC of equivalent Rust; that is the entire reason the
   split exists. *(Current state: builtins are Rust-first; the JS layer
   is active during the R22 migration.)*
2. **One canonical spec-op path.** `ToPrimitive`, `ToPropertyKey`,
   `ToObject`, `IteratorNext`, `IteratorClose`,
   `CreateDataPropertyOrThrow`, `OrdinaryHasProperty`, `IsCallable`,
   `SameValueZero`, … live in exactly one place: `src/eval/ops.rs`,
   exposed to JS as a frozen `__ops__` object. Every builtin (Rust or
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
  `Realm`). *(Current state: cached in thread-locals, cleared by
  `intrinsics::clear_intrinsics`; a `Realm` struct is the R23 target.)*
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
  diff is rejected. Fixture: `cargo nextest run` + `cargo clippy --all-targets`
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
  They never reach into `Object` storage directly — they call `__ops__`.
  New op: `eval/ops.rs` + failing test → `__ops__` property → JS callsite.
- **Crate-backed primitives** (regress / chrono / num-bigint /
  serde_json / urlencoding) live in `builtins/core/` as small Rust
  fns; the `.prototype.*` and constructor wiring is JS.
- **Symbols**: `Value::Symbol(Rc<Symbol>)`; `property_key()` yields the
  `desc\0id` key string, used directly as a property key.
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
