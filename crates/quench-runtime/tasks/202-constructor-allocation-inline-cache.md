# 202 — Constructor allocation and direct-entry inline cache

Status: in_progress

Add a first-class `ConstructRecipe` beside ordinary call recipes. A cache case contains
the actual constructor identity (or shared function image plus current closure context),
initial object shape/layout, exact arity, direct callee entry, continuation, and the
JavaScript constructor-result rule. Its fast composition is:

`guard callee ; allocate known shape ; initialize receiver ; enter constructor ; select object-or-receiver result`

The monomorphic case is a patched stencil arm operating on the surrounding machine
context. Bounded polymorphic cases compose as additional arms; the megamorphic case uses
a shared immutable kernel. Allocation uses Tasks 148/156/162 rather than `Rc`, and the
callee transfer uses Tasks 146/187 rather than `Vm::construct` followed by generic
`Vm::call`. A failed guard or allocation branches to the canonical generic stencil edge,
never an interpreter.

All policy limits—arm count, inline bytes, exact-arity variants, and layout classes—are
named constants. Selection depends only on semantic cache facts, never source spelling,
benchmark identity, or execution count.

Acceptance: successful stable constructor sites allocate/initialize and enter user code
without a Rust call/construct helper; constructor replacement, closure constructors,
polymorphic sites, primitive/object explicit returns, exceptions, prototype mutation,
and allocation failure retain JavaScript semantics; RayTrace/Splay/DeltaBlue/Earley-Boyer
construction counters fall and complete alternating V8v7 A/B improves.

Primary sources: JavaScriptCore's polyvariant/constructor discussion
<https://webkit.org/blog/3362/introducing-the-webkit-ftl-jit/>, V8 create lowering
<https://v8.dev/blog/slack-tracking>, and SpiderMonkey allocation-site ICs
<https://firefox-source-docs.mozilla.org/js/how-we-optimize.html>.

## First implementation slice

`Construct` now uses the same function-identity `CallIcSite` as ordinary calls and a
borrowed `RegisterArguments` view instead of allocating and populating a temporary
`Vec<Value>` at every construction. Repeated monomorphic construction can therefore
reuse the compiled function image and closure environment through the existing cache.
The constructor result rule now preserves an explicit object, function, or RegExp
returned by user code; primitive user returns retain the freshly allocated receiver.

This slice deliberately does not claim the task complete: receiver allocation still
uses the generic object path, construction is still entered through the semantic
kernel, and no direct `ConstructRecipe` stencil arm exists yet. Measure this slice before
adding known-shape allocation and direct-entry composition.

The five-repetition complete-suite A/B is recorded in
`reports/task202-borrowed-construct-full-ab-5/comparison.txt`. It improved the aggregate
from 1765.37 to 1792.18 (+1.52%): RayTrace +5.93%, Earley-Boyer +2.60%, RegExp +3.32%,
Crypto +1.90%, DeltaBlue +0.59%, Navier-Stokes +0.54%, Richards -0.14%, and Splay
-2.37%. Retain the slice. The constructor-focused four-suite aggregate was +2.16%.

## Native receiver-allocation elision

Native constructors now bypass creation of the generic receiver that their current
semantics never observe: each native constructor allocates and returns its own result.
The generic construct kernel passes `undefined` as the unused receiver on that edge,
while user constructors retain the normal freshly allocated receiver and JavaScript
object-or-receiver result rule. This removes an immediately discarded object allocation
without specializing by constructor name or benchmark identity.

The five-repetition complete-suite A/B is recorded in
`reports/task202-native-receiver-elision-full-ab-5/comparison.txt`. It improved the
aggregate from 1749.22 to 1763.54 (+0.82%). Splay improved 5.93%, RayTrace 2.69%, RegExp
1.21%, and Richards 0.57%; DeltaBlue, Crypto, Earley-Boyer, and Navier-Stokes were
negative. Retain the slice as a small, noisy aggregate improvement. It does not complete
Task 202 because native construction still uses the shared semantic kernel and user
construction still lacks known-shape allocation and a direct-entry stencil arm.
