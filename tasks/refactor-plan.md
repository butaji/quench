# Refactor Plan — active queue

Refactor queue, in priority order. This queue is deferred until the complete
configured Test262 corpus is at 100% (zero failures and zero skips). Until
then, perform only minimal targeted conformance fixes; do not treat a failing
stage as permission for an opportunistic refactor or broad change. Every item
starts from a complete 100% baseline, uses a failing unit test per `AGENTS.md`
(reproducer or refactor pin), and finishes only after a complete Test262 rerun
proves that 100% was preserved. The relevant stage is diagnostic evidence, not
the acceptance gate for a refactor.

## Phase order

Phase 0 contains the conformance and runner work through R40. Phase 1 is the
fresh baseline (R41) and minimal rooted-handle/isolate boundaries (R42). Phase
2 is IR parity (R43). Phases 3–5 add layouts (R45), the collector spike (R46),
and the entry-guarded native tier (R47), each only after the prior phase's
conformance and measurement gates pass.

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

- **R36 — Conformance acceleration gate.** Conformance remains the release
  gate. Architectural experiments may proceed only behind differential tests
  and must not delay Test262 fixes; performance claims require the separate
  reproducible framework and VM-kernel benchmarks.

- **R37 — Test262 crate boundary.** Move harness, metadata, host, and runner
  implementation out of `quench-runtime`; keep the runtime limited to the
  neutral `QuenchRuntime` API (`eval`, `eval_module`, `eval_script`, module
  import, realm snapshots, property access, calls, and host callbacks). Do
  not export interpreter/environment internals to make the move compile.
  Inventory is complete: 17 runtime-side files remain. The first move exposed
  private intrinsic/interpreter state, `Context::env`, dynamic-import, and
  object-storage dependencies; replace these with neutral host-script and
  realm APIs before physically moving the tree. The physical move landed in
  `e824f88a`; neutral runtime APIs landed in `5ea5daa8`, and `quench-test262`
  passes its compile gate in `725737f5`. Runtime production code builds without
  `src/test262`; remaining work is removing Test262-specific nested unit tests
  from the runtime test target.

- **R38 — Module fixture identity.** Preserve canonical module identity for
  `-as.js` resolution fixtures and follow named re-export chains without
  changing Test262 inputs. Use a fresh digest to assess remaining failures;
  this plan does not retain conformance counts.

- **R39 — Long-stage conformance execution.** Keep stages 44, 53, and 65
  explicitly unresolved until complete digests are collected. Diagnose long
  stages with external subdirectory sharding or a longer-lived runner process;
  do not treat partial output or unit-test counts as conformance evidence.

- **R40 — Remaining unit gates.** The lower-switch pin, stale timeout probes,
  and async-generator dynamic-import queueing are resolved. Current module
  work has pins for lexical isolation, self-imported named and `export * as`
  re-exports, string export/import names, and indirect alias cycles. Remaining
  semantic gates are live imported bindings, TDZ during module instantiation,
  and top-level-await scheduling. Resolve them through runtime/module
  operations rather than runner-only special cases before claiming the
  conformance crate green.

- **R41 — Fresh conformance and performance baseline.** Run a complete digest
  from stage 0 before architectural changes. Record pass/fail/skip counts,
  wall time, RSS, worker count, and discovery, bootstrap, parse, execution,
  microtask, and cleanup timings outside `docs/` and `tasks/`.

- **R42 — Heap and execution boundaries.** Introduce concrete isolate-local
  ownership for heap allocation, object identity, execution frames, roots, and
  job state. Define opaque rooted handles for the Rust host API without
  exposing object layouts. Preserve the current collector and object layout;
  do not build a generic runtime-strategy framework.

- **R43 — Interpreter-first IR.** Add a compact IR for constants, locals,
  control flow, calls, property operations, throws, and suspension points.
  Add lowering incrementally and differential tests against the AST evaluator.
  Keep complex behavior in canonical runtime operations. Do not change the
  default execution path until parity is established and affected Test262
  stages pass.

- **R44 — Runtime LOC budget.** Keep `quench-runtime` production Rust under
  100,000 lines, excluding test modules. Measure with
  `rg --files crates/quench-runtime/src -g '*.rs' | grep -vE '(/tests\.rs$|/tests/)' | xargs wc -l`;
  the current baseline is below budget. Before adding an abstraction, remove
  or consolidate duplicate runtime code where possible; do not trade the LOC
  target for narrowly tailored conformance glue. Keep the Rust host API narrow
  and concrete; do not introduce generic heap, collector, or executor
  strategies without measured need.

- **R45 — Hot object layouts (Phase 3).** Only after Phase 2 IR parity and
  profiles identify property/array storage as the bottleneck, add immutable
  shapes with compact slots for ordinary objects, dictionary fallback for
  dynamic objects, and dense/holey/dictionary array representations. Preserve
  one canonical property-operation path and prove Test262 parity before the
  next phase.

- **R46 — Collector spike (Phase 4).** Run a bounded single-isolate MMTk
  integration spike. It must prove roots, write barriers, weak edges,
  ephemerons, host handles, `WeakRef`/`FinalizationRegistry` cleanup ordering,
  native-code safepoints, RSS, and complete Test262 parity before selecting a
  production collector or enabling multi-isolate deployment.

- **R47 — Entry-guarded native tier (Phase 5).** After the collector decision
  and benchmark evidence, lower only hot, IR-parity-proven functions to
  Cranelift. Check all specialization assumptions at entry; a failed guard
  restarts in the generic IR interpreter. Do not add OSR or mid-function
  deoptimization in this phase.

- **Future considerations — post-Phase 3.** Moving/generational GC follows a
  successful MMTk spike. A Cranelift entry-guarded tier follows IR parity and
  benchmark evidence; mid-function deoptimization and OSR remain deferred
  until that tier demonstrates a need. NaN-boxing and additional execution
  models remain measurement-driven.
