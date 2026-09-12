# 21 — Loop-invariant hoisting via associativity

Status: planned

`loop_(cond, body, labels)` builds `cond.labeled(top) + jump(exit) + body + jump(top) + identity.labeled(exit)`. When a prefix sub-morphism of `body` is a `Stencil<Ctx, Ctx>` that provably reads no loop-carried binding (no dependency on any slot mutated elsewhere in `body` or by `cond`), associativity — proven in [[01-stencil-category-core]] — permits rewriting `loop_(cond, invariant + rest, labels)` to `invariant + loop_(cond, rest, labels)`, moving `invariant` before the loop entirely. The rewrite's safety follows from the associativity law itself, not from a bespoke invariant-hoisting analysis; the remaining engineering work is purely the dependency check that identifies which prefix sub-morphisms qualify as invariant.

Keep the dependency check conservative and general (slot-write-set analysis over the existing `Environment`/`NameIc` machinery in [[13-environment-frames]]), not tied to any one loop shape from the benchmark suite.

Acceptance: a synthetic loop with a provably invariant prefix executes the invariant computation once instead of per iteration, verified by an execution counter, not just output equality; a loop where the "invariant-looking" prefix actually reads a loop-carried binding is correctly rejected and left inside the loop; existing loop/branch composition tests pass unchanged.
