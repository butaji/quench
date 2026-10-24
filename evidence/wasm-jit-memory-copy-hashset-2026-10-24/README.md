# Wasm JIT memory.copy: HashSet score vs compile peak (shelved)

Host: Linux x86_64 cloud container, JetStream 3 via quench-node `--jit`, base aa58e13e1, 2026-10-24.
`memory-copy.patch` lowers `memory.copy` within the one directly addressed memory
(both ranges checked, then a `ptr::copy` leaf), with an interpreter-oracle test.

HashSet-wasm, 5-run paired medians (`medcmp.py`): score 24.61 -> 43.98, max RSS 26100 -> 125780 KiB.
The gain and the peak both come from compiling `main` (stats function 3, 66145 bytecodes,
384131 CLIF instructions, 23913 blocks, 1.14 MB code). Phase probes (RSS/HWM KiB):

| point | RSS | HWM |
|---|---|---|
| emitter start | 20880 | 22164 |
| IR emitted | 67300 | 67300 |
| finalized | 63568 | 67788 |
| after optimize (probe copy) | 89616 | 108564 |
| after compile | 79480 | 126192 |

Rejected mitigations: single_pass regalloc (88 MiB, score 26), opt_level none (148 MiB),
one shared unwind block (236 MiB, score 25), dropping FunctionBuilderContext early (no change).
Leaving `main` interpreted (bytecode cap 60000) keeps score 24 and RSS 19-20 MiB.
Not landed: the score still loses to Bun JIT (~102) while RSS would lose too (Bun ~54 MiB).
It lands with a baseline Wasm tier that compiles huge functions cheaply.
