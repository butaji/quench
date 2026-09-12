# 176 — Partial escape analysis with edge materialization

Status: planned

Refine Tasks 24 and 33 from whole-function, all-or-nothing escape to control-flow-
sensitive partial escape. Represent an allocation as virtual field values while it is
nonescaping. At an edge that stores, returns, captures, or passes it to an unknown callee,
compose one materialization stencil from virtual fields to a real shaped object; other
paths remain allocation-free.

Task 164's canonical frame state records whether each object is virtual or materialized.
Task 172 owns field lifetimes, Task 173 supplies heap-effect facts, and Task 177 prevents
known noncapturing callees from forcing materialization. Materialization is explicit
edge data and a shared kernel/template family, never hidden mutation in an analysis pass.

Acceptance: branch-local escape tests materialize only on the escaping edge; identity,
aliasing, mutation, exception, and repeated-loop materialization remain correct;
allocation counters fall for RayTrace vector/color chains and DeltaBlue temporaries;
complete V8v7 alternating A/B improves.

LuaJIT's allocation-sinking optimization is the same mechanism under a different name:
sink an allocation to every side exit where its value actually escapes, driven by store
forwarding plus alias/escape analysis, rather than eagerly materializing on entry. Its
practical lesson is that store-forwarding and sinking must be solved together — a
materialization stencil that recomputes fields from stale forwarded stores instead of
the current virtual-field values would silently reintroduce the allocation it was meant
to elide.

Do not restrict the analysis to isolated acyclic objects. JavaScriptCore describes its
production pass as a must-points-to analysis able to sink whole allocation graphs,
including cycles. Represent a virtual allocation graph plus field-state map as canonical
quoted data; each escaping edge derives a materialization script from that graph. This
preserves identity and cycles without eager allocation, and gives one place to encode
array-hole state rather than overloading a numeric NaN sentinel during materialization.

Sources:
<https://ssw.jku.at/Research/Papers/Stadler14/Stadler2014-CGO-PEA.pdf>;
LuaJIT allocation sinking: <https://github.com/tarantool/tarantool/wiki/LuaJIT-Allocation-Sinking-Optimization>.
JavaScriptCore allocation sinking and exit materialization:
<https://webkit.org/blog/10308/speculation-in-javascriptcore/>.

## Hole-state correctness constraint

Array holes are a distinct virtual-field coproduct case, never an F64 NaN value. Every
materialization script must map `Hole` to an absent element and `Number(NaN)` to a
present numeric element. Preserve this distinction across phis, loop-carried virtual
state, boxed exits, and typed-backing widening. A 2026 JavaScriptCore bug fixed precisely
this class of error when its internal PNaN hole marker was boxed as observable NaN:
<https://bugs.webkit.org/show_bug.cgi?id=312664>.

## Round-twenty-six IC-expansion prerequisite

Do not classify a property operation as an opaque escape merely because its original JS
semantics are dynamic. First expand a validated IC case into explicit `CheckShape`,
`LoadOffset`, `StoreOffset`, and `TransitionShape` operations with a general side exit. The
escape pass can then forward virtual fields and sink the allocation through those pure/exact
effects. JavaScriptCore demonstrates this ordering for `delete`: inlining the IC case exposes
simple shape/offset operations, redundant checks disappear, and object allocation
elimination can reduce the allocation to a zero-code phantom:
<https://webkit.org/blog/10298/inline-caching-delete/>.
