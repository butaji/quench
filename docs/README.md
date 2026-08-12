# Documentation

The repository [README](../README.md) contains the current Quench doctrine.
The test262 runner remains in `crates/quench-test262` and its supporting tools;
the execution engine is intentionally being rebuilt around the doctrine.

Run `tools/lint-rust.sh` before committing. It enforces zero warnings, the
500-line file limit, the 40-line function limit, and cognitive complexity 10.

## Frozen machine-first direction

Quench is designed for one thing: make the machine execute as few bytes, loads,
branches, allocations, and dispatches as possible while preserving JavaScript
observability. Reduction is the primary optimizer. Runtime code is the residue
of uncertainty, not a general-purpose JavaScript implementation.

The non-negotiable representation is:

```text
OXC data -> facts -> flat Code/Op tables -> HeapRef(u32) values -> shape/slot heap
                                      -> fixed stack frames -> bounded slow paths
```

There is one semantic path, one residual instruction vocabulary, and one
compact interpreter in this phase. No JIT, native backend, alternate IR,
self-hosted builtin universe, shadow AST, or parallel optimizer is allowed.

Every hot-path decision is data-driven and measurable. The budget includes
instructions, branches, allocations, live bytes, peak RSS, binary text, static
data, cache bytes, generated code, generated source, handwritten source, and
compile-time memory. An optimization that improves throughput while inflating
the resident set or duplicating semantics fails the design review.

The implementation must enforce these boundaries:

- OXC owns syntax, scopes, and symbols; Quench stores only indexed facts.
- Objects are `HeapRef(u32) -> ShapeId + packed slots`; no string-keyed object
  vectors on the hot path.
- Ops are flat fixed-width opcode words with side tables for uncommon
  operands; nested `Vec<Op>` is a compiler temporary, never the runtime
  format.
- Calls use fixed stack frames and register windows; continuations contain only
  live state at genuine suspension points.
- Property, call, conversion, construction, iteration, descriptor, equality,
  and completion protocols each have exactly one semantic owner.
- Keys, shapes, code, environments, and metadata use bounded per-program or
  per-realm tables and are reclaimable.
- Generated declarations own all mechanical consequences; readable Rust owns
  observable algorithms.
- Repeated JavaScript literals and intrinsic metadata are declared once in a
  canonical table or macro; consumers use generated IDs and accessors, not
  copied strings. Literal declarations must not become a second semantic DSL.
- Generic semantics precede guards; every guard falls back to the same generic
  operation without changing ordering or observability.

See [`architecture.md`](architecture.md) for the target layering and
[`../tasks/architecture.md`](../tasks/architecture.md) for the implementation
work items. The task file is a design backlog, not a progress ledger; test
results remain ephemeral.
