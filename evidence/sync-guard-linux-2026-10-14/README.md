# Decision-24 guard for the v2-cloud publish into v2-merged

Base `aba295222` (v2-merged before), candidate `7c7e1fb51` (after merging `v2-cloud`), both `cargo build --profile production -p quench-node` on the Linux x86_64 VM (Xeon 2.1 GHz, 4 vCPU). Script: `evidence/tod123-rss-linux-2026-10-14/callgrind-guard.sh`. Result in `guard-aba295222-vs-7c7e1fb51.txt`: per-run instructions flat or lower on all eight benches (0.00% to -2.85%); startup about 150M instructions lower on each. Merge checks: workspace build clean, runtime lib 585/585, Wasm 67,124/67,124.
