# 306 — Bounded inherited-property guard stencil

Status: complete

Extend the existing cooked own-property observation leaf with the smallest bounded
prototype recipe: an immediate-prototype holder. A unified `PropertyIc` records receiver
shape, slot, and optional holder shape. A null holder shape denotes an own slot; a non-null
holder shape denotes the receiver's current prototype. The rustc/LLVM-cooked stencil
validates that immutable recipe directly and remains in the linked function on a hit.
Empty, deeper, changed, non-object, and ownership-sensitive cases tail-transfer to the
existing semantic kernel.

This is one normalized categorical observation: receiver guard, optional holder guard,
then slot-load result. Its record is reusable data consumed by every property block stencil;
it is not shaped around a source spelling or V8v7 program. Own and inherited lookup remain
two arms with the same entry and continuation contexts.

Do not add a global mutation epoch or retain a raw holder identity. The stencil rereads the
receiver's current prototype and guards its immortal shape; same-shape replacement is safe
because shape fixes key-to-slot layout, while shadowing/add/delete changes a guarded shape.
Use named layout offsets and assert the Rust/AOT layout contract through execution tests.

Acceptance: warmed inherited hits for the existing load/store/jump and predicate stencils
stay in cooked machine code; receiver shadowing, intermediate shadowing, prototype relink,
deep chains, empty caches, heap-owned results, and collection invalidation rejoin canonical
semantics; release/stress suites pass; selected-suite and complete V8v7 A/B improve without
unacceptable code growth.

Primary references: Deegen's inline IC slab and impossible-key design
<https://arxiv.org/abs/2411.11469>; JavaScriptCore structure-chain guards and watchpoints
<https://webkit.org/blog/10308/speculation-in-javascriptcore/>; SpiderMonkey CacheIR's
guard/pure/result effect discipline
<https://firefox-source-docs.mozilla.org/js/cacheir.html>.

## Result: correctly wired, rejected and removed

The candidate unified the own and immediate-prototype recipes in one three-word
`#[repr(C)] PropertyIc`. `get_static_cached` published depth-one holder shape/slot facts;
the AOT `read_cached_property` reread the receiver's current prototype and guarded its
shape inside every existing cooked property block. Same-shape holder replacement, holder
shape change, receiver shadowing, deep-chain fallback, and direct cooked execution were
covered. Normal and stress-GC suites passed, including the new focused tests.

The frozen v2 binary
`229f166ff88392f425b03649cfe8018494e35a86dad3f252701f34dd3419b080`
was compared with Task 305's frozen baseline over five alternating 500 ms full-suite pairs.
Aggregate V8v7 changed from 2,064.35 to 2,059.08 (-0.26%). Richards was +0.77%, RegExp
+2.93%, RayTrace -2.61%, and all components remained within the standing -5% floor. The
catalog grew from 12,172 to 12,372 bytes (+200, +1.64%), while symbol and relocation counts
were unchanged. Evidence is in `reports/task306-depth1-property-v2-full-ab-5`.

Profiling explains the neutral result. The three existing coarse stencil patterns execute
about 541,020 times per 100 ms in Richards but only 68 times in DeltaBlue and zero in
RayTrace/Splay; Richards' selected sites are overwhelmingly own-field loads. In contrast,
500 ms inherited-cache and direct-call populations are 8.56m/7.75m in Richards,
12.62m/10.19m in DeltaBlue, and 1.97m/1.82m in RayTrace. Inherited traffic is primarily
method lookup feeding calls, outside these standalone patterns.

The candidate and focused-only tests were removed. Do not add another isolated inherited
property stencil before Task 146/163 can consume the property proof as the first stage of a
method-call/frame/return continuum.
