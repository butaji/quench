# Architecture work items

This is an implementation backlog, not a status ledger. Do not add pass
counts, stage totals, completion percentages, or skip lists here. Verify work
with the relevant commands and test262 runs at execution time.

## Representation boundary

- Define the runtime heap interface around `HeapRef(u32)` without introducing a
  second semantic object model.
- Move object access behind shape/slot operations; keep generic property
  semantics canonical and observable.
- Replace copied closure captures with shared indexed environments and explicit
  capture/update rules.
- Separate immediate values, heap references, frames, continuations, and
  completion state so their storage can be packed independently.

## Declarative generation

- Introduce one macro-owned declaration for runtime values, heap layouts, and
  tracing metadata.
- Introduce one `ops!` declaration for semantic operations and mechanically
  generated physical dispatch, verification, disassembly, and profiling hooks.
- Introduce declarative builtin and primordial metadata; retain complex builtin
  algorithms as readable Rust.
- Derive specialization guards and superinstructions from canonical semantic
  operations rather than adding parallel implementations.

## Execution performance

- Encode residual Ops compactly and keep interpreter dispatch on the physical
  operation path.
- Add measured property-site specialization: cold, monomorphic, polymorphic,
  and generic.
- Fuse frequent operation sequences only through generated composition of
  primitives.
- Add a baseline compiler only after profiling demonstrates sustained hot-loop
  demand; it must consume the exact residual Ops.

## Memory and RSS

- Remove avoidable `Rc`, `RefCell`, boxed trait objects, string-keyed maps, and
  duplicated metadata from the hot runtime path.
- Make heap references, slots, arrays, captures, shapes, and snapshots compact
  and relocatable.
- Drop OXC arenas after reduction unless source-level tooling explicitly needs
  retention.
- Measure cold start, peak RSS, allocation volume, and cache-sensitive runtime
  behavior before and after each representation migration.

## Engineering constraints

- Keep OXC as the only syntax and semantic frontend.
- Keep facts unified as `Proven`, `Guarded`, or `Unknown`.
- Never specialize through observable JavaScript behavior.
- Keep `quench-runtime` unaware of test262 and keep harness fidelity entirely in
  `quench-test262`.
- Preserve zero warnings, 500-line files, 40-line functions, and cognitive
  complexity ≤ 10 for every Rust change.
