# V8-v7 Score: PGO build and the binding-time changes (TOD-135)

Host: a Linux container (Firecracker VM), 4 vCPUs (Intel Xeon @ 2.80 GHz), 15 GiB RAM.
Toolchain: rustc 1.99.0. Run-to-run noise on this host is about ±15%, so every
comparison below is paired or uses a median over many rounds.

## Engines
- **Quench:** `v2-cloud-c` at 297988b847, two builds of the same tree:
  - **plain:** `cargo build --profile production -p quench-node`;
  - **PGO:** `tools/pgo-build.sh production`, trained on the interpreter curriculum,
    `quench-bench/property-get.js` and the Node compatibility fixtures. No scored suite is
    in the training set.
- **References:** Node 22.22.0 with `--jitless`, Bun with `BUN_JSC_useJIT=0`, and QuickJS.

## PGO against plain, same tree
Paired with `ab.cjs`, the harness run by quench-node. "Pairs" counts the pairs where the
PGO build scored higher.

| Fixture | Plain | PGO | Change | Pairs |
|---|---:|---:|---:|---:|
| Crypto | 1436 | 1734 | +20.8% | 7/7 |
| EarleyBoyer | 2011 | 2311 | +14.9% | 10/11 |
| DeltaBlue | 887 | 935 | +5.4% | 6/11 |
| RayTrace | 2302 | 2413 | +4.8% | 7/7 |
| RegExp | 685 | 697 | +1.8% | 5/7 |
| Richards | 924 | 927 | +0.3% | 5/11 |
| Splay | 3611 | 3489 | −3.4% | 5/7 |
| NavierStokes | 2069 | 1981 | −4.3% | 1/7 |

NavierStokes and Splay stay ahead of every reference under PGO (5 rounds, medians):

| Fixture | Quench PGO | Bun | Node | QuickJS |
|---|---:|---:|---:|---:|
| NavierStokes | 1781 | 1516 | 995 | about 1130 |
| Splay | 3958 | 2459 | 2481 | about 1000 |

## Standings with the PGO build (`vs.sh`, 15 interleaved rounds, medians)

| Fixture | Quench | Bun | Node | Result |
|---|---:|---:|---:|---|
| DeltaBlue | 1002 | 710 | 944 | Quench leads, +6.1% over Node |
| Richards | 984 | 1023 | 851 | Quench 3.8% behind Bun; two of its samples are outliers (622, 532) |
| EarleyBoyer (9 rounds) | 1937 | 1535 | 3049 | Quench at 0.64 of Node |

## Instruction evidence for the session's changes
Callgrind, fixed-iteration fixtures, startup subtracted.

**Context register for JavaScript views** (321aa06e73):
- Thread-local context loads on Richards: 7.05M → 0.60M.
- Instructions: Richards −1.6%, DeltaBlue −1.6%, EarleyBoyer −1.1%.

**Capture stores and loads:**
- Stores (297988b847): EarleyBoyer −2.0%.
- Loads (4c3794a302): EarleyBoyer −1.0%, Richards −0.7%.

**Inherited fields, multi-way, plus array-record receivers:**
- DeltaBlue −4.7% instructions.
- Paired Score +15.7% (12/15).
