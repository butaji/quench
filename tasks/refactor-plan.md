# Refactor Plan — active queue

Active refactor queue, in priority order. R0–R17 (the self-hosting pivot and
earlier splits) are complete; R18+ come from the 2026-08 architecture review
(`docs/review-2026-08.md`). Every item starts with a failing unit test per
`AGENTS.md` (reproducer or refactor pin), then the minimal change, then the
relevant test262 stage.

- **R18 — SameValueZero symbol identity (verified bug).**
  `value/compare.rs::same_value_zero` falls to `a == b` for Symbols;
  `Value::PartialEq` is false for all Symbols, so Map/Set symbol keys never
  match: `m.set(s,"v"); m.get(s) === undefined`. Fix the Symbol arm
  (`ai.id == bi.id`, reuse `same_value_same_type`), failing test first
  (`Map`/`Set` symbol key get/set/has). Stage: `built-ins/Map`, `built-ins/Set`.

- **R19 — Map/Set internal slots leak.** `_entries`/`size` are stored as
  ordinary enumerable properties (`map/helpers.rs::init_map_object` via
  `.set()`); `Object.keys(new Map())` returns `_entries,size` (spec: `[]`).
  Store non-enumerable, or real internal slots; `size` belongs on the
  prototype as an accessor. Failing test first.

- **R20 — Loose equality must use the canonical ToPrimitive.**
  `value/compare.rs` hand-rolls valueOf/toString (`object_to_primitive_for_compare`),
  ignoring `Symbol.toPrimitive` and swallowing throws; `parse_number_string`
  overflows `u64` on long hex. Route `loose_eq` through
  `value/primitive.rs::to_primitive`, propagate TypeErrors, delete the
  hand-rolled copy and its `#[allow(dead_code)]`.

- **R21 — `__ops__` bridge semantics.** `eval/ops.rs` diverges from the
  canonical ops: `IsCallable` misses callable objects, `HasProperty` is
  `has_own`, `SameValueZero` calls `same_value` (wrong on -0/+0),
  `CreateDataProperty` uses `.set()`, and `DefineProp`/`SealObject`/
  `FreezeObject`/`SetPrototypeOf` duplicate descriptor logic that lives in
  `builtins/object_static/descriptors.rs`. One op = one implementation; add a
  failing test per op. Required before R22 revives the JS layer.

- **R22 — Migrate all builtins to JS.** The migration is tracked in
  `tasks/builtin-migration.md`. `bootstrap_js_builtins` is currently dormant;
  migrate one family at a time on top of canonical `__ops__`, measure the
  relevant Test262 stage, and delete duplicate Rust registrations only after
  the JS path is green. Rust remains for the interpreter core,
  performance-sensitive storage/scheduling, and crate-backed primitives.

- **R23 — Thread-local reduction.** 48 `thread_local!` slots vs the
  shrink-to-zero principle. Targets: strict mode (`interpreter.rs:290`,
  saved/restored at eval boundaries) → carry on the environment/scope;
  generator suspend staging (`eval/iteration.rs:161`, `eval/generator.rs:46`,
  `value/generator_replay.rs`) → fields on `GeneratorObject`;
  `CURRENT_SOURCE` `'static` transmute (`context/mod.rs:89`) → RAII guard or
  scoped source. Pin each with a refactor-pin test (nested eval, nested
  generators, panic mid-eval hygiene).

- **R24 — Stage catalog reconciliation.** The test262 digest is the sole
  coverage authority. `tasks/index.json` is descriptive configuration only;
  never treat its statuses or counts as evidence of coverage.

- **R25 — Dead code and dependencies.** Remove `anyhow`, `tracing`, `phf`
  from `Cargo.toml` (zero uses in `src/`); delete `interner.rs`
  (`StringInterner` is constructed per `Context`, read nowhere; interning is
  deferred per `docs/architecture.md`); delete `#[allow(dead_code)]` symbols
  (`ClassValue::from_ast`, `to_primitive_for_compare`, `Context::env`) or
  give them callers.

- **R26 — Dedup `Context` eval entry points.** `eval`, `eval_es_module`,
  `eval_typescript` are three copies of the same set/run/clear/microtask
  plumbing (`context/mod.rs:77-200`). Collapse to one `eval_inner(parse,
  is_module)`; ~80 LOC saved.

- **R27 — Linter gate enforcement.** `cargo clippy --all-targets` must fail on
  warnings (wire `-D warnings` into the clippy invocation / CI). Split the
  >500-line files: `eval/class/helpers.rs` (3863), `early_errors.rs` (3264),
  `eval/statement.rs` (2053), `eval/iteration.rs` (1859),
  `builtins/typed_array.rs` (2255), `builtins/object_static/descriptors.rs`
  (1468), `builtins/bootstrap.rs` (1479), `test262/harness/mod.rs` (1896),
  `value/generator.rs` (1875). Audit the 77 `#[allow(...)]`s (keep only
  re-export `unused_imports`).

- **R28 — Data-driven global lengths.** `Context::register_native`
  (`context/mod.rs:253`) hardcodes `length` for parseInt/parseFloat/isNaN/…
  in a match; move to the builtin registration data.
