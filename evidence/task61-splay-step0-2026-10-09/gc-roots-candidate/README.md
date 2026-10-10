# Splay GC root metadata candidate

This is a Splay-only fixed-work gate for a general GC representation change.
It derives whether a shape may contain symbol keys or accessor values when the
shape is created, scans each root-bearing shape at most once per collection,
and records live object shapes during marking. It also avoids empty weak-root
probes and avoids sparse-array side-table probes for non-array cells. The
generic tracing behavior remains for root-bearing shapes.

The source patch is `m1.patch` (base revision `0f980819a5982fd27f09a83fee87d03ba77368ff`,
SHA-256 `dba18864fb66a08f8b5228d2e0a8a4a22b131d177a65b4a6dc38748018adba1a`).
The production candidate binary is pinned at
`target/pinned/d28c6a722fd77b3a52a2a5c31ec455fcf0207609e393ca7f82f96ffd5255b962/quench-node`.
The fixed-work runner and both raw reports record executable hashes and host
details. `summary.json` is the compact machine-readable result.

On the M4/macOS 26.5 host, an 11-round schema-2 gate ran 300 Splay `run()` calls
per process and paired each work process with K=0 setup/teardown. Quench
marginal cycles fell from 10,324,242 to 9,395,225 per run. The paired median
was -9.03% (bootstrap 95% interval -9.45% to -8.52%); marginal elapsed time
fell 8.78% (bootstrap median interval -9.61% to -7.89%). Median maximum RSS
moved by +16,384 bytes, with a paired bootstrap median interval from -32,768
to +196,608 bytes. Splay output matched in all rounds.

Relative to the Step 0 bars (Node `--jitless`: 4,590,844 marginal cycles/run;
Bun no-JIT: 57,950,208 bytes maximum RSS), the speed distance improved from
2.23x to 2.05x. RSS distance is unchanged at 3.76x (217,939,968 bytes versus
57,950,208 bytes). This is progress, not a Splay win.

The three-round all-eight fixed-work guard completed with matching outputs.
Median paired cycle deltas were Crypto +0.64%, DeltaBlue -0.37%, EarleyBoyer
-5.87%, NavierStokes +0.35%, RayTrace -5.54%, RegExp -0.58%, Richards +0.08%,
and Splay -9.43%. Median RSS deltas were within 49,152 bytes in seven fixtures;
RegExp was -442,368 bytes. This is a safety screen, not qualification.

The pre-change ten-second symbolized Splay sample contains 114 samples at
`active_shape_attributes` source line `gc.rs:21`; the candidate has one sample
at that line and one at `gc.rs:30`. These are sampled PCs, not a time-share
estimate. The full before/after profiles are `../splay.sample.txt` and
`splay-m1.sample.txt`.

The allocation-site lifetime census for the same 300-run Splay work shows
2,016,187 of 2,184,763 ordinary-object lifetimes stayed at two properties;
168,495 exceeded the two-value inline capacity. The two dominant two-property
literal sites `(program 13, function 7, pc 6)` and `(13, 7, 24)` account for
1,024,000 and 992,000 lifetimes and remain exactly two properties. The census
does not directly count inline-to-arena migration events, and no per-field-site
receiver/holder hit split was recorded. Those facts therefore do not yet
justify changing field-cache capacity or constructor slack.

Correctness gates passed on this source before timing: runtime unit tests
540/540, Test262 stage 47 47/47, stages 79–81 255/255, and Wasm directives
67,124/67,124. The fixed-work Splay and all-eight outputs also matched. Keep
the Splay lens active; the next mechanism must be selected from a fresh
candidate profile and measured attribution.
