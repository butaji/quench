# 108 — Batched call-binding initialization

Status: complete

Initialize a function's `this`, optional `arguments`, and parameter slots while holding
one mutable borrow of its environment frame. This is a semantics-preserving fusion of
adjacent writes to the same product component: binding order and `Environment::declare`
remain canonical, while repeated `RefCell` borrow checks are removed from every user
function call.

The change is general bytecode/runtime infrastructure and does not inspect source names,
benchmark identity, call counts, or hotness. Hoisted closure construction remains outside
the borrow because it can recursively interact with the VM and environment.

Acceptance: all unit tests and a complete V8v7 smoke pass. An exact alternating
six-repetition A/B against [[106-cleared-slot-pool-invariant]] must improve aggregate
score without crossing the standing -5% per-suite floor. Revert and record rejection
otherwise.

Result: accepted. All 44 tests and the complete V8v7 smoke passed. The exact
six-repetition alternating comparison in
`reports/batched-call-bindings-ab-6/comparison.txt` raised aggregate score from
832.735 to 835.988 (+0.39%). Richards (+0.82%), DeltaBlue (+2.07%), Crypto (+0.92%),
Earley-Boyer (+0.30%), and Navier–Stokes (+0.16%) improved; RayTrace (-0.41%), RegExp
(-0.22%), and Splay (-0.49%) remained well inside the component floor.

Accepted binary SHA-256:
`0111ec558cd0434ea50f37e53bb4b8db654163b50ee8ed57b3b4bccb0b55112d`.
