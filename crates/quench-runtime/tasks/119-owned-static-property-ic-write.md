# 119 — Owned static-property IC write

Status: complete

Runtime profiling records roughly 6.3 million `SetStatic` operations across the current
V8v7 measurement. The canonical semantic kernel clones the source register into an
owned `Value`, passes it by reference to `set_static_cached`, clones it again into the
property slot on every monomorphic hit, and then drops the temporary owner. For
immediates this still executes redundant tag dispatch; for heap values it performs a
balanced but unnecessary Rc increment/decrement pair.

Change the immutable property-IC kernel to consume-or-return ownership:
`Value -> Result<(), Value>`. A cache hit moves the supplied value into the shaped slot
with the canonical `Value::overwrite`; a miss returns that exact owner for the generic
`set_prop` path. The source register remains unchanged because the first clone is the
explicit JS copy from register storage. No branch duplicates ownership semantics.

This is the coproduct eliminator at the kernel boundary: `Cached + Generic` has one
owned input and exactly one consuming output path. It is bytecode-general, always used,
and independent of source identity, literal values, or runtime heat.

Acceptance: ownership tests cover cached heap replacement and miss fallback without
leaks or double drops; all release tests and complete V8v7 smoke pass; an exact
alternating six-run A/B against [[110-constant-condition-control-stencils]] improves
aggregate without crossing the standing -5% per-suite floor. Revert and preserve
evidence otherwise.

## Result: accepted

The property IC now consumes one owned `Value` and returns it unchanged on a miss.
Monomorphic own-property hits move that owner directly through `Value::overwrite`; the
generic fallback receives the returned owner. A focused test verifies both Rc paths.
All 47 release tests pass and the complete smoke is
`reports/owned-static-property-ic-write-smoke.jsonl`.

The initial Splay smoke outlier was checked independently with six alternating runs in
`reports/owned-static-property-ic-write-splay-ab-6/comparison.txt`; it was noise, and
the candidate improved Splay by 0.32%. The exact full six-repetition comparison in
`reports/owned-static-property-ic-write-ab-6/comparison.txt` passes the standing gate:

- aggregate: 833.546 -> 834.119 (+0.07%);
- Richards +1.94%, DeltaBlue +0.45%, Splay +1.18%, Navier-Stokes +1.54%;
- Crypto -1.20%, RayTrace -2.29%, Earley-Boyer -0.12%, RegExp -0.87%, all above
  the -5% component floor.

Accepted binary SHA-256:
`e3f8253c4a59bc80bc1c3b79d29e7f85cd7f46f6cc29c64765c597cfe7218346`.
The accepted executable is preserved as
`/tmp/deegen-after-owned-static-property-ic-write` (2,991,264 bytes).
