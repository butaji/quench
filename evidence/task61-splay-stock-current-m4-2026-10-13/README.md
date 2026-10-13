# Current-trunk Splay stock attempt (M4)

This is a Splay-only 11-round stock-harness attempt on the current
`v2-merged` runtime source, `4062a6be9` (published to `v2-merged` with
evidence-only commits through `bd14b29b3`). The run is preserved but excluded
from the Splay scoreboard because host contention entered during the campaign.

## Identity

- Platform: Apple M4 arm64 macOS, 10 logical CPUs, 16 GiB physical memory.
- Quench source: `4062a6be95e53bc600747a72f19c24efa7f4fb61` runtime tree; the
  report source revision is `bd14b29b3439f6b326173829e6bcf2f19ff75725`, whose
  intervening changes are evidence-only.
- Quench binary SHA-256: `098cc7aa69195bab87115b496a4df9660f78c5d7364e60a52c8761f2e857f5e6`.
- Runner SHA-256: `bdc9d6c4afc537fe8ed30d4fccba9157cd4232e345ce75810465a0fff98db109`.
- Raw report: `splay-stock-11.json`, SHA-256
  `1ab945f49ad3f5a56839f90e37206bd3bc89f087031bd555cb931e8f16431242`.
- 11/11 rounds valid and output-equal. `qualification_ready` is false because
  this intentionally selected only Splay from the eight-fixture corpus.

## Contention and exclusion

Round 0 started with one-minute load 2.43 and no Deno worker among the top CPU
consumers. Deno first appeared in the host snapshots during round 4. It was
visible from rounds 5–10, joined by Bun workers in later rounds, and load rose
to 5.75 across 10 logical CPUs. The Quench Score fell from 9,123 in round 0 to
4,225 in round 9 and 5,215 in round 10. This is a substantial, engine-sensitive
host disturbance, so the full campaign is excluded; its medians are not
qualification measurements and do not update either Splay distance.

The raw report contains before/after load and top-CPU-consumer snapshots for
each engine sample. The same-campaign medians are retained only for audit:

| Engine | Median Score | Median max RSS |
| -- | --: | --: |
| Quench | 9,092 | 117,587,968 B |
| Node `--jitless` | 14,660 | 406,159,360 B |
| Bun no-JIT | 9,812 | 65,945,600 B |
| QuickJS | 8,532 | 151,044,096 B |

The last valid official M4 Splay row remains the earlier snapshot in TOD-7.
This attempt measured the current trunk binary but cannot replace that row.
