# Decision-24 guard for the v2-cloud publish into v2-merged

Base `aba295222` (v2-merged before), candidate `7c7e1fb51` (after merging `v2-cloud`), both `cargo build --profile production -p quench-node` on the Linux x86_64 VM (Xeon 2.1 GHz, 4 vCPU). Script: `evidence/tod123-rss-linux-2026-10-14/callgrind-guard.sh`. Result in `guard-aba295222-vs-7c7e1fb51.txt`: per-run instructions flat or lower on all eight benches (0.00% to -2.85%); startup about 150M instructions lower on each. Merge checks: workspace build clean, runtime lib 585/585, Wasm 67,124/67,124.

## 08:10 UTC: v2-cloud-c2 publish (guard FAILS on Splay)

Base `84978fd52`, candidate `76ba30d45` (`84978fd52` + `v2-cloud-c2` through `5f8b1273a`, the same commits the lane published as `f1a6589f4`). Result in `guard-84978fd52-vs-76ba30d45.txt`: Splay per-run instructions +16.30% (21,462,869 -> 24,960,732) and Splay startup +198M; the other seven benches within -0.59% to +0.66%. Merge checks passed (runtime lib 588/588, Wasm 67,124/67,124). Cause: `5f8b1273a` sets `LARGE_HEAP_GC_HEADROOM` to 1/2; its own message reports Splay Score 3,100 -> 2,660 (-14%) for max RSS 66.3 -> 54.5 MB, one run each.

## 10:10 UTC: v2-cloud-c2 publish `ca89cdf8f` (guard passes)

Base `728530f4e`, candidate `0a69bf846` (`728530f4e` + `v2-cloud-c2` through `7d60b6eee`; same commits as the lane's publish). `guard-728530f4e-vs-0a69bf846.txt`: all eight flat or lower per run (EarleyBoyer -2.99%, RegExp -2.91%, Splay -0.51%). Merge checks: runtime lib 588/588, Wasm 67,124/67,124. Splay's base of 24.96M instructions per run shows the `5f8b1273a` regression (21.46M before it) is still in `v2-merged`.

## 13:30 UTC: merge of v2-cloud-c (13 commits) and v2-gc (1) with resolved conflicts

Base `f9bdbab34`, candidate `d497c700d` (both merges, the small-heap headroom restore `b82da5af3`, the VFS fix `d497c700d`). Checks on the candidate: workspace build clean; runtime lib 589/589; quench-regexp 10/10; Wasm 67,124/67,124; all eight V8-v7 benches complete with output equal to Node v26.10.0; `fs` and `node:vfs` behave as before by default and now work under `--experimental-vfs`.

Guard (`guard-f9bdbab34-vs-d497c700d.txt`): per-run instructions lower on all eight, Crypto -8.72%, DeltaBlue -17.00%, EarleyBoyer -18.73%, NavierStokes -21.69%, RayTrace -40.37%, RegExp -13.06%, Richards -26.86%, Splay -26.77%.

Fixed-work max RSS (guard scripts, `maxrss.c` launcher, five launches for the three smallest deltas): Crypto -0.8%, DeltaBlue +1.6% (+240 KB), EarleyBoyer -2.7%, NavierStokes +1.3% (+200 KB), RayTrace -1.3%, RegExp -1.9%, Richards +3.3% (+450 KB), Splay +5.3% (the large-heap factor back at 1/1 from the RSS lane's 1/2).

Before restoring the small-heap headroom, the merged Score-lane constants (2^16-cell minimum, 1/1 small-heap factor) measured DeltaBlue +27%, EarleyBoyer +36%, RayTrace +22%, RegExp +8.5% max RSS.

Then merged `origin/v2-merged` (Node-lane socket options, static OpenSSL zlib, Test262 waivers) as `3e5ca74ac`: `http_shared_vm/poll.rs` takes the Node lane's version (our side differed only by the Score lane's rustfmt pass), `modules/mod.rs` declares both new modules. On `3e5ca74ac`: workspace build clean, runtime lib 589/589, Wasm 67,124/67,124, V8-v7 8/8 output equal to Node v26.10.0, VFS default and `--experimental-vfs` both work. `quench-node` lib: 2 of 4 fail (`process_shared_vm` root tests, atom index out of bounds in `atom_keys.rs:62`); the same two fail on `origin/v2-merged` before this merge.

## 13:50 UTC: v2-cloud-c `56447112c`, `d11d9917e` (guard FAILS, held)

Base `e3fc15972`, candidate `0736e42b8` (`e3fc15972` + those two commits; not published). Runtime lib 591/591, Wasm 67,124/67,124. `guard-e3fc15972-vs-0736e42b8.txt`: Crypto -20.42%, NavierStokes -25.53%, but Splay +7.18% and Richards +2.19% per run (DeltaBlue +0.66%, EarleyBoyer +0.46%).

## 14:20 UTC: v2-cloud-c2 (3 commits) published, v2-cloud-c (7) held

Base `490a0461f`. Candidate A `6d3cd48a2` (+ `v2-cloud-c2` through `6f2cb3522`): all eight flat or lower, Splay -18.63% per run; published. Candidate B `9ad7d4a7b` (A + `v2-cloud-c` through `3b09e9ea9`): Crypto -38.01%, NavierStokes -46.65%, Splay -12.25% against base but +7.8% against A, Richards +2.12%: held (decision 24). Runtime lib 594/594, quench-regexp 11/11 on B.

## 16:10 UTC: v2-cloud-c through `2b925fb75` (9 commits) still held

Base `053afbcff`, candidate `6f98fda79`: Crypto -54.38%, NavierStokes -49.49%, Richards +2.17%, Splay +7.82% per run.
