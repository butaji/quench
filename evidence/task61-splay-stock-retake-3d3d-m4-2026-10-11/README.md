# M4 stock Splay retake for the gated `3d3d1d4e5` runtime

This is an 11-round stock-harness Splay result for the exact pinned
`3d3d1d4e5` performance binary. It is a Splay-only stock measurement, not a
full Task 61 qualification and not a stock result for the later correctness
fix build.

## Reproduction and provenance

- Host: M4, macOS arm64, `Mac16,10`, 10 logical CPUs.
- Runner: `quench-bench` schema 5, SHA-256
  `bdc9d6c4afc537fe8ed30d4fccba9157cd4232e345ce75810465a0fff98db109`.
- Fixture corpus: `64e1860c736c1b899708f4cd646721bc71e53d8b`.
- Quench executable SHA-256:
  `66704b3b7c3e3f0bc29082d0cb5c95cc10841ef90b9e89772f04838443f94f36`.
  This is the production binary for the gated `3d3d1d4e5` speed head merged at
  `3760888a0`.
- The runner's clean source checkout was at `222b0163f`; the pinned executable
  predates correctness-only commit `85def2450`. The report records both the
  checkout revision and executable SHA. Splay does not execute the changed
  destructuring path, but this is not a rebuilt binary for the latest trunk.
- `PATH` included `/usr/sbin`, so load capture succeeded. Across 88
  before/after snapshots, one-minute load ranged 1.65–2.80 and five-minute load
  2.20–2.43. The 15-minute load ranged 4.10–4.26, reflecting earlier host
  activity. Snapshots showed no unrelated Deno or Bun test workers. The
  benchmark's own engine processes are excluded by the before/after sampling.
- All 11 rounds completed with equal output. This report intentionally selects
  only Splay, so its `qualification_ready` field is false for the eight-fixture
  campaign requirement.
- Raw report SHA-256:
  `db97fe1477c10867105904c2ba3444ffa2bd4768d6ea4d6c78df3c7e7cadaba1`.

## Results

| Engine | Median stock Score | Median max RSS |
| -- | --: | --: |
| Quench | 9,078 | 116,539,392 B |
| QuickJS | 8,638 | 151,060,480 B |
| Bun, JIT disabled | 9,974 | 64,454,656 B |
| Node `--jitless` | 14,864 | 405,602,304 B |

The stock Score bar is Node (`14,864 / 9,078 = 1.637x`); the max-RSS bar is Bun
(`116,539,392 / 64,454,656 = 1.808x`). For the measured `3d3d1d4e5` binary,
Splay is therefore **1.64x from the Score bar and 1.81x from the RSS bar**.
The current trunk's post-`85def2450` executable still needs a stock retake before
these become the current-trunk scoreboard values.

Raw report: [stock-11.json](stock-11.json).
