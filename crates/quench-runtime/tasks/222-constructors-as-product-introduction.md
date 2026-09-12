# 222 — Constructors as categorical product-introduction morphisms

Status: planned

The second-largest shape in the whole-suite residual profile
(`reports/task132-residual-generic-blocks.jsonl`) is
`LoadLocal,LoadLocal,SetStatic,LoadLocal,LoadLocal,SetStatic,Return` at 594,143
entries — a function that sets exactly two/three fields on `this` and returns, appearing
in the "static-property" family that totals 4,319,715 entries, the single largest
semantic family in the whole profile. Confirmed against the actual source at
`/private/tmp/js-engine-benchmark/v8-v7/raytrace.js:223-233`:

```js
initialize : function(x, y, z) {
    this.x = (x ? x : 0);
    this.y = (y ? y : 0);
    this.z = (z ? z : 0);
},
copy: function(vector){
    this.x = vector.x;
    this.y = vector.y;
    this.z = vector.z;
},
```

**Categorical framing.** A function whose entire body is "set field 1, set field 2, ...,
set field N" (optionally through a default/ternary per field) and then implicitly
returns is a *product-introduction morphism*: given values `(v1, ..., vn)`, it produces
the unique morphism into the product object `A1 × A2 × ... × An` whose composition with
each projection `πi` recovers `vi` — this is literally the universal property of a
categorical product, and it is what licenses treating the whole function as a single
struct-literal write rather than as N sequential, independently-guarded property
stores. The current lowering visibly treats it as the latter: each `this.x = ...` goes
through [[08-property-inline-caches]]'s per-store guard machinery independently, once
per field, even though every field's target shape and slot are simultaneously and
statically known the moment the first one is.

This is a distinct, narrower case than [[130-static-property-region-stencils]]'s general
region-of-static-property-ops machinery: the useful additional fact here is *totality
and shape-finality* — every field the target shape has gets written, in one pass, with
no interleaved read, branch, or call that could observe the partially-constructed
object, and the shape after the last store is exactly the target's known-final shape
(not a series of transitions arrived at one field at a time). That admits compiling the
whole function to one aggregate struct write (guard the target shape once, write all N
fields as one region, transition-if-needed exactly once) instead of N independent
guarded stores — [[199-copy-on-write-array-literal-kernels]] and
[[206-aggregate-array-literal-materialization]] already do the array-literal analog of
this; this task is the equivalent for object/`this`-field-setting functions specifically.

Concrete steps:
1. Recognize the pattern in the compiler: a function body consisting entirely of
   sequential `this.<field> = <expr>` stores (each `<expr>` itself simple — a parameter,
   a ternary-defaulted parameter, or a read from another object of the same shape, as in
   `copy`) with no other observable effect before the implicit/explicit return.
2. Prove (statically, at compile time, not by execution count) that all fields written
   equal the target shape's complete field set in this-function's-defining-shape,
   reusing [[07]]/[[135]]'s shape machinery.
3. Compile the whole body as one aggregate-write stencil: one shape guard on `this`
   (or one shape assignment if this is effectively acting as a constructor/initializer),
   one contiguous multi-field store, one return — instead of N chained per-field IC
   stores.

Acceptance: `Vector.prototype.initialize` and `.copy` (and any other source site
matching the pattern across the suite) compile to one aggregate-write stencil, verified
by instruction count against the current N-independent-stores baseline; the
`LoadLocal,LoadLocal,SetStatic,LoadLocal,LoadLocal,SetStatic,Return` residual shape's
entry count drops in a rerun of [[132]]'s profiler; RayTrace's score improves in an
alternating A/B with every suite at or above the standing floor; a function that does
*not* satisfy the totality/no-interleaved-effect precondition (partial field set, or a
call between stores) is correctly excluded and falls back to the existing per-field IC
path, verified by a negative test.

Source: `/private/tmp/js-engine-benchmark/v8-v7/raytrace.js` (local V8v7 corpus
checkout, lines cited above); residual evidence:
`reports/task132-residual-generic-blocks.jsonl`,
`reports/task132-residual-generic-blocks-summary.txt`.
