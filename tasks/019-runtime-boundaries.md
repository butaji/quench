# 019 — Pure runtime and test262 runner boundaries

## Goal

`quench-runtime` is a pure JavaScript engine
(`OXC AST → compact Quench IR → interpreter`), generic over
`Runtime<Heap, Collector, Allocator, Frames, Executor, Exceptions, Environments>`.
`quench-test262` is a separate conformance runner owning metadata, harness
loading, staging, process isolation, metrics, and reports. The only
connection is a host execution interface; the runner never inspects engine
internals.

## Status (2026-08-08)

Done: public `Test262Host` contract; generic `Runtime` façade with typed
subsystem accessors and `with_components`; `QuenchIr`/`IrProgram` owned-IR
parser entry points for all modes (script, module, TS, JSX) routed through
`Context::eval*`; `crates/quench-test262` created with runner-owned
frontmatter, harness composition, file I/O, batch execution with
`StageReport`, and recursive stage discovery; first IR storage step landed
(top-level statements packed into an owned boxed slice); Stage 44 baseline
recorded; repository-wide size gate enforced.

## Open

- [ ] Reduce the remaining Rust files over 500 lines (each split must also
  satisfy the 40-line function and complexity-10 gates).
- [ ] Move metadata, harness loading, stages, isolation, and metrics from
  the transitional `quench-runtime/src/test262` into `crates/quench-test262`.
- [ ] Define the stable host API used by both the native runner and future
  embedders.
- [ ] Replace the compatibility `QuenchIr` alias with `IrProgram`
  throughout, then migrate storage to the compact owned representation
  (arena/index-backed where measurements justify it).
- [ ] Replace the remaining marker-only subsystem traits with
  behavior-bearing interfaces, wired into their runtime subsystems.
- [ ] Re-run staged test262 after each boundary extraction; no skips or
  undocumented compatibility paths.

### Runner tooling (absorbed from harness-roadmap)

- [ ] `TEST262_CLUSTER=<substring>` — filter digest to one failure group.
- [ ] `tools/diff-digest.sh` — before/after diff of `failures-N.json`.
- [ ] Per-stage progress fields in `tasks/index.json` for `stage-status.sh`.
- [ ] Eliminate crash-file skips (larger test-thread stack or fix
  recursion in class/prototype paths); skip count → 0.
- [ ] Persistent worker pool for isolated runs.

## Evidence and gates

- Runtime: `cargo test -p quench-runtime`, `cargo clippy -p quench-runtime
  --all-targets`, and the relevant test262 stage.
- Runner: runner unit tests plus the same stage through the host interface.
- Architecture: no runner dependency on parser, IR, heap, builtin, or
  interpreter implementation types.
- Conformance: `tasks/index.json` remains the authoritative stage order.

## Non-goals

- No Node adapter or Node-host compatibility layer.
- No edits under `tests/test262`.
- No speculative runtime abstraction without a production callsite and a
  test admitted by `AGENTS.md`.
- No parallel *stage* execution (hides regressions), no feature skip
  lists, no hand-maintained failure markdown (JSON is the source of truth).
