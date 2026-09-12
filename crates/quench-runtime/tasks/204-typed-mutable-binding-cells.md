# 204 — Typed mutable environment and global binding cells

Status: planned

Extend representation selection from object fields to lexical/global binding cells.
Each eligible cell has a monotone representation state such as
`Constant | I32 | F64 | Tagged`; widening preserves JavaScript semantics and invalidates
or redirects dependent stencil instances through a named fuse. Captured/global numeric
regions may then load, compute, and store raw I32/F64 values, boxing only at observable
generic edges.

The binding's semantic identity, ownership, mutability, and representation are one
quoted record used by environment lookup, SSA analysis, stencil selection, and side-exit
materialization. Do not maintain a second optimizer-only shadow type. Dynamic scope,
`eval`, deletion, accessors, or incompatible writes select the tagged generic morphism.

Acceptance: typed-cell transitions are law-tested; raw numeric regions avoid repeated tag
checks/conversions across mutable captured/global accesses; closure aliasing and writes
from generic calls invalidate correctly; Crypto/Navier-Stokes/Earley-Boyer counters show
lower binding and boxing traffic; full V8v7 alternating A/B improves.

Primary sources: V8 mutable heap-number slot states
<https://v8.dev/blog/mutable-heap-number>, JavaScriptCore value prediction and watchpoints
<https://webkit.org/blog/10308/speculation-in-javascriptcore/>, and V8 Maglev
representation selection <https://v8.dev/blog/maglev>.
