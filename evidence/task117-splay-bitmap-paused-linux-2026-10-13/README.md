# TOD-117 paused Splay collector experiment

The user reassigned v2-cloud to Richards in parallel with Splay on v2-cloud-c.
The shape-bitmap experiment is paused before any candidate build, completed
correctness suite, or candidate measurement. The patch is archived here and in
the local stash; it is not applied to the integrated runtime. No improvement
or win is claimed.

The baseline production binary was pinned from clean source 81b571194, Rust
1.99.0, production profile (thin LTO, repository 64-byte function alignment),
without PGO or instrumentation. Linux x86_64, 4-CPU cgroup quota, 16 GiB memory
limit. Exact engine/binary hashes, host, commands, no-JIT proofs and samples are
in baseline-stock-11.json. All 11 rounds were valid and output-equal. This
Splay-only report is diagnostic, not qualification.

The baseline Callgrind 3.24.0 K=0 and K=300 runs use the exact fixed-work runner
template materialized by quench-bench, executed through quench-node. Their
totals, marginal instructions/run and source hashes are in baseline-instructions.json;
the compressed raw traces and materialized files are retained alongside it.
These counts are attribution only. No candidate comparison was run.
