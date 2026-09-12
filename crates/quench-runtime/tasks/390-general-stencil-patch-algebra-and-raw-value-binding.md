# 390 — General stencil patch algebra and raw-value binding

Status: in_progress

Replace the rustc stencil cooker's parallel special-case relocation arrays with one
immutable patch manifest:

```text
PatchSite { offset, encoding, binding }
```

`encoding` describes the physical AArch64 field (`Branch26`, unsigned load/store
immediate, add immediate, mov-wide lane, raw word, or pointer). `binding` describes the
semantic demand (`Next`, `Slow`, `Taken`, operand role, site advance, or named raw value).
The cooker produces this quoted data. A stencil instance supplies values. The only
mutation occurs when `StencilTemplate::instantiate` applies the bound patches to its
private byte copy. Shared kernels and templates remain immutable.

The experimental first consumer is a family of one-through-four-lane burned
numeric-literal stencils. Pinned rustc emits a `MOVZ` followed by only the required
`MOVK` lanes, a separately patched destination slot, the ordinary patched site advance,
and a tail branch. The value and destination come from the quoted numeric `RegionOp`; no
runtime observation, source identity, hotness counter, or benchmark-specific selector is
involved. Automatic selection is currently disabled because neither the family nor the
one-lane subset passed its whole-suite performance gate.

Preflight: the old `deegen_region_load_literal` loads both destination and raw bits from
`InlineSite`. The replacement removes those two dependent site loads but adds three
mov-wide instructions versus a single loaded word. It is primarily an enabling patch
primitive for future coarse I32/F64 regions, so it must still pass the whole-suite screen
to remain wired in production.

Implemented evidence:

- `stencil-aot/patch_schema.rs` is the shared encoding/binding vocabulary used by the
  cooker and runtime linker.
- The generated `RustcStencil` contains only `bytes` plus `patch_sites`; next/slow/taken,
  operand, site-advance, and raw-value views are derived from that one fact.
- The linker specializes each encoded manifest demand once into a raw word patch before
  `StencilTemplate::instantiate`; instantiation is one byte copy followed by direct
  word/pointer writes. Encoding validation is not repeated in the copy loop.
- The differential cooker audit passes with 155 templates, 47 placeholder-sensitive
  templates, and 94 audited patch fields. The extra six fields are destination, site
  advance, and four raw-value lanes.
- Release tests execute a nontrivial F64 literal through the four-lane patched machine
  stencil and verify the destination value. All 146 release tests pass.
- The final infrastructure-only binary is 16,384 bytes smaller than the accepted Task 381
  baseline (3,466,032 versus 3,482,416 bytes).

Measured selector experiments:

- Uniform four-lane burning was rejected at -0.58% aggregate in the initial five-pair
  screen (`reports/task390-raw-literal-ab-5/comparison.txt`).
- A diagnostic whole-suite census found 219 one-lane, 11 two-lane, four three-lane, and
  zero four-lane linked literal instances. The census is in
  `reports/task390-lane-literal-census/`; diagnostic counters are not selection inputs.
- Selecting the minimum lane count for every encodable literal was rejected at -1.49%
  aggregate, including component-floor failures, in
  `reports/task390-lane-literal-ab-5/comparison.txt`.
- Selecting only one-lane literals was effectively tied in the first screen (-0.06%), but
  a longer nine-pair 500 ms screen was -0.50%; it was therefore rejected. Evidence is in
  `reports/task390-one-lane-literal-ab-5/` and
  `reports/task390-one-lane-literal-ab-9x500/`.
- With literal selection disabled, the first generic-apply linker failed the nine-pair
  exact gate at -0.77%, 95% paired bootstrap CI [-5.23%, +2.76%]. Evidence is in
  `reports/task390-patch-algebra-final-exact-9/comparison.md`.
- The subsequent one-pass word-patch specialization removes runtime encoding checks and
  passes correctness/audit validation. Its quick screen was -0.92%, but the run contains
  clear simultaneous host collapses and a Splay component-floor outlier. A post-run process
  audit found unrelated `rustc`, Test262, and node-test processes consuming roughly three
  CPU cores, so this is pre-screen evidence only
  (`reports/task390-one-pass-patch-ab-9x500/`). A clean exact gate on a quiescent host is
  still required before this task can be complete.

Acceptance: all release tests and the cooker audit pass; disassembly contains no site
load for destination or literal bits; a randomized whole-suite comparison has no
component-floor violation and a nonnegative aggregate result. If the isolated literal
consumer fails that gate, retain the general patch algebra only if an A/B comparison
proves it performance-neutral, and disable/revert the literal selection while preserving
the measured rejection in this task.
