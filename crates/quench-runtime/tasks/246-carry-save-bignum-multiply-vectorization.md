# 246 — Carry-save vectorization for crypto.js's schoolbook bignum multiply

Status: planned

Refines [[39-vectorized-numeric-stencils]] with the specific obstacle its current text
does not name, and is the concrete corpus-verified instance of
[[249-associative-scan-monoid-recognition]]'s general carry-chain specialization —
this task supplies the evidence and acceptance criteria; [[249]] supplies the reusable
mechanism, so nothing here should be implemented as a one-off `am3`-shaped rewrite.
crypto.js's actual hot inner loop is not a plain elementwise map/reduce
that autovectorizes directly. `crypto.js:1677` calls `setupEngine(am3, 28)`
unconditionally — a single, statically-known digit-multiply variant, not a runtime
platform choice — so the entire benchmark's multiply cost is exactly one loop shape,
`am3` (`crypto.js:108-121`):

```js
function am3(i,x,w,j,c,n) {
  ...
  while(--n >= 0) {
    var l = this_array[i]&0x3fff;
    var h = this_array[i++]>>14;
    var m = xh*l+h*xl;
    l = xl*l+((m&0x3fff)<<14)+w_array[j]+c;
    c = (l>>28)+(m>>14)+xh*h;
    w_array[j++] = l&0xfffffff;
  }
  return c;
}
```

The variable `c` (carry) computed at the end of iteration `i` is consumed at the start
of iteration `i+1` — a genuine loop-carried dependency chain, not an independent
per-element computation. Naive SIMD lowering (process 4/8 lanes of `this_array[i]`
simultaneously) is unsound here without restructuring: it would require each lane's
carry to be known before the next lane's addition, defeating the parallelism. This is
exactly why a plain "autovectorize the loop body" pass — which is roughly what
[[39]]'s current text implies — cannot correctly speed this loop up as written; it needs
the specific, well-established bignum-library technique instead: **carry-save
vectorization** (compute the wide/high/low partial products for all lanes independently
via SIMD, accumulate them into two or three separate running sums — sum and carry —
without propagating carries lane-to-lane during the parallel phase, then perform one
final, cheap sequential carry-propagation pass at the end). This is how GMP, OpenSSL,
and every serious bignum library vectorizes exactly this shape; it is a known,
"solved elsewhere" algorithmic restructuring (matching this session's `fancy-regex`
precedent of reusing an established solution rather than inventing one), not a novel
compiler transformation to design from scratch.

Concrete steps:
0. Prove `am3`'s carry-combine step forms a monoid (associative, with identity 0) per
   [[249]]'s general recognition rule, before proceeding — this is the specific
   instance of [[249]]'s step 1, not a separate ad hoc justification.
1. Recognize the `am3` loop shape specifically at the stencil-selection level: a
   fixed-stride array loop over a guarded packed-numeric array
   ([[32-element-kind-guarded-arrays]]) whose body computes a carry value consumed by
   the next iteration and produces one output-array write per iteration — a distinct,
   checkable pattern from [[39]]'s general elementwise map/reduce target.
2. Lower the recognized shape to a carry-save-restructured native loop: SIMD-computed
   partial products across `PACKED_WIDTH` lanes, deferred carry accumulation, one final
   sequential carry-propagation pass — not a naive per-iteration SIMD lowering of the
   original carry-chained form.
3. Verify numerically against the existing scalar path on the full BigInteger
   correctness test surface (multiply, square, modular reduction all route through this
   or the closely related `am`-family functions) before accepting any measured gain,
   since a carry-save restructuring is easy to get subtly wrong at the boundary lanes.

Acceptance: `am3`'s recognized shape compiles to a carry-save-vectorized native loop,
verified by disassembly showing SIMD instructions on the partial-product computation;
BigInteger multiply/square/modular-reduction correctness tests pass exactly, including
edge cases at digit-array length boundaries not evenly divisible by the SIMD width;
alternating A/B on the crypto suite specifically shows a measured gain; the general
[[39]] elementwise map/reduce path remains unaffected for suites that do not have this
carry-chained shape (navier-stokes, confirmed to route through the simpler path).

Primary sources: standard bignum-library carry-save/carry-propagate-deferred
vectorization technique, as implemented in GMP and OpenSSL's bignum multiply routines
(no single canonical paper — this is established practitioner technique; cite the
`am3` source itself, `/private/tmp/js-engine-benchmark/v8-v7/crypto.js:108-121`, as the
concrete shape this task targets, local V8v7 corpus checkout).
