# Splay speed attribution after M1 — 2026-10-09

This diagnostic replaces the hand counts in the loop note. It is attribution, not a performance gate. The measured production executable is the pinned M1 candidate `d28c6a72…` (SHA-256 recorded in `per-op.json`). The profiler binary is pinned at `target/pinned/042e54c9…`, built before M1. M1 changes GC root derivation and does not change the fixture's bytecode or dispatch-counter sites; the physical counts are therefore invariant across that patch. The materialized Splay input and both executable hashes are recorded in `splay-one-run-opcode-census.json`.

The opcode census subtracts one setup-only process from one setup-plus-one-`run()` process. It records 191,448 physical dispatches per Splay `run()` (80 insert/remove iterations):

| Work | Physical operations per run | Per 80 iterations |
| --- | ---: | ---: |
| `Call` | 6,961 | 87.0 |
| `Construct` | 480 | 6.0 |
| `GetField` | 21,933 | 274.2 |
| `SetField` | 0 | 0 |
| `SetFieldStrict` + `SetThisFieldStrict` | 7,938 | 99.2 |
| `MakeObject2` | 2,480 | 31.0 |
| `SuperConstArrayObject2` | 2,560 | 32.0 |

The new CJS-shaped microloops include a local-store control, then field read/write, local call, construct, two-key object literal, and nested-array object literal. Active-process counters subtract the corresponding zero-work process and divide by loop iterations. Three repetitions were run against Quench M1, rqj, qjs, Node `--jitless`, and Bun with JIT disabled. Raw samples and script hashes are in `per-op.json`.

To rank operations against the Splay bar engine, the stable instruction estimate subtracts the same engine's local-store row from each operation row, then compares Quench to Node `--jitless`. Multiplying by the Splay one-run opcode count gives a non-additive exposure estimate:

| Family | Quench / Node instructions per iteration | Net excess after each engine's local-store baseline | Count/run | Weighted excess instructions/run |
| --- | ---: | ---: | ---: | ---: |
| Local call | 3,026 / 374 | 1,768 | 6,961 | 12.30M |
| Nested-array literal | 3,763 / 597 | 2,284 | 2,560 | 5.85M |
| Field read | 1,317 / 198 | 236 | 21,933 | 5.18M |
| Two-key literal | 2,723 / 524 | 1,315 | 2,480 | 3.26M |
| Construct | 7,679 / 582 | 6,214 | 480 | 2.98M |
| Field write | 1,481 / 227 | 370 | 7,938 | 2.94M |

These products rank likely work; they do not sum to the fixture's total because the microloops include shared loop and operation setup, and compound paths overlap (construction includes property writes; nested literals include array creation). They put JS call/frame work first, then leaf allocation, then field reads. That agrees with the existing M1 sample, whose hot stacks cluster in `call_user_frame_mode` and `run_frame_general_until`; the sample predates this diagnostic and is not counted as a new profile.

Cycle attribution is provisional. On the 2M nested-literal row, Quench ranged from 1,020–1,176 cycles/iteration and Node ranged from 109–130 while retired instructions stayed within 0.5% and 1.3%, respectively. Immediately after the screen, host load average was 20.58 and several Deno/Node processes were consuming CPU. Treat `cycles_elapsed` in the raw file as diagnostic only; the stable retired-instruction rows support triage, not a keep/reject decision. Re-run the cycle rows on an idle host before using them to select a small speed change.

The active M1 Splay distance remains the matched 11-round result in `task61-splay-step0-2026-10-09/gc-roots-candidate/summary.json`: 9,395,225 median marginal cycles/run versus the Node target 4,590,844 (2.05x; 4,804,381 cycles/run gap), and 217,939,968 bytes versus the Bun target 57,950,208 (3.76x; 159,989,760-byte gap). This attribution run changes no code and makes no new performance claim.
