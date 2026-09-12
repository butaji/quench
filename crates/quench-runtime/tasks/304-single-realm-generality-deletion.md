# 304 — Delete embedding-generality overhead outright instead of guarding around it

Status: planned

Strengthens [[252-closed-world-single-file-scope-validation]] with the "why V8/JSC
can't do this" half of the argument, using [[302]]'s legacy-tax classification as its
evidence source. V8 and JSC are both designed to be *embedded*: multiple realms/contexts
sharing one isolate (browser tabs, iframes, Node workers), embedder callback hooks at
many boundary points, and security isolation between contexts that must never leak
state. Every one of these is a real, load-bearing feature for their actual deployment —
but every one of them also imposes a per-operation tax (a context check, a realm-
identity comparison, an indirection through an embedder-overridable hook) that a
standalone, single-script, non-embedded VM has no structural reason to pay, because the
*feature itself* — not merely its runtime check — does not need to exist here.

This is a stronger claim than [[252]]'s validation (which asks "does the closed-world
machinery correctly handle the single-file case") and a stronger claim than [[229]]'s
protector cells (which make an embeddable feature's check cheap when unused) — this task
asks whether some of these features should be **deleted from the design entirely**,
not merely guarded around cheaply, because a from-scratch, non-embedded VM never needs
the capability the check exists to protect.

Concrete steps:
1. Using [[302]]'s classification, identify which specific hot-path checks in V8/JSC
   exist for multi-realm/embedder-hook/security-isolation reasons (not corrected by a
   protector cell or IC guard, because the check itself would need to exist even in the
   monomorphic/fast case — e.g. a receiver's realm identity must be checked even when
   its shape is monomorphic, because realm identity is orthogonal to shape).
2. For each, confirm this VM's actual execution model (`cargo run -- path/to/script.js`,
   one process, one script, no embedding API) structurally cannot need the corresponding
   feature — not merely "doesn't use it in the V8v7 corpus," but "the capability has no
   path to ever being invoked given how this VM is built and deployed."
3. Where confirmed, state explicitly that the corresponding check does not need a fast
   path, a protector cell, or any runtime representation at all — it should not exist in
   the compiled stencil in any form, which is a stronger and cheaper outcome than
   [[231]]'s exact-abstraction erasure (that erases a check whose *value* is proven
   constant; this deletes a check whose *entire justification* does not apply).

Acceptance: [[302]]'s classification yields at least one concrete multi-realm/embedder-
hook/isolation check in V8/JSC's hot paths; this VM's execution model is confirmed to
structurally never need the corresponding capability; the equivalent check (if this
project ever accidentally introduced one while modeling a V8/JSC mechanism) is
confirmed absent or is removed; this task's finding is recorded in [[252]] as
corroborating evidence for why the closed-world/single-file scope is not merely
convenient but a design advantage this project can lean into rather than merely
tolerate.

Source: [[302]]'s classification output and V8/JSC source read under [[254]]; this VM's
own execution model per `README.md`'s stated entry point and [[55]]'s architecture
scope.
