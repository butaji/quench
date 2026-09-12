# 114 — Dead local/local numeric condition stencils

Status: complete

Promote the general four-op block grammar
`LoadLocal(left); LoadLocal(right); Binary(compare); JumpIfFalse` to rustc/LLVM-cooked
block stencils when all three temporary registers are dead outside their defining uses.
Provide the complete numeric comparison family (`==`, `!=`, `<`, `<=`, `>`, `>=`),
selected only from bytecode structure and operand topology.

This is a real higher-level morphism rather than another decoding kernel: the immutable
AOT template directly reads the two fixed local slots from `InlineSite` metadata, guards
bounds and numeric tags, computes the comparison, and tail-branches to the copy-patched
taken or sequential continuation. It emits no dead register stores and uses the existing
shared semantic kernel only on a guard miss. Every variant is macro-generated from one
semantic/codegen schema.

Evidence: `reports/semantic-block-profile.jsonl` contains 196,076 executions of the
`Binary:Gt` member alone, plus smaller local/local comparison members. The form is common
for loop bounds and object-algorithm predicates and is not tied to a source file or suite.

## Result

The complete six-variant family was implemented from one Rust macro. Structural tests
covered all comparisons, deadness, topology, and destination alias rejection. Extractor
tests proved next/slow/taken relocations, all 47 release tests passed, and the complete
V8v7 smoke passed. Candidate SHA-256 was
`2b14204febcab1e35645736dbc10e3fc56afee57e350eea52812edfc64af3294`.

The exact alternating six-run comparison is in
`reports/dead-local-condition-ab-6/comparison.txt`:

| Suite | Baseline | Candidate | Change |
|---|---:|---:|---:|
| Richards | 489.0 | 478.0 | -2.25% |
| DeltaBlue | 428.0 | 441.5 | +3.15% |
| Crypto | 1013.0 | 1024.5 | +1.14% |
| RayTrace | 1071.0 | 1081.5 | +0.98% |
| EarleyBoyer | 1657.5 | 1611.5 | -2.78% |
| RegExp | 221.0 | 209.0 | -5.43% |
| Splay | 1791.5 | 1830.0 | +2.15% |
| NavierStokes | 1157.0 | 1113.5 | -3.76% |
| **Aggregate** | **802.728** | **795.567** | **-0.89%** |

Because the run suffered visible machine-wide throttling, the dominant target was also
remeasured alone for eight alternating 500 ms repetitions. Navier-Stokes still regressed
from 1166 to 1151 (-1.29%); see
`reports/dead-local-condition-navier-ab-8/comparison.txt`.

Rejected and reverted. The rebuilt binary is byte-identical to the Task 110 baseline
(`a239bed0433b589ed5efcd9f41029041db382e7c76c566052c196989a6887bed`).

## Codegen finding

The cooked local/local `greater` stencil is 104 bytes (26 AArch64 instructions). Its
disassembly contains two local-bound guards, two number-tag guards, the floating compare,
and continuation-address reconstruction. That is too much repeated boundary proof for a
four-op region and loses to the compact shared block loop. The next experiment should
move statically guaranteed local-slot validity into the input context/type of every AOT
stencil, validating it once at `DynJitCode::build`, so individual morphisms do not reprove
the same invariant.
