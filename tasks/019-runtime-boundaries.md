# 019 — Pure runtime and test262 runner boundaries

## Goal

Implement a pure JavaScript engine in `quench-runtime` with the pipeline
`OXC AST → compact Quench IR → interpreter`, generic over:

```text
Runtime<Heap, Collector, Allocator, Frames, Executor, Exceptions, Environments>
```

Implement `quench-test262` as a separate conformance runner that owns
metadata, harness loading, staging, process isolation, metrics, and reports.
The only connection is a host execution interface; the runner does not
inspect engine internals.

## Progress — 2026-08-07

- [x] Document the target boundaries in `docs/architecture.md`.
- [x] Record the canonical goal in `tasks/index.json`.
- [x] Identify the current runner as transitional under
  `quench-runtime/src/test262`.
- [x] Move the engine-facing `Test262Host` execution contract to the public
  `quench_runtime::host` API while retaining a compatibility re-export.
- [x] Name the post-OXC lowered representation `QuenchIr` and expose it as
  the parser/interpreter boundary; OXC AST values do not escape `parser.rs`.
- [x] Add the public generic `Runtime<Heap, Collector, Allocator, Frames,
  Executor, Exceptions, Environments>` façade with a production default
  component set.
- [x] Give each runtime parameter its own public subsystem trait and prove
  independent custom component selection with a unit test.
- [x] Make `Runtime` retain the selected subsystem instances, expose typed
  accessors, and provide `with_components`; defaults use the production
  component set.
- [x] Give `Executor` a behavior-bearing contract and route `Runtime::eval`
  and `Runtime::eval_es_module` through the selected executor instance.
- [x] Create `crates/quench-test262` with a host-only runner API and outcome
  mapping test; engine implementation types do not cross the boundary.
- [x] Add runner-owned frontmatter parsing for module dispatch and negative
  expectation metadata, with tests.
- [x] Classify expected negative errors in the runner while preserving the
  engine host contract as a simple success/error result.
- [x] Parse async, only-strict, and ordered harness-include metadata in
  `quench-test262`.
- [x] Add callback-based harness loading and deterministic include/strict
  composition before host dispatch.
- [x] Add runner-owned file loading with explicit UTF-8 I/O errors.
- [x] Add harness-aware file execution so disk-loaded tests use the same
  include/strict composition path as in-memory tests.
- [x] Add deterministic batch execution with `StageReport` counts and
  retained failure paths/reasons.
- [x] Add harness-aware batch execution so every staged file follows the
  metadata/include composition path before host dispatch.
- [x] Keep batch execution progressing across unreadable test files by
  recording file I/O errors as runner failures.
- [x] Add deterministic recursive JavaScript file discovery for stage input.
- [x] Remove uncompiled duplicate runner sources from `quench-test262`; its
  public host-facing runner API is now the single maintained boundary.
- [x] Specify the compact Quench IR ownership, indexing, interning, and
  cold-metadata contract in `docs/architecture.md`.
- [x] Add an owned `IrProgram` wrapper and parser entry point as the first
  migration step away from the compatibility alias.
- [x] Make `QuenchIr` name the owned IR type; legacy parser-returning APIs now
  state their `Program` return type explicitly.
- [x] Route `Context::eval` and `Context::eval_es_module` through the owned
  IR boundary before interpretation.
- [x] Route TypeScript/TSX evaluation through the same owned IR boundary.
- [x] Preserve the `IrProgram` type through the interpreter entry point;
  legacy `Program` access is now contained behind `eval_ir_program`.
- [x] Pack top-level IR statements into an owned boxed slice and execute that
  slice directly, beginning the low-RSS storage migration.
- [x] Record a Stage 44 native performance/conformance baseline.
- [ ] Move metadata, harness loading, stages, isolation, and metrics from the
  transitional runtime module into `crates/quench-test262`.
- [ ] Define the stable host API used by both the native runner and future
  embedders.
- [ ] Replace the compatibility `QuenchIr` alias throughout the runtime with
  `IrProgram`, then migrate its storage to compact owned representation
  (arena/index-backed where measurements justify it).
- [ ] Replace the remaining marker-only subsystem traits with behavior-bearing
  interfaces and wire each owned component into its runtime subsystem.
- [ ] Re-run staged test262 after each boundary extraction; no skips or
  undocumented compatibility paths.

## Evidence and gates

- Runtime: `cargo test -p quench-runtime`, `cargo clippy -p quench-runtime
  --all-targets`, and the relevant test262 stage.
- Runner: runner unit tests plus the same stage through the host interface.
- Architecture: no runner dependency on parser, IR, heap, builtin, or
  interpreter implementation types.
- Conformance: `tasks/index.json` remains the authoritative stage order and
  count source.

## Non-goals

- No Node adapter or Node-host compatibility layer.
- No edits under `tests/test262`.
- No speculative runtime abstraction without a production callsite and a
  test admitted by `AGENTS.md`.
