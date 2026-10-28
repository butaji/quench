# TSF indirect-call attribution

This is a one-round diagnostic run. It passed JetStream's stock validation and
reported equal observable output. It is not a qualification result: call-site
counters were enabled, the source worktree was dirty, and the Node comparator
was v24.19.0 (the earlier qualification used v26.10.0).

At measurement time, the source revision was
`cec64be7da00201d3996efb171c22a25584a5232`, with a diff to
`crates/quench-runtime/src/vm/compiled.rs` and `vm/wasm.rs` whose SHA-256 was
`2686f9e2eecdb85992c87445f202043746a6e0edb78745149fa93597037ca6fe`. The
Quench and benchmark binary hashes are `836caf32be846ffdd3b7ef4807360a00aafa04dca9b796ccc9ddc58d4d32f0a5`
and `74e6038e536798342f32a027f2eb583e635c891b22a311c8dbf56a4e9278a3a4`.
The complete raw report is
[`tsf-call-sites-jit-diagnostics.json`](tsf-call-sites-jit-diagnostics.json)
(SHA-256 `e0dd4a2c9ef012b1968cf150a7937586736f1521aa48e011208ee5422d16dd18`).

Quench recorded 2,869,000 compiled reference callouts and 27,006 imported-call
callouts. Of the reference calls, 2,859,000 (99.65%) resolved to a same-program
compiled target; 10,000 took the VM fallback or trap path. The hottest site is
runtime function 221 at PC 63: 2,663,108 calls, all to compiled targets. The
pinned TSF symbol map names function 221 `read`. The other prominent native
sites include function 51/PC 23 (79,350 calls) and function 115/PC 19 (33,600).
The diagnostic compilation produced 156 of 244 functions, 178,760 code bytes,
180,224 mapped bytes, and reported 113.8 ms compile time.

The one-round diagnostic scores/RSS were Quench 14.41/224.4 MiB, Bun 1.4.2
218.93/134.5 MiB, and Node 24.19.0 194.87/79.7 MiB.
These instrumented values are included only to identify the run; do not compare
them to qualification data.

The result points to the VM transition as the next high-value target. The
existing same-program native-target path still converts each raw Wasm argument
to a VM `Value`, calls `run_compiled`, reconstructs raw arguments, and enters a
second wrapper. A same-instance direct call through the existing fixed entry
wrapper can skip that conversion/setup for the 2.66-million-call site while
leaving table bounds, null, and signature checks in the resolver. Cross-instance
and uncompiled targets must keep the current fallback.
