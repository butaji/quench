# Current-trunk Splay stock retakes (M4)

These Splay-only 11-round stock-harness retakes use current `v2-merged` runtime
source `4062a6be9` (later commits through `c0163e7ea` publish evidence only).
The first attempt is excluded for contention. The second is clean and updates
the M4 Splay distances; neither bar is won.

## Identity

- Platform: Apple M4 arm64 macOS, 10 logical CPUs, 16 GiB physical memory.
- Quench source: `4062a6be95e53bc600747a72f19c24efa7f4fb61` runtime tree; the
  report source revision is `bd14b29b3439f6b326173829e6bcf2f19ff75725`, whose
  intervening changes are evidence-only.
- Quench binary SHA-256: `098cc7aa69195bab87115b496a4df9660f78c5d7364e60a52c8761f2e857f5e6`.
- Runner SHA-256: `bdc9d6c4afc537fe8ed30d4fccba9157cd4232e345ce75810465a0fff98db109`.
- Excluded first attempt: `splay-stock-11.json`, SHA-256
  `1ab945f49ad3f5a56839f90e37206bd3bc89f087031bd555cb931e8f16431242`.
- Clean retake: `splay-stock-clean-11.json`, SHA-256
  `34898f8bc56dff8d32c094ef90165749565e95378d6fd643f6c4610bdac7cbcb`.
- Both reports have 11/11 valid and output-equal Splay rounds.

## Contention and exclusion

Round 0 started with one-minute load 2.43 and no Deno worker among the top CPU
consumers. Deno first appeared in the host snapshots during round 4. It was
visible from rounds 5–10, joined by Bun workers in later rounds, and load rose
to 5.75 across 10 logical CPUs. The Quench Score fell from 9,123 in round 0 to
4,225 in round 9 and 5,215 in round 10. This is a substantial, engine-sensitive
host disturbance, so the full campaign is excluded; its medians are not
qualification measurements and do not update either Splay distance.

The raw report contains before/after load and top-CPU-consumer snapshots for
each engine sample. The excluded attempt's medians are retained only for audit:

| Engine | Median Score | Median max RSS |
| -- | --: | --: |
| Quench | 9,092 | 117,587,968 B |
| Node `--jitless` | 14,660 | 406,159,360 B |
| Bun no-JIT | 9,812 | 65,945,600 B |
| QuickJS | 8,532 | 151,044,096 B |

## Clean Splay retake and distances

The clean retake completed 11/11 valid, output-equal rounds. The highest
sampled one-minute load was 3.08 across 10 logical CPUs (0.308 per CPU), below
the established 0.32-per-CPU cutoff; no Deno, Node, Bun, Cargo, rustc, or
benchmark worker appeared in the snapshots. The report's `qualification_ready`
is false only because it intentionally selected Splay rather than all eight
fixtures; its Splay rounds and output validation are complete.

| Engine | Median Score | Median max RSS |
| -- | --: | --: |
| Quench | 9,167 | 117,489,664 B |
| Node `--jitless` v26.10.0 | 14,848 | 405,749,760 B |
| Bun 1.4.2 no-JIT | 9,934 | 65,945,600 B |
| QuickJS 2026-06-04 | 8,597 | 151,044,096 B |

Node is the same-campaign Score bar; Bun is the RSS bar. The current M4 trunk
distances are **1.620× Score** and **1.782× max RSS**. Quench remains below the
Score bar and above the RSS bar, so Splay is not won. Exact binary and engine
hashes are recorded in the JSON report.
