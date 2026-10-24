# Final PGO recheck

This directory contains the corrected 11-round package matrix for the current clean PGO production binary. The no-JIT reports include Node wherever Node 24.19.0 supports the workload; Vite and Undici omit Node because `--jitless` removes WebAssembly. All JIT reports include Node and Bun.

See [scorecard.md](scorecard.md) for gate calculations and reproduction notes. Every report records output parity, exact commands, versions, input hashes, per-round throughput and RSS. Linux RSS is sampled process-tree RSS and is diagnostic only.
