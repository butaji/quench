# doxbee promise-job overhead, Quench no-JIT (lane)

Host: Linux x86_64, Intel Xeon @ 2.10GHz, 4 vCPU, 16 GB (cloud container), 2026-10-21.
Runs: `JetStreamDriver.js --no-prefetch --test=NAME` with the lane prelude, one engine process
per run, rounds interleaved across binaries; median / min / max Score and max RSS (wait4).
Run-to-run Score noise on this host is about ±10%, so single runs are not compared.

Binaries (release, lane GC pacing, quench-alloc):

- `head`: 3181fbb9 (before this series)
- `mod`: 2eaf333e (hook-less job boundaries, realm kept across jobs, settled-module check)
- `then`: 9dc78be3 (thenable resolution as its own job kind)
- `job`: b51f9cfd (ALS stores per context; unobserved jobs keep no context record)

| fixture | binary | median | min | max | max RSS KiB |
|---|---|---|---|---|---|
| doxbee-promise | head | 11.68 | 10.88 | 12.14 | 55260 |
| doxbee-promise | job | 22.56 | 21.14 | 23.15 | 51440 |
| doxbee-async | head | 21.01 | 20.02 | 24.14 | 40420 |
| doxbee-async | job | 28.93 | 26.19 | 29.68 | 39212 |

Attribution (3 rounds, doxbee-promise): mod 11.96, then 12.85, job 21.46. Instruction counts
(callgrind, 6 iterations): head-series 10.67G, mod 10.63G, then 9.95G.

The step at `job` is mostly memory behaviour rather than instructions: every reaction captured
a per-job context record that was only freed when the job ran or its promise was collected, so
the record map grew with pending reactions. A job captured with no resource while no promise
hook observes it (ids 0/0, as in Node) now keeps no record.

Bun default (BUN_JSC_useJIT=0) on the same host: doxbee-promise 79.16 / 42264 KiB,
doxbee-async 136.17 / 39156 KiB (`../jetstream3-nojit-vs-bun-default-linux-2026-10-21`).
