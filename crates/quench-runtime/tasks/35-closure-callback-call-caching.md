# 35 — Closure-callback call-target caching

Status: planned

Evidence from the V8v7 suite: splay's `SplayTree.prototype.traverse_(f)` invokes a captured closure `f` once per visited node in a loop; richards' scheduler repeatedly invokes task closures through the same call machinery. Unlike [[27-callsite-devirtualization]]'s receiver-identity guard (which caches "this call site always sees function X"), the relevant identity here is the captured closure value passed as an argument or held in a loop-local binding — the call site itself is fixed in source, but the callee is a runtime value threaded through a parameter.

Use a two-state call-IC key, following Deegen's direct-call/closure-call split. A site
starts in `Direct`, keyed by the exact function value. If an identity miss has the same
immutable function-code identity as the cached closure, replace the site recipe with
`Closure`, keyed by function-code identity and consuming the current closure's environment
pointer. This lets closures created by the same function literal share one native entry
without incorrectly reusing the first closure's captured environment. A different code
identity takes the bounded polymorphic or generic arm.

This is an evidence-driven IC state transition, not hot-path detection: the first exact
function installs `Direct`; a same-code/different-environment miss proves the closure
factory pattern and installs `Closure`. The state machine is data, and both arms lower
from the same quoted call recipe. The eventual hit must compose directly with Task 181's
in-place native call/return continuation; merely caching `Rc<DynJitCode>` while still
constructing a Rust sidecar is not acceptance.

Acceptance: Splay's `traverse_` loop resolves one function-code identity and reuses its
linked image while passing each current closure environment; separately created closures
from the same literal return their own captured values; a different function body never
hits the closure-mode arm; direct/closure transitions and hits are counted separately;
the hit executes through Task 181's native continuation and full V8v7 A/B improves.

Primary source: Deegen's baseline-JIT call IC starts with exact function identity and
switches to a function-prototype key after observing a same-prototype miss, specifically
to handle function factories: <https://sillycross.github.io/2023/05/12/2023-05-12/>.
