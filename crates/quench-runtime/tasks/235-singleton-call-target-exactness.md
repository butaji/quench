# 235 — Singleton call-target exactness under closed-world devirtualization

Status: planned

A fourth concrete site for [[231-exact-galois-insertion-guard-erasure]]'s criterion,
distinct from the three already named there, applied to calls rather than shapes/tags.
[[27-callsite-devirtualization]] (already `complete`) caches a call site's resolved
target under a monomorphic-receiver guard — checked cheaply, per call, the same
"O(1)-with-a-check" tier [[231]]'s introduction distinguishes from true erasure.
[[46-closed-world-mode]]'s whole-program colimit, when it proves a call site's *entire*
reachable callee set (not just its observed history) has exactly one member — a
genuinely closed, unique target with no possible alternative anywhere in the closed
program, not merely "the only one seen so far" — gives that call site the same
singleton-fiber exactness [[231]]'s site 3 describes for [[69]]'s colimit, specialized
to calls.

This is a meaningfully stronger and rarer condition than [[27]]'s existing guard: [[27]]
caches "this call site has always resolved to function X" (an inductive, runtime-history
fact, re-checked because a *different* value could in principle arrive next time);
this task targets "this call site's static call graph proves X is the *only* function
that could ever arrive here" (a closed-world structural fact, requiring no re-check
because there is no alternative to distinguish from). The two guards look similar at a
call site but rest on different evidence and license different erasure: [[27]]'s check
must remain (the underlying fact could, in an open-world sense, still vary); this task's
target check can be deleted (there is nothing else it could ever observe).

Concrete steps:
1. Identify, within [[46]]'s closed-world colimit construction, which call sites in the
   actual V8v7 corpus have a provably singleton callee set — not "monomorphic in
   practice" (which [[27]] already handles) but "monomorphic by construction of the
   closed program," e.g. a call to a function expression assigned once to a `const`
   binding never reassigned and never escaping to code that could be replaced.
2. Distinguish this proof from [[27]]'s runtime-observed monomorphism explicitly in the
   compiler's guard-classification so the two are never conflated (conflating them
   would let an open-world call site incorrectly skip its needed re-check — exactly the
   error [[224]]'s blame-calculus boundary discipline exists to catch).
3. Erase the call-target guard at proven-singleton sites entirely: the call compiles to
   a direct, unconditional call/tail-call to the one known target, with no receiver or
   identity check at all.

Acceptance: at least one call site in the V8v7 corpus is identified as
closed-world-singleton (not merely runtime-monomorphic) and compiles with zero
call-target guard instructions, verified by disassembly; [[27]]'s existing
runtime-monomorphic sites are confirmed unaffected (still checked, as they must remain);
a negative test — a call site that looks monomorphic in a short run but is *not*
closed-world-singleton (the closed-world proof does not actually hold for it) — is
confirmed to correctly retain [[27]]'s check rather than being incorrectly promoted to
erased; alternating A/B on the identified site(s) shows a measured gain.

Primary sources: builds on [[231]]'s and [[232]]'s existing citations; no new primary
source needed beyond [[46]]'s and [[69]]'s own colimit-uniqueness grounding.
