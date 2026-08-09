# ADR 0005: OXC data, unified facts, and residual execution

- Status: accepted.

## Decision

Quench is a program reducer, not a traditional compiler pipeline. OXC owns
syntax, scopes, and symbols. Quench combines those ephemeral inputs with
TypeScript, JSDoc, declaration, profile, snapshot, and runtime observations in
one `ProgramDb` fact algebra, then emits only residual operations.

`Fact<T>` has exactly `Proven(T)`, `Guarded(T, Guard)`, and `Unknown` states.
Only proven facts may erase work. Guarded facts select a specialized residual
op only when the guard executes; unknown facts use generic semantics. No fact
may eliminate or reorder observable ECMAScript behavior.

The frontend is expressed through `value`, `place`, `effect`, `control`, and
`define` contexts. Their small semantic kernel composes load, store, property,
convert, binary, compare, call, construct, branch, allocate, suspend, and
complete. Physical ops, including quickened forms and measured
superinstructions, are generated from the same semantic declaration and have
no independent meaning.

`HeapRef(u32)`, tagged `Value`, shapes/slots, frames, code, facts, and shared
continuations are the core runtime data. `Completion` unifies abrupt control
semantics while ordinary PC progression remains the fast path. Primordials and
builtin metadata are generated static realm data; builtin algorithms are
ordinary Rust handlers.

Macro declarations own repeated facts and generate tags, layouts, tracing,
encoding, decoding, verification, descriptors, formatting, and snapshot
metadata. They must remain data declarations with named readable handlers,
not a hidden language.

## Consequences

- No internal AST clone, HIR/MIR ladder, permanent TypeGraph, or self-hosted
  builtin bootstrap is permitted.
- OXC arenas are dropped after reduction unless an explicit tooling feature
  requires retained source data.
- Shapes, type information, profiles, and snapshots are facts, not separate
  optimizer or type-runtime subsystems.
- A future baseline compiler may consume the same residual ops linearly. It is
  explicitly deferred and cannot introduce a second semantic representation.
- The test262 stage run remains the correctness gate. Generated code and
  specializations do not weaken observable-ordering requirements.

## Doctrine

1. Never represent the same semantic fact twice.
2. OXC owns syntax.
3. Static structure remains data or disappears.
4. VM code represents only dynamic uncertainty.
5. Semantic abstractions do not imply runtime allocations.
6. Share semantics; specialize physical execution.
7. One declaration generates every mechanical consequence.
8. Generated LOC is cheap; handwritten semantic LOC is expensive.
9. Facts are Proven, Guarded, or Unknown.
10. Never optimize through observable JS behavior.
11. Heap references stay compact.
12. No subsystem receives an independent universe without semantic need.
13. Types and profiles are facts, not runtimes.
14. If work can disappear before runtime, it must justify remaining.
