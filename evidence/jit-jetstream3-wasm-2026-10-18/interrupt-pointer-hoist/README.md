# Loop interrupt pointer-hoist experiment

This experiment tested marking the stable `CallContext.interrupt` pointer load
as Cranelift-invariant while retaining the atomic byte load at every safepoint.
Generated x86-64 code hoisted the pointer field load into a callee-saved
register and kept fresh atomic loads at both the pre-call and loop-latch polls.
The focused loop-interrupt test passed, but the speed effect did not survive
the benchmark screen, so the production source keeps the original trusted
load flags.

The controlled baseline and confirmation reports each contain 11 valid
QuickSort rounds with matching output. They use the same source except for the
single memory-flag change. The baseline binary hash is
`efaaabbd6b2a97003b1227c575392d729ec6611d99f6797a2732762a2e9614f8`; the
candidate hash is
`a71caa754865df38446ba94731c80f8e0d2b88d1eb01485c0a9e4d5615d8b5c3`.

| Screen | Quench Score / RSS MiB | Bun Score / RSS MiB | Node Score / RSS MiB |
| --- | ---: | ---: | ---: |
| Baseline, 11 rounds | 241.31 / 42.72 | 372.45 / 47.23 | 481.14 / 88.40 |
| Candidate, 11-round confirm | 214.52 / 48.05 | 356.84 / 47.31 | 473.72 / 88.44 |

The candidate's median Score was 11.1% lower than this baseline; the
independent bootstrap 95% interval for the candidate/baseline Score ratio was
0.832–1.008, so it does not establish a score effect. Its QuickSort RSS was
5.3 MiB higher in this pair, but two earlier candidate screens measured
42.44–42.57 MiB, showing substantial run-window variation. Pooling the three
candidate screens gives 206.94 Score / 42.74 MiB; the baseline screen gives
241.31 / 42.72 MiB. These runs do not show a performance or memory win, and
the hoist was dropped.

Raw reports are `quicksort-baseline-11-round.json`,
`quicksort-candidate-confirm-11-round.json`, and the two preceding candidate
screens. The binaries share the same Wasm cache and re-entrant GC safety
changes; only the interrupt pointer-load memory flags differ in the controlled
pair.
