# 288 — V8's crypto win is inlining away the call boundary, not loop vectorization

Status: planned

[[253]]'s actual measured V8 profile (`reports/task253-comparative/v8-js-top.tsv`)
overturns the working assumption behind [[246-carry-save-bignum-multiply-vectorization]]
and sharpens [[251-cross-stencil-register-allocation-ceiling]] with decisive, specific
evidence instead of a general hypothesis. The profile shows:

```
node crypto 3574 48.5% *  montReduce    crypto.js:583
node crypto 2088 28.3% *  bnpSquareTo   crypto.js:431
node crypto  577  7.8% *  bnModPow      crypto.js:1098
node crypto  507  6.9% *  bnpMultiplyTo crypto.js:415
node crypto   18  0.2% *  am3           crypto.js:108
```

`am3` — the exact digit-multiply-with-carry loop [[246]] targeted for carry-save SIMD
restructuring — accounts for **0.2%** of V8's optimized-tier samples, even though it is
the innermost, most-executed loop in the whole BigInteger multiply/square pipeline.
V8's optimizing compiler has inlined `am3`'s entire body directly into `bnpSquareTo`
and `bnpMultiplyTo`, which is *why* those two functions show 28.3% and 6.9%
respectively instead of `am3` itself showing a large number — the call boundary between
caller and digit-multiply loop has been erased entirely, letting one register-allocated
function body handle the whole squaring/multiplication pass with no call overhead, no
argument marshaling, and no separate stack frame for the hot loop.

**This means [[246]]'s premise needs re-ranking, not abandoning.** Carry-save
vectorization may still be a real, additional win once the call boundary is gone, but
the *primary* lesson from this measurement is that V8's crypto advantage is dominated by
cross-function inlining eliminating exactly the kind of boundary
[[251]]'s hypothesis is about, not by a data-parallel restructuring of the carry chain.
This is concrete, corpus-specific confirmation of [[251]]'s general architectural
concern and should reorder priority: closing the `am3`-into-caller inlining gap (via
[[20-inline-via-node-composition]]/[[163-caller-customized-stencil-images]], or
whichever composition mechanism can fuse a helper stencil directly into its caller's
region at cook time) is very likely a larger, more direct win for crypto than
[[249]]/[[246]]'s carry-save scan restructuring, and should be attempted and measured
*first*.

Concrete steps:
1. Confirm directly (via deegen's own disassembly, not inference) whether `am3`'s
   deegen-side equivalent is currently compiled as a separate call or already inlined
   into its caller region — do not assume either way.
2. If it is a separate call, apply whichever cross-function inlining/caller-customization
   mechanism is furthest along ([[20]], [[163]], or the coarse-region composition family
   from [[128]]/[[131]]/[[284]]) to fuse it into its caller, and measure the crypto suite
   specifically before attempting [[246]]'s carry-save restructuring.
3. Only after step 2's measurement, evaluate whether [[246]]'s carry-save vectorization
   adds further gain on top of the inlined form, or whether inlining alone closes most
   of the gap — this ordering avoids attributing a win to vectorization that was actually
   just inlining, a real risk given how easy the two are to conflate without this
   specific profile evidence.

Acceptance: `am3` (or its deegen equivalent)'s call-boundary status is confirmed by
disassembly; inlining is attempted and measured on crypto specifically before carry-save
restructuring is attempted; the resulting measurement states clearly how much of
crypto's gap-to-target is closed by inlining alone versus what remains for [[246]]'s
vectorization to address, so future work is correctly attributed rather than credited to
the wrong mechanism; [[251]]'s register-allocation-ceiling hypothesis gains this
measurement as its first piece of suite-specific confirming evidence.

Source: `reports/task253-comparative/v8-js-top.tsv` (this project's own measured V8
profile of the actual crypto.js suite, produced by [[253]]).
