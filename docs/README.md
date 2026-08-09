# Documentation

The repository [README](../README.md) contains the current Quench doctrine.
The test262 runner remains in `crates/quench-test262` and its supporting tools;
the execution engine is intentionally being rebuilt around the doctrine.

Run `tools/lint-rust.sh` before committing. It enforces zero warnings, the
500-line file limit, the 40-line function limit, and cognitive complexity 10.

## Frozen performance and representation direction

Quench optimizes for three properties together: the smallest possible
handwritten semantic core, V8-class execution performance on representative
workloads, and the minimum practical memory/RSS footprint.

The design is intentionally LISP-like: syntax, types, shapes, facts, semantic
operations, and runtime metadata are data. Combinators operate on that data;
the VM represents only dynamic uncertainty. Heavy declarative macros are the
preferred tool for generating tags, layouts, operations, dispatch, tracing,
verification, and other mechanical consequences. Generated LOC is cheap;
duplicated handwritten semantic LOC is not.

The implementation must preserve these boundaries while it evolves:

- OXC owns syntax, scopes, and symbols; Quench owns facts and reduction.
- Semantic operations are canonical; physical operations may be specialized,
  fused, encoded compactly, interpreted, or compiled from the same definition.
- Runtime objects use compact `HeapRef(u32)` handles, shape IDs, and slots.
- Closures use shared indexed environments rather than copied object graphs.
- The OXC arena is ephemeral unless source tooling explicitly requires it.
- Any specialization must preserve observable JavaScript behavior.

See [`architecture.md`](architecture.md) for the target layering and
[`../tasks/architecture.md`](../tasks/architecture.md) for the implementation
work items. The task file is a design backlog, not a progress ledger; test
results remain ephemeral.
