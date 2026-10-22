# Splay post-merge M4 Step 0

Source revision: `3d8020107ef0b3ffdcc84cc3934631050f95eb19` (clean trunk). Host: Mac16,10, Darwin arm64 T8132, 10 logical CPUs; Rust 1.99.0. The production profile uses the checked-in thin-LTO policy.

## Fixed-work diagnostic

`four-engine-11.json` is the schema-4 fixed-work diagnostic for Splay, 11 rounds, 300 `run()` calls per work sample, with paired `K=0` setup-only subtraction. It is not a stock-harness qualification run. The runner and production Quench binary hashes are recorded in the JSON. `profile-input.js` has the exact same SHA-256 as the runner's 300-run materialized input.

## RSS composition

`profile.stderr` and `profile.stdout` come from one 300-run profile-memory execution using the exact materialized input. The build and command were:

```sh
CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/candidate-a cargo build \
  --profile production -p quench-node --bin quench-node --features profile-memory
QUENCH_MEMORY=1 QUENCH_MEMORY_PEAK=1 /usr/bin/time -l \
  target/pinned/5403d02c97edcfe9f0126f4f34f666f1ca13eb0e8d0492bb7adc0f2b00b07f01/quench-node-profile-memory \
  evidence/task61-splay-step0-post-merge-2026-10-10/profile-input.js \
  > evidence/task61-splay-step0-post-merge-2026-10-10/profile.stdout \
  2> evidence/task61-splay-step0-post-merge-2026-10-10/profile.stderr
```

The profile-memory binary adds per-slot and per-site lifetime counters. At peak it reports 60,856,976 B of profile instrumentation; its 185,712,640 B process max RSS must not be compared with production. Use the fixed-work report's production RSS for the bar. `summary.json` contains the selected before/after-GC and completion snapshots plus the corresponding production measurements.

At the pre-GC high-water, the heap had 1,562,021 occupied slots (1,030,520 Objects and 496,400 Arrays), a 56 B slot stride, and 87,506,944 B reserved in the slot arena. The next GC retained 780,984 slots. The internal accounted runtime total was 94,828,990 B before GC; this excludes the profile counters and allocator/RSS residual.
