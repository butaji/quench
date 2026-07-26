# Refactor — simplicity program (principles: docs/principles.md)

Structural simplifications identified by a full-core audit. Ordered
by payoff/risk: R0–R2 are mechanical quick wins; R3 is the big one.
Each item enters through the standard gate (failing reproducer
`#[test]` first where behavior changes; pure moves are refactor pins).

## R0. `throw_type_error(msg) -> JsError` helper

AGENTS.md mandates this one-liner; it does not exist. The 3-line
idiom — `create_js_error_with_type` + `set_thrown_value` + `Err` —
appears at ~150 sites (`create_js_error_with_type`: 166 calls in 42
files, e.g. `eval/class/helpers.rs:37-54` twice in 10 lines).

- Add `value::error::throw_type_error(msg)` (and siblings for the
  other error kinds only where ~10+ sites exist).
- Convert call sites module by module; unit test per module converted.
- Est. −300 LOC. Zero behavior change.

## R1. Arg/receiver helpers for Rust builtins

- `args.first().cloned().unwrap_or(Value::Undefined)` — 100 sites in
  32 files → `Args::at(i)`-style helper returning `Value::Undefined`.
- `builtins::get_native_this().unwrap_or(Value::Undefined)` — 60
  sites in 22 files → a *checked* receiver helper that throws
  TypeError on wrong receiver (fixes latent bug farm, e.g.
  `Map.prototype.add.call(5)` should throw). Each conversion needs a
  reproducer test where behavior changes.
- Est. −100–150 LOC.

## R2. Const-table builtin registration + constructor cleanup

- `p.set("add", native_fn(...))` registration blocks (e.g.
  `builtins/map/set.rs:75-85`) → `const METHODS: &[(&str, NativeFn,
  u32)]` + one `register_methods` function (sets name/length/
  configurability in one place). Start with map/set/weak (27
  `native_fn` sites) as the proving ground.
- `NativeFunction`'s 5 near-identical constructors
  (`value/function/native_function.rs:46-148`, ~60 duplicated field
  inits) → one constructor + builder or `..Default`.
- Est. −100–150 LOC; every future builtin becomes table rows.

## R3. `Completion` type — kill the control-flow side channels

The dominant structural win. Today abrupt completion is smuggled
through two thread-locals plus a sentinel:

- `CONTROL_FLOW` thread-local (`interpreter.rs:24,49-69`) — 257
  occurrences in 16 files (`statement.rs` 92, `iteration.rs` 41).
- `THROWN_VALUE` thread-local (`value/error.rs:48-67`) — 182
  occurrences in 36 files.
- Sentinel `Err(JsError("Generator threw"))` — 11 sites, one
  string-matched (`eval/object/helpers/destructuring.rs:241`).
- ~30 hand-rolled take/match/re-set propagation blocks (e.g.
  `statement.rs:174,321,853,915,1051`, `iteration.rs:384,593`);
  try/finally hand-encodes §13.15 with 4 save/restore passes
  (`statement.rs:1131-1236`).

Target shape:

```rust
enum Abrupt { Break(Option<Label>), Continue(Option<Label>),
              Return(Value), /* Yield... folds in */ }
struct Completion { value: Value, abrupt: Option<Abrupt> }
type JsResult = Result<Completion, Value>; // throw = Err, carrying the value
```

Deletions this unlocks:

- `CONTROL_FLOW`, `THROWN_VALUE`, the sentinel, ~30 plumbing blocks
  → `?` plus `match` only where an abrupt can be *consumed* (loops,
  labeled statements, try/finally).
- `ForOfIterResult`/`ForInIterResult` (`iteration.rs:102-107,570-574`)
  — adapters that re-encode completion.
- Triplicated `is_empty_completion` (`interpreter.rs:410-417`,
  `statement.rs:150-159,301-310`) → one method.
- `EXPLICIT_RETURN_STACK` and per-statement-kind tail-call
  special-casing (`statement.rs:260-295`).
- `abrupt_close`'s manual save/restore (`iteration.rs:82-100`).

Est. −400–700 LOC, and removes the "forgot to re-set control flow"
bug class (see comment trail `statement.rs:166-170,314-317`).

Sequencing: R0 first (it aligns error creation so thrown values can
move into `Err` in R3 without churn). R3 lands per-module
(statement → iteration → destructuring → expression), each step a
passing full unit suite + relevant test262 stages.

## R4. Explicit eval context (after R3)

40 `thread_local!` blocks in 22 files (15 in `interpreter.rs`:
`CURRENT_THIS`, `STRICT_MODE`, `NEW_TARGET`, `LABEL_STACK`,
`SUPER_CLASS`, …) collapse into an explicit context/frame parameter.
Only slots that genuinely cannot ride the call stack (generators,
`CURRENT_CONTEXT`) survive. Do after R3 — the completion refactor
removes the biggest consumer first and shows which slots remain.

## Rejected

- Macros for the R0–R2 repetition: functions + const tables cover it
  (docs/principles.md — "Functions before macros").
- Reactive/FRP architecture: solves a problem Quench doesn't have
  (no long-lived dataflow graph), adds a translation layer between
  spec text and code, fights minimum-LOC.
