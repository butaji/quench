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
- [x] Record a Stage 44 native performance/conformance baseline.
- [ ] Extract the transitional runner into `crates/quench-test262`.
- [ ] Define the stable host API used by both the native runner and future
  embedders.
- [ ] Replace the compatibility `QuenchIr` alias with compact owned storage
  (arena/index-backed where measurements justify it).
- [ ] Introduce runtime subsystem traits incrementally, each with a failing
  unit test and a real production callsite; replace marker components with
  owned subsystem implementations as each migration lands.
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
