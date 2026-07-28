# Refactor Plan

Goal: 100% of test262 (all 50k+ tests, no skips), staged to 100% per
stage, **as soon as possible**, with **minimum LOC**.

Architecture: **OXC + walker** (`docs/architecture.md`). Execution
order: `tasks/plan.md` (Phases A → B → C). This file is the work
queue behind that path.

Every item follows the `AGENTS.md` failing-test-first cycle. Lint gate
applies to every touched file (`-D warnings`; files ≤ 500 lines,
functions ≤ 40 lines, complexity ≤ 10, ≤ 3 bool params, no `#[allow]`).
Run `cargo clippy -p quench-runtime --all-targets` for current offenders.

Code audit: **`tasks/code-audit-2026-07-25.md`** (real bugs to avoid
reintroducing).

## Active blockers

These are the items blocking progress. Work them in order.

### R17 — oxc_semantic early errors  *(Phase A, top lever)*

Hand-rolling early errors in `lower/` is thousands of LOC.
`oxc_semantic` already implements them. Verify if `ctx.semantic()` is
available from existing oxc usage, then wire parse → semantic check →
SyntaxError before lowering. Delete redundant hand-rolled checks.

- `#[test]`: duplicate `let` in one block → catchable `SyntaxError`.
- `DEPENDENCIES.md` row if a new feature or version is needed.

### R5 — Object property store + spec semantics  *(partial — spec bugs fixed, full collapse pending)*

Spec bugs fixed (commit `28bc28b7`): symbol identity, key ordering,
strict writes, non-writable, `defineProperty` defaults.

Remaining: collapse to `own_props: IndexMap<Key, Prop>`. Do **not**
wait for R0 — language stages need this now.

### R1 — `eval/ops.rs` owns every spec op  *(incremental NOW; finish Phase B)*

`builtins/core/ops_wrapper.rs` re-exports from `eval/ops.rs` — scaffold
only. Private copies remain in `builtins/*.rs` and `eval/`. Grow R1 on
every op touch; finish before Phase B gate.

Ops to own: `to_primitive`, `to_property_key`, `to_object`,
`to_number`, `to_string`, `same_value`, `same_value_zero`,
`is_callable`, `is_constructor`, `ordinary_has_property`,
`create_data_property_or_throw`, `get_iterator`, `iterator_next`,
`iterator_step`, `iterator_close`, `create_iter_result_object`,
`native_fn`, `throw_type_error`.

**Phase B gate:** before R0 / Object stage, zero private copies of the
ops list above remain outside `eval/ops.rs`.

### R0 — Self-host builtins in JS  *(in progress — 34 files in builtins/)*

34 `.js` files in `builtins/`, loaded by `bootstrap.rs`. Continue per
dependency order in `docs/architecture.md`. Never grow Rust builtins
for stages that R0 will delete.

### R2 — One iterator protocol  *(Phase B, with R0 Iterator.js)*

Four duplicates today: `eval/iteration.rs`, `builtins/weak.rs`,
`builtins/map.rs`, `eval/object`. If `for-of` fails earlier on the
eager materializer, land the streaming `ops` path without waiting for
full R0.

### R3 — `chrono`-backed Date core  *(Phase B)*

`builtins/date.rs` hand-rolls leap-year math but never imports `chrono`.
Wire `chrono::NaiveDate` + `chrono::Utc` properly.

---

## When needed (triggered by digest clusters)

These are NOT queued ahead of the active blockers. Do them when a digest
cluster demands them, or opportunistically on touch.

| Item | Trigger |
|------|---------|
| **R6** Realm owns intrinsics | `ThrowTypeError` stage / `Context::reset` bugs |
| **R8** `panic!` → `throw_type_error` | digest hits a panic site |
| **R9** Dead code sweep | surface shrinks under R0/R1/R5 |
| **R10** RAII `CURRENT_CONTEXT` | touches `Context::reset` paths |
| **R11** Collapse `call_js_function` | touches call paths |
| **R14** `lower_expr` fail-loud | new OXC variant appears |
| **R15** Linter sweep | on every PR touch |
| **R16** Drop `FROZEN_OBJECTS` | R5 freeze path lands |
| **R18** RegExp `\p{}` Unicode | RegExp stage |
| **R19** `bumpalo` arena | Phase B, pairs with R20 |
| **R20** NaN-boxed `JsValue` | Phase B, after R5 |
| **R21** String interning | Phase B |
| **R22** Profiling | when loop is the bottleneck |
| **R23** Remove harness overrides | Phase B, after built-ins stabilize |

R7 (absorbed by R1), R12 (done), R13 (absorbed by R0/R5) — no action.

---

## Rules

- Stages gate sequentially in `tasks/index.json` order.
- Within a stage: fix by digest cluster frequency.
- A cluster spanning multiple stages → one root-cause fix.
- `tasks/index.json` is the only live truth; `failures-N.json` regenerate
  at the start of each stage.
- Every fix enters via a failing `#[test]`.
- `cargo test -p quench-runtime` + `cargo clippy --all-targets` green
  before merging. test262 stage gate must not regress.
