# M4 stock Splay retake after IteratorClose fix

This is an 11-round stock-harness Splay result for the production binary built
from trunk commit `17077748a9b5f060e107a9926756e90047467b89`. It is a
single-fixture retake, not a full Task 61 qualification.

## Reproduction and provenance

- Platform: arm64 macOS, `Mac16,10`, 10 logical CPUs, 16 GiB RAM.
- Runtime source: `17077748a9b5f060e107a9926756e90047467b89`.
- Quench production executable SHA-256:
  `8432da1159e301d021c236148a4f4a13e5f1b8de40fb87893fb54a2ebf5795dd`.
- Runner: `quench-bench` schema 5, SHA-256
  `bdc9d6c4afc537fe8ed30d4fccba9157cd4232e345ce75810465a0fff98db109`.
- Fixture corpus: `64e1860c736c1b899708f4cd646721bc71e53d8b`.
- Competitors: QuickJS 2026-06-04; Bun 1.4.2 with `BUN_JSC_useJIT=0`;
  Node v26.10.0 with `--jitless`. The JSON records executable hashes and
  environment for all engines.
- The report is complete, all 11 rounds are valid, and outputs are equal.
  Its `qualification_ready` is false because this is a one-fixture campaign.
- Across the 22 before/after host snapshots, one-minute load ranged from
  2.01 to 2.81 on 10 logical CPUs (0.201–0.281 per CPU); none exceeded the
  0.32-per-CPU clean-window threshold. Five- and fifteen-minute averages still
  include earlier host activity.
- Raw report SHA-256:
  `0f2ef8f66a20aab2425da92c3ffedee51617f2d3b910d3870dd0e71ee198d57a`.

## Results

| Engine | Median stock Score | Median max RSS |
| -- | --: | --: |
| Quench | 8,888 | 116,621,312 B |
| QuickJS | 8,622 | 151,060,480 B |
| Bun, JIT disabled | 9,926 | 64,438,272 B |
| Node `--jitless` | 14,791 | 407,846,912 B |

Node has the best reference Score, and Bun has the lowest reference max RSS.
The M4 Splay distances are therefore **1.664x on Score**
(`14,791 / 8,888`) and **1.810x on max RSS**
(`116,621,312 / 64,438,272`). Splay has not won either bar.

Raw report: [stock-11.json](stock-11.json).
