# 188 — Borrowed arguments view and direct apply forwarding

Status: planned

Represent ordinary nonescaping `arguments` as a borrowed view over the current VM call
frame: actual count plus directly indexable argument slots. `length` and indexed reads
become fixed-offset stencils. Materialize a heap arguments object only on escape or an
operation requiring object identity/property mutation; mapped sloppy arguments must
retain parameter aliasing exactly.

Recognize stable `Function.prototype.call`/`apply` identities through Task 183 recipes.
Forward a frame view, dense array slice, or arguments view directly into Task 146's call
ABI instead of `ArrayStorage::to_vec` plus another argument vector. Array-like/proxy and
mutated-builtin cases take the canonical semantic kernel.

Acceptance: escaping/non-escaping and mapped/unmapped arguments semantics pass; apply to
dense arrays and arguments views performs zero argument-vector allocations; altered
`Function.prototype.apply` and generic array-like objects remain correct; Earley-Boyer
allocation/call-boundary counts fall and full V8v7 A/B improves.

Primary sources: <https://v8.dev/blog/adaptor-frame>,
<https://webkit.org/blog/10308/speculation-in-javascriptcore/>, and V8's higher-order
builtin optimization notes <https://v8.dev/blog/v8-release-80>.

