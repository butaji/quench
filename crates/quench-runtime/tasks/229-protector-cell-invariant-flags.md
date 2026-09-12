# 229 — Protector-cell invalidation flags for builtin fast-path invariants

Status: planned

Cross-runtime survey finding: V8 (protector cells), and the equivalent mechanism under
other names in JSC and SpiderMonkey, guard every builtin fast path (`Array.prototype`
methods, `for-of` over a plain array, `instanceof` against an unmodified prototype
chain) not with a per-call check of the actual condition, but with **one global boolean
cell per invariant** that starts "valid" and is flipped to "invalidated" exactly once,
the first time any code does something that could violate it (assigns to
`Array.prototype`, adds an indexed accessor to `Object.prototype`, redefines a
well-known builtin). Every fast-path stencil checks its relevant protector cell — one
memory load and branch, O(1) regardless of prototype chain depth or program size — and
falls back to the fully generic path only once, ever, after the one-time flip; before
that, the fast path proceeds with zero per-call verification that the invariant still
holds.

**Why this project needs it and does not yet have it.** [[212-guarded-dense-array-builtin-kernels]]
(already planned) wants native kernels for `Array.prototype` methods on guarded dense
arrays, but a native `push`/`shift`/`map` kernel is only correct if the receiver's
*actual* `Array.prototype.push` (etc.) has not been monkey-patched — and neither [[212]]
nor any other planned task states how that gets checked. Checking it per-call by walking
the prototype chain would defeat the entire point of a native kernel (turning an O(1)
push into an O(chain depth) push); not checking it at all is a correctness bug (a
program that reassigns `Array.prototype.push` would silently keep running the native
fast semantics). The protector-cell pattern is the standard answer: pay for the check
once per invalidating mutation (astronomically rare in real programs), not once per
fast-path call (astronomically common).

Concrete steps:
1. Enumerate every fast-path invariant this project's guarded builtin kernels
   ([[212]], and any future `Map`/`Set` fast paths from [[91]]) actually depend on
   beyond the receiver's own shape: "`Array.prototype` methods are unmodified,"
   "`Array.prototype` has no indexed accessor," "no object in a guarded array's
   prototype chain has an indexed property" (the classic sparse-array-via-prototype
   trap), and any equivalent for [[91]]'s `Map`/`Set` kernels.
2. Give each invariant one global cell (a `Cell<bool>`/atomic-free single-threaded flag,
   consistent with this project's single-threaded execution model — no barrier/atomic
   machinery needed, unlike a concurrent engine's version of the same mechanism).
3. Wire every mutation that could violate an invariant (prototype reassignment, builtin
   redefinition, indexed-property addition on a prototype object) to flip its cell,
   reusing whatever property-store instrumentation [[07]]/[[08]] already has at the
   point a `SetStatic`/`SetComputed` targets a prototype object.
4. Wire every native builtin kernel from [[212]] (and future [[91]] kernels) to check
   its relevant cell(s) — one load, one branch — before taking the fast path, falling
   back to the fully generic path on invalidation.

Acceptance: a program that never touches `Array.prototype`/relevant prototypes runs
every guarded builtin kernel with zero per-call invariant-verification cost beyond the
one cell check; a program that monkey-patches `Array.prototype.push` (or adds an indexed
accessor to `Array.prototype`) after using it correctly falls back to generic semantics
from that point forward, verified by a correctness test; the cell-check cost is O(1)
regardless of prototype chain depth, verified by comparing a deep-prototype-chain
benchmark against a shallow one and confirming no cost difference; alternating A/B on
[[212]]'s fast-path suites shows no regression from the added (single, O(1)) check.

Primary sources:
- V8 protector cells, referenced in the context of `Array.prototype.sort`'s fast-path
  invariant checking: <https://v8.dev/blog/array-sort>
- General fast-path-builtin invariant-checking discussion (V8):
  <https://github.com/thlorenz/v8-perf/blob/master/language-features.md>
