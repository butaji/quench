# 330 — Zero-code frame-state hints and lazy exit reconstruction

Status: planned

Represent every semantic assignment needed for recovery as an immutable `FrameHint`
in the quoted region. Optimizers treat the hint as an ordered abstract store, but normal
machine-code emission produces zero bytes. At a guard failure, exception, or general
stencil re-entry, reaching definitions plus bytecode liveness derive one
`Materialize<SpecializedGamma, CanonicalGamma>` recipe.

This is one representation shared by [[158-register-resident-cps-stencil-planner]] and
[[164-canonical-side-exit-frame-state]]. It eliminates eager frame synchronization while
preserving the exact state required by every exit. A hint must follow all parts of a
multi-result semantic operation before an exit is legal; an observable effect and its
corresponding hint cannot be separated by an exit.

Acceptance: normal paths emit no load/store for hints; exit-at-every-guard differential
tests reconstruct identical locals/registers/ownership; metadata growth is linear via
persistent parent/delta sharing; native profiles show reduced frame traffic; complete
V8v7 alternating A/B passes. Hint and materialization limits are named constants.

Primary source: JavaScriptCore's `MovHint` is modeled as an abstract store, emits no code,
and is reconstructed from reaching definitions and liveness at OSR exit:
<https://webkit.org/blog/10308/speculation-in-javascriptcore/>.

