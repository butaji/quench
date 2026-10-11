# Splay speed-lane head 3d3d1d4e5 on M4

This screens `3d3d1d4e5a91a24d293cf222251a2786d6cb6ab6` against the pinned M4
trunk binary from `81c5147270befc131c7d7df28766c772783d916b`. It is a
fixed-work mechanism gate, not the stock-score qualification.

## Reproduction

- Host: M4, macOS arm64, `Mac16,10`, 10 logical CPUs, 16 GiB RAM.
- Rust: 1.99.0, LLVM 23.1.2.
- Candidate source checkout: clean at `3d3d1d4e5a91a24d293cf222251a2786d6cb6ab6`.
- Build: `CARGO_TARGET_DIR=/Users/admin/Code/GitHub/quench-v2/target/candidate-a cargo build --profile production -p quench-node --bin quench-node`.
- Production profile: opt-level 3, thin LTO, one codegen unit, panic abort,
  stripped symbols. The committed `.cargo/config.toml` enables 64-byte function
  alignment for this target.
- Candidate binary SHA-256:
  `66704b3b7c3e3f0bc29082d0cb5c95cc10841ef90b9e89772f04838443f94f36`.
- Baseline binary SHA-256:
  `ceeca73f772b141ee7619ae869bffaac1d93eeee99ecba2b07337d8345937e80`.
- Runner: schema 5, SHA-256
  `bdc9d6c4afc537fe8ed30d4fccba9157cd4232e345ce75810465a0fff98db109`.
- Fixture corpus: clean submodule revision
  `64e1860c736c1b899708f4cd646721bc71e53d8b`; all-eight report says the fixture
  set matches and all outputs are equal.

The runner records `source_revision=597dd55d…` and `source_dirty=true` because
it was launched from the dirty coordination checkout. The measured engines are
identified independently by their executable paths and SHA-256 values above;
the candidate source worktree was clean. The pinned runner itself and the clean
fixture submodule are recorded in each raw report.

```sh
target/pinned/bdc9d6c4afc537fe8ed30d4fccba9157cd4232e345ce75810465a0fff98db109/quench-bench \
  quench-bench/js-engine-benchmark/v8-v7/splay.js \
  --fixed-work --runs 11 --timeout-ms 300000 \
  --quench target/pinned/ceeca73f772b141ee7619ae869bffaac1d93eeee99ecba2b07337d8345937e80/quench-node \
  --quench-peer target/pinned/66704b3b7c3e3f0bc29082d0cb5c95cc10841ef90b9e89772f04838443f94f36/quench-node \
  --out evidence/task61-speed-3d3d-m4-2026-10-11/splay-fixed-work-trunk-vs-candidate-11.json

target/pinned/bdc9d6c4afc537fe8ed30d4fccba9157cd4232e345ce75810465a0fff98db109/quench-bench \
  --all --fixed-work --runs 11 --timeout-ms 300000 \
  --quench target/pinned/ceeca73f772b141ee7619ae869bffaac1d93eeee99ecba2b07337d8345937e80/quench-node \
  --quench-peer target/pinned/66704b3b7c3e3f0bc29082d0cb5c95cc10841ef90b9e89772f04838443f94f36/quench-node \
  --out evidence/task61-speed-3d3d-m4-2026-10-11/all-eight-fixed-work-trunk-vs-candidate-11.json

target/candidate-a/production/task61-fixed-work-guard start \
  --initial-report evidence/task61-speed-3d3d-m4-2026-10-11/all-eight-fixed-work-trunk-vs-candidate-11.json \
  --checkpoint evidence/task61-speed-3d3d-m4-2026-10-11/guard-checkpoint.json \
  --change-kind semantic-neutral-metadata-or-dispatch
```

## Splay result

All 11 paired rounds were clean. Per-run medians (300 `run()` calls per
process):

| Metric | `ceeca73f` trunk | `3d3d1d4e5` | Candidate change |
|---|---:|---:|---:|
| Marginal wall / `run()` | 1,096,584 ns | 900,498 ns | −17.92% paired median |
| Marginal cycles / `run()` | 4,412,398 | 3,612,685 | −17.75%; 95% paired bootstrap interval [−18.35%, −17.40%] |
| Marginal instructions / `run()` | 24,079,371 | 20,655,752 | −14.21% |
| Median work max RSS | 116,424,704 B | 116,342,784 B | −81,920 B |

The all-eight resumable guard reports **passed**, with 11/11 clean pairs for
each fixture. Every cycle interval excludes a regression; RegExp is neutral
within its interval. The table reports paired median changes, and cycle
intervals are the guard's deterministic 95% bootstrap intervals.

| Fixture | Marginal wall | Cycles (95% paired interval) | Instructions | Work max RSS |
|---|---:|---:|---:|---:|
| Crypto | −13.07% | −13.09% [−13.60%, −12.52%] | −5.88% | 0.000% |
| DeltaBlue | −9.17% | −8.95% [−9.42%, −8.64%] | −6.07% | −0.314% |
| EarleyBoyer | −16.52% | −16.62% [−17.10%, −16.32%] | −12.73% | −0.618% |
| NavierStokes | −7.04% | −7.43% [−7.72%, −5.92%] | −6.48% | −0.311% |
| RayTrace | −11.21% | −11.20% [−11.54%, −10.62%] | −8.72% | −0.856% |
| RegExp | +0.29% | −0.07% [−1.05%, +1.45%] | −0.20% | −1.024% |
| Richards | −14.76% | −14.96% [−15.14%, −14.34%] | −8.93% | 0.000% |
| Splay | −17.92% | −17.75% [−18.35%, −17.40%] | −14.21% | −0.042% |

## Stock-score attempt

The 11-round stock run completed all engine processes but is **not a valid
qualification result**. Deno workers appeared in host snapshots during round
3 and dominated later rounds; Quench's score fell from about 9,000 to 700–900.
Do not use its medians to update the Splay bars. The raw report is retained for
the contention record; repeat stock Splay in a clean window.

## Raw artifacts and hashes

| Artifact | SHA-256 |
|---|---|
| [Splay fixed-work pair](splay-fixed-work-trunk-vs-candidate-11.json) | `1eb0161a3d2fce9f13cf701815b61f81fad53fa0ea3975b44ebda005472a96ec` |
| [All-eight fixed-work pair](all-eight-fixed-work-trunk-vs-candidate-11.json) | `f78142e927e0cb881734530f5c05414d320f80875447fdf439c563006a413bf4` |
| [Guard checkpoint](guard-checkpoint.json) | `d1cd4ef3df6c3e0dfa082e51e21f761bdc31d08bb4290b230fb4fdd3c6e6ebe4` |
| [Guard decision](guard-checkpoint.decision.json) | `be03847cff69cff2e7ba8a494766fe046965da04954233e56b910b4b1960fdef` |
| [Stock attempt, contended](splay-stock-11.json) | `71926e0f560cc234cdd72e92de4381e997c24090df486115738a1431b50556ab` |

Stock Score and the active Splay distance are unchanged until a clean
stock-harness campaign is recorded. These fixed-work results qualify the
candidate for the M4 mechanism gate only; they do not mark Splay won.
