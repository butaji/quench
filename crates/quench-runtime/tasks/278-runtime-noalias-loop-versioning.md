# 278 — Runtime no-alias versioning for multi-backing loops

Status: planned

Refine a numeric loop with uncertain memory aliasing into a coproduct of two ordinary
stencil loops:

`MayAliasGamma -> DisjointGamma + MayAliasGamma -> ExitGamma`.

Derive one immutable `AliasPartition` from Task 171's access graph, Task 173's memory
locations, and Task 175's affine access envelopes. Each partition member names a backing
identity plus the minimum and one-past-maximum byte range touched by the loop. The
preheader emits only the pairwise range checks required to separate read/write groups.
Distinct VM array backings may use identity checks initially; the fact stays range-based
so future slices/views do not require a second representation.

The disjoint arm selects rustc/LLVM-cooked templates whose pointer/slice arguments carry
the no-alias contract, allowing invariant-load motion, store forwarding, and Task 39
vectorization across multiple arrays. Constructing Rust references from raw backings is
allowed only after the guard proves their alias rules; the conservative arm retains raw
ordered accesses and full JavaScript semantics. A failed guard is normal control flow,
not deoptimization or interpreter fallback.

Keep `MAX_ALIAS_GROUPS_PER_LOOP`, `MAX_RUNTIME_ALIAS_CHECKS`, and
`MAX_NOALIAS_VERSION_BYTES` as named policy constants. Do not clone a loop when the
estimated removed memory dependencies cannot repay the guard and code-size cost. No
runtime execution count, benchmark name, source location, or property spelling enters
the decision.

Acceptance: differential tests cover identical arrays, distinct arrays, overlapping
future views, empty/zero-trip ranges, read-only sharing, writes, holes, backing growth,
exceptions, and side exits; disassembly proves the disjoint AOT template has no alias-
forced reloads and vectorizes an eligible multi-array loop; counters report checks,
disjoint hits, conservative selections, and bytes cloned; Crypto/Navier-Stokes plus full
V8v7 alternating A/B improve without a component-floor violation.

Primary sources:

- LLVM's loop-versioning algorithm and legality checks:
  <https://llvm.org/doxygen/LoopVersioningLICM_8cpp_source.html>
- LLVM runtime pointer-check groups and access envelopes:
  <https://llvm.org/docs/doxygen/LoopAccessAnalysis_8h_source.html>
- LLVM vectorizer runtime pointer checks:
  <https://llvm.org/docs/Vectorizers.html>

