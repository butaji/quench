# 80 — Persisted cross-run linked-image cache

Status: planned

`build.rs` performs AOT stencil template extraction at build time, but there is no
runtime-side cache of the per-script linked image. Every invocation of `cargo run --
path/to/script.js` re-runs the full OXC AST -> DynCode -> quoted stencil -> linked
executable pipeline, even when the script and target are unchanged from a previous run.

This matters specifically for server-side/"defined program" deployment: repeated
process starts (serverless cold starts, CLI reruns, dev-loop iteration) all pay the
same AOT cooking cost for identical input. V8's on-disk code cache and Node's bytecode
caching exist for the same reason, but deegen has no interpreter tier to fall back to
while a cache warms, so a cache hit here replaces the entire compile pipeline rather
than just bytecode generation.

Add a cache directory (e.g. `~/.cache/deegen/images/<hash>`) keyed by a hash of the
script source, target triple, and a `DEEGEN_CACHE_VERSION` constant. On startup, `main.rs`
checks the cache before running the pipeline; on a miss, it runs the pipeline as today
and writes the finished executable image plus relocation/source metadata before
executing.

Acceptance: cold start for an unchanged script drops to cache-load + relink time;
correctness gate confirms cache invalidation on any source, flag, or deegen-version
change; cache hit/miss is observable via `DEEGEN_JIT_STATS` (see task 82).

Related: 46 (closed-world mode), 50 (deployment-scaled specialization budget),
14 (code image reuse — that task covers in-process reuse across closures/call sites;
this task covers cross-process persistence).
