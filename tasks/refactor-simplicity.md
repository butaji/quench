# Refactor — simplicity program (principles: docs/principles.md)

Structural simplifications identified by a full-core audit.
**Sequencing vs. the conformance path (tasks/plan.md): R0–R2 ride
inline during stage fixes; R3 lands at the A→B boundary, strictly
before plan B2/B3; R4 after plan B1.** R3/R4 are LOC/clarity levers,
not conformance levers — stage digests show failures cluster in early
errors, destructuring, and TDZ, not completion plumbing.
Each item enters through the standard gate (failing reproducer
`#[test]` first where behavior changes; pure moves are refactor pins).

No occurrence counts or line references below — they rot with a
single commit. Locate current sites with `grep` when starting an item.

## R0. `throw_type_error(msg) -> JsError` helper

AGENTS.md mandates this one-liner; it does not exist. The idiom —
`create_js_error_with_type` + `set_thrown_value` + `Err` — is
repeated throughout `builtins/` and `eval/`.

- Add `value::error::throw_type_error(msg)` (and siblings for other
  error kinds only where they recur heavily).
- Convert call sites module by module; unit test per module.
- Zero behavior change; pure deletion.

## R1. Arg/receiver helpers for Rust builtins

- The `args.first().cloned().unwrap_or(Value::Undefined)` idiom → an
  `Args::at(i)`-style helper returning `Value::Undefined`.
- The `builtins::get_native_this().unwrap_or(Value::Undefined)`
  idiom → a *checked* receiver helper that throws TypeError on wrong
  receiver (fixes a latent bug class: e.g. `Map.prototype.add.call(5)`
  must throw). Each behavioral conversion needs a reproducer test.

## R2. Const-table builtin registration + constructor cleanup

- `p.set("name", native_fn(...))` registration blocks →
  `const METHODS: &[(&str, NativeFn, u32)]` + one `register_methods`
  function (sets name/length/configurability in one place). Prove the
  pattern on the map/set/weak modules first.
- `NativeFunction`'s several near-identical constructors → one
  constructor + builder or `..Default`.
- Payoff compounds: every future builtin becomes table rows.

## R3. `Completion` type — kill the control-flow side channels

The dominant structural win. Today abrupt completion is smuggled
through side channels instead of the return type:

- a `CONTROL_FLOW` thread-local (`interpreter.rs`) with set/take
  accessors and hand-rolled take/match/re-set propagation blocks at
  every statement and loop boundary (`statement.rs`, `iteration.rs`,
  `destructuring.rs`);
- a `THROWN_VALUE` thread-local (`value/error.rs`) because `JsError`
  is just a string and the real thrown value rides alongside `Err`;
- a magic sentinel error string for generator throws, including one
  site that string-matches on it;
- try/finally hand-encoding §13.15 completion semantics with repeated
  save/restore passes.

Target shape:

```rust
enum Abrupt { Break(Option<Label>), Continue(Option<Label>),
              Return(Value), /* Yield... folds in */ }
struct Completion { value: Value, abrupt: Option<Abrupt> }
type JsResult = Result<Completion, Value>; // throw = Err, carrying the value
```

Deletions this unlocks:

- both thread-locals, the sentinel, and every take/match/re-set block
  → `?`, plus `match` only where an abrupt can be *consumed* (loops,
  labeled statements, try/finally);
- the for-of/for-in per-iteration result enums that re-encode
  completion;
- the copy-pasted `is_empty_completion` matches → one method;
- the explicit-return stack and per-statement-kind tail-call
  special-casing;
- the manual save/restore around `IteratorClose`;
- the entire "forgot to re-set control flow" bug class (see the
  apologetic comment trail in `statement.rs`).

Sequencing: R0 first (it aligns error creation so thrown values can
move into `Err` in R3 without churn). R3 lands per-module
(statement → iteration → destructuring → expression), each step a
passing full unit suite + relevant test262 stages.

## R4. Explicit eval context (after R3)

The remaining `thread_local!` blocks (`CURRENT_THIS`, `STRICT_MODE`,
`NEW_TARGET`, `LABEL_STACK`, `SUPER_CLASS`, …, concentrated in
`interpreter.rs`) collapse into an explicit context/frame parameter.
Only slots that genuinely cannot ride the call stack (generators,
`CURRENT_CONTEXT`) survive. Do after R3 — the completion refactor
removes the biggest consumer first and shows which slots remain.

## Rejected

- Macros for the R0–R2 repetition: functions + const tables cover it
  (docs/principles.md — "Functions before macros").
- Reactive/FRP architecture: solves a problem Quench doesn't have
  (no long-lived dataflow graph), adds a translation layer between
  spec text and code, fights minimum-LOC.
