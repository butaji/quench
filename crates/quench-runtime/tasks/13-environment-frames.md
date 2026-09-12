# 13 — Cached lexical resolution and non-escaping frames

Status: complete

Preserve the existing `NameIc { depth, slot }`: per-function immutable name layouts and cached depth+slot resolution are correct. Completed improvements include fixed local pointers and reuse of non-capturing environments.

Non-escaping function frames now use a pooled plain `Vec<Value>` addressed through the
same fixed local-slot connector as captured frames. Only functions containing hoisted
or dynamically created closures materialize `Rc<RefCell<Environment>>`; their children
therefore retain the same live lexical storage. Because bytecode lowering already maps
every function-owned name to a local slot, unresolved names start directly at the outer
environment without a synthetic local environment node.

Evidence already observed: environment pooling improved call-heavy workloads, including Richards and DeltaBlue, but does not remove environment allocation/borrow semantics.

Current experiment: `NameIc` now guards the immutable `Rc<HashMap<name, slot>>` layout identity. Each dynamic frame precomputes an eight-entry raw view of its live environment chain, so a cache hit can borrow the guarded environment and index its value slot directly without cloning parent `Rc`s or hashing the name again. Cache misses, changed layouts, and deeper chains retain the canonical resolver. A/B evidence is required before acceptance.

Measurement: accepted. `reports/name-ic-chain-ab/comparison.txt` records seven-suite alternating medians of 513.977→529.748 (+3.07% aggregate), with every suite non-negative: Richards +4.05%, DeltaBlue +5.16%, Crypto +0.51%, RayTrace +0.47%, RegExp +0.87%, Splay +3.56%, and Navier–Stokes +7.06%. A focused test verifies that copy-on-write layout mutation invalidates and refills the cached layout identity.

## Non-escaping frame result: accepted

The accepted implementation keeps immutable binding names and slot indices on the
shared `DynJitCode` kernel and instantiates only the mutable value vector per call. The
vector pool has an explicit cleared-slot ownership invariant; a new test proves that
releasing it drops heap values and reacquisition returns only `undefined`. The existing
live-closure test proves that the heap-frame coproduct remains selected when capture is
possible. All 62 release tests and the complete V8v7 semantic smoke pass.

The exact four-repetition, 200 ms-window alternating comparison in
`reports/task13-stack-local-frame-stable-full-ab-4/comparison.txt` raises aggregate
score from 1045.75 to **1151.24** (+10.09%). Richards improves 18.49%, DeltaBlue
25.26%, Crypto 6.79%, RayTrace 10.88%, Earley-Boyer 15.04%, RegExp 0.45%, Splay
7.28%, and Navier–Stokes changes -0.99%. A separate eight-repetition 250 ms RegExp
check in `reports/task13-stack-local-frame-regexp-ab-8/comparison.txt` measured +0.22%.

Accepted binary SHA-256:
`c8befa7fc957ae7520a760999e081821c293670fa31d00529069a4b653c87357`.
The accepted source rebuilds byte-identically to that artifact.
