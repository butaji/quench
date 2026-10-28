# Splay schema-5 fixed-work comparison — M4

This is an 11-pair fixed-work comparison of the pinned contiguous-cell candidate against the pinned current-trunk baseline. The benchmark runs `Splay.Setup`, exactly 300 `run()` calls, and `TearDown`; K=0 setup-only runs are subtracted for marginal per-run metrics. It is a diagnostic gate, not a stock-harness qualification.

## Provenance

- Host: M4 arm64 macOS (`Mac16,10`, 10 logical CPUs), Rust 1.99.0; process metrics from `/usr/bin/time -l`.
- Runner: `fixed_work_diagnostic` schema 5, built from source revision `19f4a1aea8edaf3932225fbae5de606fddbc9f41`; the checkout had unrelated runtime candidate changes, while the measured binaries themselves were pinned by SHA.
- Fixture: `quench-bench/js-engine-benchmark/v8-v7/splay.js`, SHA-256 `1f875cec579d03af40d256ceaeb6823f3ef16c883652650ca51d183472a8185d`.
- Baseline `quench-node`: `ceeca73f772b141ee7619ae869bffaac1d93eeee99ecba2b07337d8345937e80`.
- Candidate `quench-node`: `bdcc9a93be82d25c1bdef0bbc1c7db677a0dd4905b87d92a8b0c6d341f87ca71`.
- All 11 pairs passed the schema-5 contention checks; both engines produced valid, matching output.

The raw report is [`splay-schema5-current-trunk-vs-contiguous-11.json`](splay-schema5-current-trunk-vs-contiguous-11.json). The analysis is reproducible with [`analyze_splay_schema5_pair.py`](analyze_splay_schema5_pair.py) and is saved as [`splay-schema5-current-trunk-vs-contiguous-11.analysis.json`](splay-schema5-current-trunk-vs-contiguous-11.analysis.json). The paired interval resamples the 11 round deltas with the same 20,000-replicate percentile bootstrap and seed as `task61-fixed-work-guard`.

## Result

| Metric | Trunk | Candidate | Candidate vs trunk, paired median (95% interval) |
|---|---:|---:|---:|
| Marginal wall per `run()` | 1.062 ms | 1.054 ms | −1.03% (−2.44%, +1.13%) |
| Marginal cycles per `run()` | 4.348 M | 4.330 M | −0.21% (−2.08%, +0.97%) |
| Marginal instructions per `run()` | 24.078 M | 23.916 M | −0.68% (−0.69%, −0.67%) |
| Work max RSS median | 116,424,704 B | 90,718,208 B | −22.02% (−22.16%, −22.00%) |
| Setup-only max RSS median | 65,388,544 B | 51,429,376 B | — |

Splay RSS improves substantially. The paired wall and cycle intervals include zero, so this run does not establish a speed gain; instructions fall by 0.68%. This does not establish a stock Score win.

The candidate is not retainable on this result alone. The earlier all-eight screens show instruction regressions on Crypto and NavierStokes (about +7.5% each) and smaller regressions elsewhere. The source audit in [`legacy-cell-access-source-audit.md`](legacy-cell-access-source-audit.md) narrows the residual access-path hypothesis to SlotEntry mapping/tag checks; it rules out a production block-directory or occupancy lookup. A schema-5 all-eight rerun is needed after a candidate removes that cost.
