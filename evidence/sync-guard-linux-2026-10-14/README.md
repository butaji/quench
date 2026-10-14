# Decision-24 guard for the v2-cloud publish into v2-merged

Base `aba295222` (v2-merged before), candidate `7c7e1fb51` (after merging `v2-cloud`), both `cargo build --profile production -p quench-node` on the Linux x86_64 VM (Xeon 2.1 GHz, 4 vCPU). Script: `evidence/tod123-rss-linux-2026-10-14/callgrind-guard.sh`. Result in `guard-aba295222-vs-7c7e1fb51.txt`: per-run instructions flat or lower on all eight benches (0.00% to -2.85%); startup about 150M instructions lower on each. Merge checks: workspace build clean, runtime lib 585/585, Wasm 67,124/67,124.

## 08:10 UTC: v2-cloud-c2 publish (guard FAILS on Splay)

Base `84978fd52`, candidate `76ba30d45` (`84978fd52` + `v2-cloud-c2` through `5f8b1273a`, the same commits the lane published as `f1a6589f4`). Result in `guard-84978fd52-vs-76ba30d45.txt`: Splay per-run instructions +16.30% (21,462,869 -> 24,960,732) and Splay startup +198M; the other seven benches within -0.59% to +0.66%. Merge checks passed (runtime lib 588/588, Wasm 67,124/67,124). Cause: `5f8b1273a` sets `LARGE_HEAP_GC_HEADROOM` to 1/2; its own message reports Splay Score 3,100 -> 2,660 (-14%) for max RSS 66.3 -> 54.5 MB, one run each.

## 10:10 UTC: v2-cloud-c2 publish `ca89cdf8f` (guard passes)

Base `728530f4e`, candidate `0a69bf846` (`728530f4e` + `v2-cloud-c2` through `7d60b6eee`; same commits as the lane's publish). `guard-728530f4e-vs-0a69bf846.txt`: all eight flat or lower per run (EarleyBoyer -2.99%, RegExp -2.91%, Splay -0.51%). Merge checks: runtime lib 588/588, Wasm 67,124/67,124. Splay's base of 24.96M instructions per run shows the `5f8b1273a` regression (21.46M before it) is still in `v2-merged`.
