# V8-v7 board on v2-gc after the trunk merge and today's memory changes

Binary `qf1`: v2-gc `3e4282416d`, static. It contains trunk `d9dc8f883f` (including c2's lazy
native table), the lazy memo tables, the first-call release, the policy-selected allocator
and the inline-first field-add sets. It is bolted with a profile retrained on itself.

Board: `quench-bench/v8-v7-rss.cjs`, MemoryFirst, 3 runs, evicted binaries, idle host. The
other engines are QuickJS-ng 0.10.1, Node 22.22.0 and Bun 1.4.2.

`gaps.txt` lists each factor: Quench's worst sample over 0.8 × the lowest other engine's best.
The previous board (`tod137-board-after-replace-and-jit-release-…`, 2026-10-22) is shown in
brackets:

| Fixture | No JIT | `--jit` |
| -- | --: | --: |
| DeltaBlue | 1.10 (1.13) | 1.06 (1.08) |
| Richards | 1.18 (1.16) | 1.21 (1.13) |
| Crypto | 1.25 (1.28) | 1.34 (1.41) |
| RayTrace | 1.35 (1.32) | 1.30 (1.11) |
| RegExp | 1.19 (1.22) | 1.16 (1.19) |
| NavierStokes | 1.21 (1.15) | 1.86 (1.84) |
| EarleyBoyer | met (0.49) | met (0.48) |
| Splay | met (0.64) | met (0.64) |

With three runs, the worst sample moves by about ±200 KiB. RayTrace `--jit` and Richards
`--jit` read worse here, but in the matched 7-run A/Bs their medians were 3330 and about
3050 KiB, the same as on the old board.

The target is still not met on six fixtures. What remains is the start-up anonymous state
(runtime realm and host bootstrap, TOD-21) and JIT compile scratch for NavierStokes (TOD-110).
