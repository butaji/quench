# ADR 0004: One compact executable stream

- Status: Accepted
- Date: 2026-08-23
- Implements: Task 102; ADR 0003 rule 84

## Starting execute path

At the start of this change, `Value` is a 32-byte enum and registers are
`Vec<Value>`. `TaggedValue` is an unused prototype. `ObjectData` and
`ArrayData` retain their current semantic stores. `Machine` owns a
`RegisterWindow`, a `CodeId`, and a `pc`, while ordinary calls still pass
through `execute_frames`.

The compiler produces nested `Vec<Op>`. `CodeArena` recursively rehomes nested
`FunctionCode`, flattens every body into one `Vec<Op>`, and records
`CodeRange`s. Frozen `CodeStore` owns `Rc<[Op]>`.
`run_ops_completion_step_from` obtains `&[Op]`, scans it, and dispatches by
matching each 152-byte `Op`. Thus strings, vectors, and nested-code metadata
occupy the fetch stream even when dispatch needs only an opcode and registers.

## Decision

`CodeStore` owns exactly one executable representation:

```text
instructions: Rc<[Instruction]>     // 8-byte fetch records
cold: Rc<[Op]>                      // semantic payload indexed by Slow.a/b
ranges: Rc<[(u32, u32)]>            // checked views into instructions
```

`Instruction` is the only fetch unit. Its opcode, flags, and three `u16`
operands are generated from the opcode declaration in `ir.rs`. Every source
`Op` is consumed while freezing `CodeArena`; it is never retained by
`CodeStore`.

The cold table preserves the complete source operation only when compact
operands cannot express its semantics. Its entry is addressed directly by the
same absolute `pc`; dispatch does not scan it. Fast instructions have no cold
payload. Slow instructions have one cold payload addressed by a packed `u32`
index, and absence is invalid.

Nested bodies are already rehomed to `CodeRange` before lowering. Therefore a
cold operation contains ranges into the same frozen store, not a second code
tree. Constants and uncommon names remain out of line.

## Ownership and lifecycle

`CodeArena` exclusively owns mutable compiler output. `freeze` lowers each
operation once, moves required semantic payloads into the cold table, and
drops the source `Vec<Op>`. The resulting immutable arrays share the lifetime
of `Rc<CodeStore>`. `FunctionCode` owns only a store reference and a checked
range.

Invalid states are: an unknown opcode, a range outside its code allocation, a
slow instruction without its matching cold payload, or a fast instruction
with operands that violate its opcode contract. Construction and focused tests
reject these states before execution.

## Consequences

A sequential fetch touches 8 bytes, giving eight instructions per 64-byte
cache line instead of 0.42 `Op`s. Dispatch is `instructions[pc]` followed by
an O(1) opcode match. JavaScript semantics remain in the existing operation
handlers reached through the cold payload; this is a storage and dispatch
change, not a second semantics implementation.

## Evidence

Both measurements used the `bench-throughput` profile and three runs per V8-v7
fixture on 2026-08-23. The scored-suite geometric mean increased from 32.30 to
36.92 (+14.3%).

| Fixture | Before score | After score | Before wall ms | After wall ms |
| --- | ---: | ---: | ---: | ---: |
| Richards | 18.9 | 22.1 | 7,844 | 6,726 |
| DeltaBlue | 17.2 | 18.5 | 13,861 | 12,867 |
| RayTrace | 32.5 | 34.7 | 75,245 | 70,348 |
| Splay | 103 | 131 | 9,635 | 7,742 |

Crypto, EarleyBoyer, and RegExp retained their 120-second timeout
classification. Navier-Stokes retained its no-score exit and improved from
73,831 ms to 72,782 ms.

A separate three-run Richards counter measurement compared the transitional
compact-fetch/dual-store checkpoint with the final single-store executor.
Median wall time fell from 8.06 s to 6.71 s and retired host instructions fell
from 165.3 billion to 133.9 billion (-19.0%). Median maximum RSS increased from
74.4 MB to 77.7 MB (+4.4%); removing the duplicate `Rc<[Op]>` therefore did not
produce a process-level RSS win in this sample. The measured throughput and
instruction-count win is accepted, while further cold-payload packing remains
necessary for an RSS reduction.
