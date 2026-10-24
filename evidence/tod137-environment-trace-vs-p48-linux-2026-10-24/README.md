# Closure-environment trace against the last commit before it (npm follow-up)

The npm lane reported that, with the cost bound (`17e0bdfa1`), typescript-eslint and
@typescript-eslint/eslint-plugin still ran 13–20% behind p48, with 16% more minor faults. Their
binaries were PGO builds.

Probe: matched static production builds, not PGO, x86_64 Linux 6.18 VM, 4 CPUs, no other load.
`tse2.py` runs the npm-top300 scenario, 9 interleaved runs per arm after one warm-up each, and
reports the median CPU (user + system) and minor faults.
- `old`: `7da0338a4`, the last commit before the closure trace.
- `trim`: v2-gc `d87c06f3e7` with a temporary `QX_NOTRIM` switch, not committed, that skips
  the trimming trace.
- `notrim`: the same binary with the switch set.

| Package | Arm | CPU ms | Minor faults |
| -- | -- | --: | --: |
| typescript-eslint | old | 1268 | 69,162 |
| | notrim | 1277 | 70,814 |
| | trim | 1268 | 73,402 |
| @typescript-eslint/eslint-plugin | old | 1239 | 69,689 |
| | notrim | 1213 | 70,167 |
| | trim | 1229 | 73,131 |

Reading:
- With the bound, the trace costs no measurable CPU on these packages.
- It touches about 2.6–3.0 K more pages per run (+4%): the trace's per-collection entry
  tables, and the stores that clear unread slots.
- The other ~1.5 K faults against `old` come with the rest of the tree, not the trace.
- npm's remaining 13–20% CPU gap does not reproduce on matched non-PGO builds. The likely
  causes are a PGO profile trained before the change, or other commits since p48. A retrained
  PGO comparison would settle it.
- An earlier, noisier pair (7 runs, with a build running) had shown +13% and +4%.
