# Canonical inspection combined integration qualification

Frozen source cc7ccbdb29e9e7dfda25f0d9d7fbd9fffa407da7 integrates canonical inspection with shared Node/npm and runtime work. Workspace/all-target checks, 639 runtime tests, 320 serial Node API tests, 43 framework fixtures and all nine no-JIT web correctness targets pass. The official release index still identifies Node v26.12.0 as latest stable.

The merge retains genuine External values, crypto-key predicates, synchronous module exports, and both lanes' tests. Native bindings use the private builtin cache; public exports derive from these identities. Raw conflict excerpts are retained for review.

All 32 fresh matched Callgrind captures are valid and all eight instruction rows pass the 1% limit: Crypto +0.1378%, DeltaBlue -6.3492%, Earley-Boyer -2.9650%, Navier-Stokes -1.0466%, RayTrace -6.6059%, RegExp -7.5358%, Richards -7.2244%, Splay -1.6459%. These combined changes do not isolate an optimization, establish production throughput, or establish a benchmark win. Canonical inspection's startup cost remains a recorded finding in the preceding evidence.

Later constructor-provenance and source-label work is excluded. The runner/test executable hashes were recorded before qualification; mutable build paths were subsequently rebuilt only after the full API run completed. The frozen CLI remains unchanged. Passing these selected checks is not total Node parity; the broad stable audit and retained open counterexamples remain authoritative gaps.
