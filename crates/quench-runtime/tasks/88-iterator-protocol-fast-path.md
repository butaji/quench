# 88 — Iterator-protocol fast-path stencils for for-of and spread

Status: planned

`for-of`, spread (`...arr`), and destructuring-from-iterable all go through the generic
ECMAScript iterator protocol: call `[Symbol.iterator]()`, then repeatedly call `.next()`
and branch on `.done`/`.value`. For the overwhelmingly common case — iterating a dense
array or a string — this protocol is pure overhead around what is actually an index
increment and a bounds check, but a naive implementation pays a property lookup and a
call per element for machinery whose outcome is fixed once the iterated value's kind is
known.

Give `for-of` (and internally, spread/iterable destructuring) a guarded fast-path
stencil keyed on the element-kind guard from [[32-element-kind-guarded-arrays]) plus a
string-iteration variant: guard the iterated value once (dense array of a stable element
kind, or a string), then lower the loop body to a direct index-increment/bounds-check
native loop with no `Symbol.iterator`/`.next()` call machinery at all. A guard miss
(the value is a `Map`/`Set`/generator/custom iterable) falls back to the fully generic
protocol dispatch through [[27-callsite-devirtualization]]'s call machinery.

Acceptance: `for (const x of denseArray)` and `[...denseArray]` over a guarded dense
array compile to a stencil with zero `.next()`/`Symbol.iterator` calls, verified by a
call-count counter; a `for-of` over a `Map`/generator/custom iterable still produces
correct results via the generic fallback; alternating A/B on iteration-heavy suites
(e.g. loops over `Array` literals in the V8v7 corpus) shows a measured gain.
