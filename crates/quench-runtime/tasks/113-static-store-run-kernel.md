# 113 — Static-property store-run kernel

Status: complete

Recognize the general block grammar
`(LoadLocal(object); LoadLocal(value); SetStatic)*; Return` with at least one store.
Lower it to one copied block connector calling one shared immutable Rust/LLVM kernel,
instead of interpreting every member through the generic opcode switch. The repetition
count, local slots, registers, property keys, and return operand remain bytecode data;
there is no benchmark/source-name or hotness condition.

The experiment preserved the single semantic source by factoring canonical local-load,
static-store, and return primitives out of `execute`. It validated every receiver before
mutation, folded the store primitives, and performed the canonical return. The repeated
store morphism was therefore lifted into one block morphism with the ordinary connector
ABI on both sides. Selection depended only on bytecode structure.

Evidence: `reports/semantic-block-profile.jsonl` records 584,480 entries for the two-store
member of this grammar, making it the highest-frequency remaining compact property block.

## Result

The implementation was fully wired, not merely present:

- one- and two-store structural selector tests passed, with mismatched operands and a
  missing return rejected;
- a native-entry integration test proved both properties were updated, the undefined
  result was returned, and a nullish receiver rejoined the canonical error path;
- all 48 release tests and the complete eight-suite smoke passed;
- the linked block copied the general connector and patched only its bytecode PC and the
  shared immutable kernel address.

Exact alternating six-run A/B is in
`reports/static-store-run-ab-6/comparison.txt`:

| Suite | Baseline | Candidate | Change |
|---|---:|---:|---:|
| Richards | 482.5 | 483.0 | +0.10% |
| DeltaBlue | 444.5 | 445.5 | +0.22% |
| Crypto | 1038.5 | 1003.5 | -3.37% |
| RayTrace | 1093.0 | 1082.5 | -0.96% |
| EarleyBoyer | 1672.5 | 1652.5 | -1.20% |
| RegExp | 228.0 | 226.0 | -0.88% |
| Splay | 1942.0 | 1943.0 | +0.05% |
| NavierStokes | 1228.5 | 1218.0 | -0.85% |
| **Aggregate** | **828.257** | **821.086** | **-0.87%** |

Rejected and reverted. The rebuilt binary is byte-identical to the accepted Task 110
baseline (`a239bed0433b589ed5efcd9f41029041db382e7c76c566052c196989a6887bed`).

## Learned constraint

Do not create another Rust helper that decodes the same `DynInstr` sequence already
handled by the inlined block loop. A useful coarser morphism must erase runtime bytecode
decoding and redundant validation, for example by copy-patching operands directly into
rustc/LLVM-produced machine code or by linking a predecoded immutable operand plan. Merely
renaming an interpreted run as a kernel changes abstraction level without changing the
work the CPU performs.
