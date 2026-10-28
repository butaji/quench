# Richards opening: Linux, no JIT

User-directed parallel assignment: v2-cloud stays on Richards (TOD-12).
Source fdc843033 includes the checked c* integration; this is a baseline and
attribution package, not a retained optimization or benchmark win.

The clean stock report has 11 interleaved rounds, all valid/output-equal:
Quench Score417 /44,572,672B RSS; QuickJS871 /2,179,072B; Bun1504 /17,866,752B;
Node1454 /46,153,728B. Exact host, cgroup, hashes and no-JIT proofs are in JSON.
Single-fixture diagnostic, qualification_ready=false. Quench wins no Score
round against any reference and wins RSS only against Node (11/11).

The stock runner materializes /tmp/*.js; NodeHost selects CommonJS there.
Attribution uses that same entry policy and exact fixed-work template, K=0
and K=100 with Richards Setup/run/TearDown preserved. Callgrind gives
102,307,782.95 marginal instructions/run. Self-function deltas reconcile
exactly. The all-program aggregate profile counts 496,127 virtual opcodes/run,
185,154 GetField, 41,306 LoadCapture, 40,466 Call. Program-specific counts are
summed across113 installed programs; global counters are read once. The method
cache handler has zero hits/misses, so cache expansion does not target this path.
Instrumented totals are attribution, never Score/RSS qualification.

The first attribution files were accidentally materialized under repository
package.json type:module. Their separate counterfactual directory preserves
their evidence and labels; none is stock attribution. The corrected CommonJS
profiles and Callgrind are authoritative here. Dynamic capture counts agree,
but the instruction costs and eligible root-binding guards differ.

Floor RSS references (11wait4/GNUtime samples): B0 empty Rust966,656B; Q0
QuickJS empty1,744,896B; F quench-node empty42,921,984B; K1 bare VM empty
9,482,240B. K1 stock is one script-goal counterfactual (11,354,112B RSS), not
an alternative measured product. Wall measurements overlapped builds and are
not speed evidence. K2-K4 are unmeasured; no irreducible floor is claimed.

Memory-only instrumentation is separate from aggregate profiling, which adds
substantial counter storage. Its startup checkpoints show Fs creation within
unconditional VFS bootstrap +15,086,496B live allocator bytes, +16,113,664B RSS;
VFS polyfill +3,849,936B live bytes, +3,280,896B RSS. These are instrumented
phase deltas, not a claim that production will save those exact amounts.
The VFS feature is disabled by default. Upstream gates its bootstrap; canonical
Buffer/Fs aliases and default Fs methods must survive any deferral.

Diagnostic source adds only profile-memory checkpoints and an opt-in
profile-aggregate all-program report. Default execution is unchanged.
