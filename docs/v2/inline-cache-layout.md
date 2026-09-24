# Inline-cache layout audit

rqj's inline caches are runtime state while its residual bytecode is immutable,
serializable data. Any claim that moving state “next to” an instruction removes
an indirection must account for that ownership boundary and the actual release
fast path.

## Current access

A field instruction carries a compiler-issued `u16` site. VM initialization
allocates one flat, exactly-sized cache vector. The monomorphic read path uses
`get_unchecked(site)` under that compiler/size invariant, so release execution
pays no bounds check and follows no per-site pointer. Address formation is the
cache vector's base plus a scaled site index. The megamorphic table is touched
only after the monomorphic shape misses.

Method sites similarly index one flat array of two-entry cache sets. Their
entries contain runtime `Value`/`CallTarget` state and therefore cannot live in
the immutable residual without interior mutability and a runtime ownership
layer.

## Measured footprint

The profile-memory build reports the following retained execution state; the
instruction counts come from the same residuals.

| Workload | Instructions | Field-cache bytes | Method-cache bytes |
| --- | ---: | ---: | ---: |
| Richards | 1,336 | 4,596 | 4,096 |
| DeltaBlue | 2,121 | 7,500 | 10,304 |
| Splay | 1,083 | 3,300 | 3,328 |
| Crypto | 6,358 | 14,316 | 23,360 |

`Instr` is a fixed 12-byte record. The smallest literal adjacency candidate,
adding only the 8-byte monomorphic `FieldCache` to every fixed instruction,
adds 10,688 / 16,968 / 8,664 / 50,864 bytes respectively. That already exceeds
the complete current field table by 2.3–3.6x and does not include method state.
Using a union large enough for the two-entry method cache would make every
instruction pay the largest variant.

## Rejected alternatives

- A per-function cache vector remains a side table with the same base-plus-index
  address, while adding allocations and bases.
- A copied mutable execution image duplicates the residual instruction stream
  and makes serialization/liveness/debug metadata refer to a second PC space.
- Variable-width cache extension words avoid widening cache-free instructions,
  but remove fixed-width PC indexing and require branch/handler relocation plus
  a size decode in dispatch.
- Storing a pointer in each instruction replaces scaled indexing with a larger
  instruction and still dereferences separate cache storage.

Task 95 also measured the narrower prerequisite of making more property
instructions share a receiver representation: it was Score-neutral and added
32 KiB Richards RSS. There is no evidence that a larger mutable execution
image can recover that page cost.

## Decision

Keep cache state in the single flat VM-owned arrays. The instruction's site ID
already directly addresses those arrays, matching the useful principle behind
feedback slots without copying another engine's variable-width bytecode
layout. Reopen only with a compact variable-width decoder whose complete
instruction-plus-cache image is smaller and wins an exact cumulative gate.
