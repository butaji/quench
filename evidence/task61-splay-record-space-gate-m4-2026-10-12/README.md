# Splay record-space gate, M4, 2026-10-12

This is a fixed-work diagnostic, not a stock-harness qualification. It compares the pushed record-heap candidate with the current trunk binary on the M4 arm64 macOS host and records the reason the candidate was rejected.

## Pinned inputs

- Host: Mac16,10, arm64 Darwin, 10 logical CPUs, 16 GiB.
- Trunk runtime source: `17077748a`; executable SHA-256 `8432da1159e301d021c236148a4f4a13e5f1b8de40fb87893fb54a2ebf5795dd`.
- Candidate source: `63fc6629bc81c7bc44b2938ae2bc553e3395a016`; executable SHA-256 `f8739affbfee468f88aad0546a6ea1de1aa4fb706024665c163db6c94d2ab180`.
- Measurement runner: schema 5, SHA-256 `bdc9d6c4afc537fe8ed30d4fccba9157cd4232e345ce75810465a0fff98db109`.
- Decision tool: `task61-fixed-work-guard`, built from trunk checkout `691b00a6a5e52fd1a9ec503f92c1f36e2bcaf843`; SHA-256 `9e4e6a30db2972e5e742f701dde78605f581d87f14f5831a967bff5a5c4f2f95`.
- Fixed work: Splay's 300 `run()` iterations per sample; each schema-5 pair subtracts the same engine's setup-only work. Every report records the suite inputs, argv, binary hashes and per-sample contention checks.

## Splay result

Same-binary A/A completed 11/11 clean pairs. Paired marginal wall drift was −0.028% (95% paired bootstrap interval −0.983% to +1.464%).

Candidate A/B completed 11/11 clean Splay pairs. Paired marginal wall was +0.680% (95% interval −0.055% to +3.191%), so the interval includes zero. The all-eight guard's 8 clean Splay pairs likewise found no cycle regression (−0.064%, 95% interval −6.100% to +1.753%). Splay max RSS fell 21.99%: 116,375,552 B to 90,800,128 B. The candidate reduces Splay's RSS while leaving its fixed-work speed statistically unresolved; the stock Splay distances remain 1.664× Score and 1.810× RSS on M4 because this candidate was not integrated.

## All-eight guard

The report completed all eight fixtures and output validation passed. Clean pair counts were Crypto 11, DeltaBlue 11, EarleyBoyer 10, NavierStokes 11, RayTrace 11, RegExp 11, Richards 11, Splay 8. Every fixture met the guard's 7-clean-pair minimum. The guard decision is `rejected`.

Paired marginal wall deltas (candidate vs trunk; 95% paired bootstrap intervals):

| Fixture | Clean pairs | Wall delta | 95% interval |
| --- | ---: | ---: | ---: |
| Crypto | 11 | +9.365% | +8.915% to +10.108% |
| DeltaBlue | 11 | −5.566% | −7.027% to −4.965% |
| EarleyBoyer | 10 | +1.950% | −1.477% to +2.769% |
| NavierStokes | 11 | +5.840% | +5.146% to +7.279% |
| RayTrace | 11 | +3.222% | +2.714% to +3.618% |
| RegExp | 11 | +1.549% | −0.854% to +7.325% |
| Richards | 11 | +3.203% | +2.544% to +3.788% |
| Splay | 8 | −0.308% | −9.172% to +1.308% |

Crypto, NavierStokes, RayTrace and Richards exceed the 2% wall-regression allowance with intervals wholly above it. The candidate therefore fails the all-eight guard despite its Splay RSS reduction. The paired wall summary is reproducible with `python3 analyze_paired_wall.py`; it uses the guard's 20,000-resample percentile bootstrap, seed, and clean-pair rule.

The guard's separate cycle/instruction/RSS decision is in `guard-checkpoint.decision.json`. Notable cycle deltas are Crypto +10.013% [9.840%, 11.523%], NavierStokes +5.865% [5.362%, 6.768%], RayTrace +3.110% [2.876%, 3.558%], and Richards +3.192% [2.660%, 3.875%].

## Next attribution target

In the candidate, legacy-cell access in `crates/quench-runtime/src/heap/access.rs` first decodes the legacy `Value`, reads a `SlotEntry`, extracts its record ID, then reads `legacy_cells`. Trunk reads the `Cell` directly from its slot slab. This adds a dependent lookup to arrays, functions, environments, strings and other legacy kinds. The measured cycle regressions are strongest on indexed-array and object-heavy fixtures, while instruction deltas are much smaller on some fixtures; treat the extra dependency as the next hypothesis to profile, not as proven sole cause. A follow-up should preserve direct object records while restoring the legacy `Cell` slot's direct address path, then repeat the same paired Splay and all-eight gates.
