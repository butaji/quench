# Refactor Plan — active queue

Refactor queue, in priority order. Every item starts with a failing unit test per
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

- **R21 — `__ops__` bridge semantics.** `eval/ops.rs` still diverges from the
  canonical ops: `IsCallable` misses callable objects, `HasProperty` needs
  complete callable/proxy semantics, `SameValueZero` calls `same_value` (wrong on -0/+0),
  `CreateDataProperty` uses `.set()`, and `DefineProp`/`SealObject`/
  `FreezeObject`/`SetPrototypeOf` duplicate descriptor logic that lives in
  `builtins/object_static/descriptors.rs`. `HasProperty` now also covers user
  function values so self-hosted Array methods can observe indexed properties
  on callable receivers. One op = one implementation; add a failing test per
  op. Required before conformance polish can declare the self-hosted layer
  complete.

- **R29 — Arguments length expansion.** Sloppy duplicate-parameter arguments
  map only the last occurrence of a parameter name, preserving earlier
  argument values. Redefining `arguments.length` now expands indexed access
  with `undefined` values for concat and related array-like consumers.

- **R22 — Targeted builtin migration to JS.** The migration is tracked in
  `tasks/builtin-migration.md`; `bootstrap_js_builtins` is active for normal
  contexts. Prioritize a family only when Test262 failure data or maintained-
  LOC measurement shows leverage. Remove duplicate Rust registrations only
  after JavaScript owns the observable algorithm, validation, coercion,
  ordering, or descriptor behavior. JS ownership is not itself a throughput
  objective.
  Rust remains for interpreter/core operations, canonical `__ops__`, storage
  and native memory, performance-sensitive work, crate-backed primitives,
  engine integration, and explicitly documented lower-LOC direct bindings.
  A JS file that only forwards a call to one Rust primitive is not a
  migration; keep that binding in Rust and record the exception.

- **R23 — Thread-local reduction.** 48 `thread_local!` slots vs the
  shrink-to-zero principle. Targets: strict mode (`interpreter.rs:290`,
  saved/restored at eval boundaries) → carry on the environment/scope;
  generator suspend staging (`eval/iteration.rs:161`, `eval/generator.rs:46`,
  `value/generator_replay.rs`) → fields on `GeneratorObject`;
  `CURRENT_SOURCE` `'static` transmute (`context/mod.rs:89`) → RAII guard or
  scoped source. Pin each with a refactor-pin test (nested eval, nested
  generators, panic mid-eval hygiene).

- **R24 — Stage catalog reconciliation.** Keep `tasks/index.json` as a
  descriptive stage catalog only. Test runs provide all conformance results.

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

- **R29 — Single builtin ownership.** `tools/check-builtin-ownership.sh`
  requires explicit `@builtin-rust` markers for Rust-owned public methods and
  rejects matching JS prototype implementations unless they are documented
  one-line proxy exceptions.

- **R30 — Test262 throughput instrumentation.** Add deterministic structured
  timing for discovery, metadata, harness, context/bootstrap, parse, execution,
  microtasks, cleanup, and worker startup. Emit JSONL outside `docs/` and
  `tasks/`, with runner unit tests.

- **R31 — Persistent configurable workers.** Replace the fixed worker cap with
  a bounded environment-configurable pool. Bootstrap once per worker, keep
  mutable contexts worker-local, support process isolation for crashes, and
  benchmark wall time, tests/sec, memory, timeout, and crash rate.

- **R32 — Root-cause failure fingerprints.** Group stable phase, error type,
  runtime location, execution mode, builtin/abstract operation, and
  normalized-message fields. Rank groups by estimated affected tests per hour
  and test equivalent and distinct causes.

- **R33 — Immutable bootstrap cache.** Measure bootstrap cost, then cache only
  immutable parsed harness/builtin artifacts or realm templates. Add
  realm-isolation, pending-job, thrown-value, and `Context::reset` refactor-pin
  tests.

- **R34 — Concurrent stage batches.** Run independent stages concurrently with
  isolated result files and serialized merge/advance. Require complete
  collection, explicit crash/timeout/skip accounting, failure propagation, and
  no partial advancement.

- **R35 — Fast-loop command.** Add one local workflow entry point for quick
  representative triage, fingerprint-ranked reproducers, full affected-stage
  digest, and final lint checks. It must not edit Test262 fixtures, write
  conformance status to `docs/` or `tasks/`, or manufacture coverage.

- **R36 — Conformance acceleration gate.** Freeze the universal embedding API
  and engine-performance work until Test262 is complete. Revisit only after
  runner timing data shows a conformance-relevant bottleneck.

- **R37 — Test262 crate boundary.** Move harness, metadata, host, and runner
  implementation out of `quench-runtime`; keep the runtime limited to the
  neutral `QuenchRuntime` API (`eval`, `eval_module`, `eval_script`, module
  import, realm snapshots, property access, calls, and host callbacks). Do
  not export interpreter/environment internals to make the move compile.

- **R38 — Module fixture identity.** Preserve canonical module identity for
  `-as.js` resolution fixtures and follow named re-export chains without
  changing Test262 inputs. The stage-53 digest improved to 393/599; remaining
  failures stay grouped by the digest output.

- **R39 — Long-stage conformance execution.** Keep stages 44, 53, and 65
  explicitly unresolved until complete digests are collected. Diagnose long
  stages with external subdirectory sharding or a longer-lived runner process;
  do not treat partial output or unit-test counts as conformance evidence.
