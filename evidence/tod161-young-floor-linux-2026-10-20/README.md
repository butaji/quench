# TOD-161: the young collection floor

Host: Linux x86_64 VM, 4 logical CPUs, 16 GB. quench-node release builds.

## Method

- **Policy sweeps (same binary).** One `gc-tuning` build of v2-gc `f2aff929`
  (`tune/quench-node`) runs every policy. Each policy is a wrapper that sets
  `QUENCH_GC_YOUNG_NUM/DEN` and `QUENCH_GC_MIN_YOUNG` (`wrapper-example.sh`), so binary
  layout cannot differ between the policies compared.
- **Scorecard.** The TOD-158 `scorecard.py` runs interleaved, order-alternating pairs:
  - V8-v7, all eight benchmarks;
  - the five framework servers;
  - eight GC-heavy JetStream tests at 15 iterations;
  - CoreMark where noted.
- **npm top 300:** `rss1.py` peak RSS per package.
- **Base:** production pacing, young 1/64 with a floor of 384.

## Decision rule (set before measuring, on TOD-161)

Adopt the largest policy for which all of these hold:

1. Suite RSS geomean ≤ ×1.005.
2. No workload's RSS rises by more than 5% or by more than 1 MB.
3. npm top 300 RSS geomean ≤ ×1.01, with no package worse by more than 5%.
4. Speed geomean ≥ ×1.0 on every suite.

## Sweep (vs 1/64, floor 384)

| Policy | Pairs | V8-v7 | JetStream | Frameworks | Rule |
| -- | --: | -- | -- | -- | -- |
| young 1/128 | 3 | ×0.974 | ×0.945 | ×1.003 | slower |
| young 1/32 | 3 | ×1.029 | ×1.118 | ×0.998 | fw speed |
| young 1/16 | 3 | ×1.134 | ×1.227 | ×1.062 | web-ssr +6.8 MB |
| young 1/8 | 3 | ×1.224 | ×1.262 | ×0.993 | web-ssr +8 MB |
| floor 16384 | 3 | ×1.560 | ×1.328 | ×1.026 | V8-v7 RSS ×1.088 |
| floor 4096 | 3 | ×1.437 | ×1.269 | ×1.000 | web-ssr +8 MB, regexp +8% |
| floor 1536 | 3 | ×1.314 | ×1.132 | ×0.996 | web-ssr +5.3 MB |
| 1/32 + floor 1024 | 3 | ×1.226 | ×1.130 | ×0.997 | koa +1.2 MB |
| floor 1024 | 7 | ×1.222 | ×1.097 (3) | ×1.012 | koa +1.1 MB |
| **floor 768** | 7 | **×1.161** | **×1.088** (3) | **×1.023** | **passes** |

Floor 768 in detail:

- **RSS geomeans:** V8-v7 ×0.997, frameworks ×0.999, JetStream ×0.999.
- **Largest single RSS change:** regexp +0.3 MB (+4%). web-ssr is unchanged.
- **npm top 300:** ×0.9994, no package worse by 5% (`npm-top300-m768.txt`).
- **Biggest wins:** raytrace ×1.54, regexp ×1.60 and earley-boyer ×1.24, each 7/7;
  lazy-collections ×1.39 and async-fs ×1.20, each 3/3.

**Mechanism.** At 1/64 of a small live set, the floor sets the young interval, so the 384
floor made small programs collect every few hundred allocations without lowering their peak.
web-ssr's peak follows the young budget: its large transient strings make the peak bimodal,
at 54 or about 60 MB. The floor that keeps the peak at 54 MB is the binding constraint.

## Production build check (`scorecard-production-build-5.txt`)

The production build of this change (`c161`) was paired against v2-gc `f2aff929` (`c160`),
5 pairs:

- **Speed:** V8-v7 ×1.165 (raytrace ×1.50 and regexp ×1.63, 5/5); frameworks ×1.043.
- **RSS geomean:** V8-v7 ×1.011, frameworks ×1.003. These are separate builds, so code-page
  layout adds about 0.1 MB per small program (crypto, deltablue, raytrace and richards each
  read +0.1 MB). On this pair regexp reads 7.5 → 7.9 MB.
- **Why the rule uses the same-binary sweep:** it isolates the policy. There, regexp is
  +0.3 MB and the suite RSS geomean is ×0.997.
- `quench-runtime` lib tests: 751 passed.
