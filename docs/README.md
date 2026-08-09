# Documentation

The repository [README](../README.md) contains the current Quench doctrine.
The test262 runner remains in `crates/quench-test262` and its supporting tools;
the execution engine is intentionally being rebuilt around the doctrine.

Run `tools/lint-rust.sh` before committing. It enforces zero warnings, the
500-line file limit, the 40-line function limit, and cognitive complexity 10.

## Frozen performance and representation direction

Quench optimizes a deliberately measured frontier: the smallest possible
handwritten semantic core, V8-class execution on workloads where reduction or
compact specialization gives a structural advantage, and shockingly low cold
and peak RSS. It does not assume one configuration can match V8 on every
dynamic workload while also minimizing memory and implementation size.

Syntax, types, shapes, facts, semantic operations, and runtime metadata are
data. The VM represents only dynamic uncertainty. Declarative macros generate tags,
layouts, operations, dispatch, tracing, verification, and other mechanical
consequences. They do not form a second language for specification algorithms.
Generation is accepted only while binary text, static data, compile time, and
RSS remain within explicit budgets.

The implementation must preserve these boundaries while it evolves:

- OXC owns syntax, scopes, and symbols; Quench owns facts and reduction.
- Canonical completion-aware semantics land before specialized execution.
- Semantic operations are canonical; physical operations may be specialized,
  fused, encoded compactly, interpreted, or compiled from the same definition.
- Runtime objects use compact `HeapRef(u32)` handles, shape IDs, and slots.
- Closures use shared indexed environments rather than copied object graphs.
- Ordinary calls use compact stack frames; resumable continuations are
  materialized only at genuine suspension boundaries.
- The OXC arena is ephemeral unless source tooling explicitly requires it.
- Caches, interning, generated code, and native code are bounded and
  reclaimable where possible.
- Any specialization must preserve observable JavaScript behavior.

See [`architecture.md`](architecture.md) for the target layering and
[`../tasks/architecture.md`](../tasks/architecture.md) for the implementation
work items. The task file is a design backlog, not a progress ledger; test
results remain ephemeral.
