# 248 — Closed-coproduct method dispatch for richards.js's fixed task-type set

Status: planned

richards.js's scheduler dispatches every simulated task through one call site,
`this.task.run(packet)` (`richards.js:337`), where `this.task` is always one of exactly
four concrete types — `IdleTask`, `DeviceTask`, `WorkerTask`, `HandlerTask`
(`richards.js:368,396,424,459`) — each with its own `.run` method, never subclassed or
extended elsewhere in the source. This is the call-dispatch analog of [[220-closed-coproduct-tag-dispatch-earley-boyer]]'s
`instanceof`-chain finding: a small, closed, statically-enumerable set of concrete
implementations behind one dispatch point, except here the dispatch mechanism is method
lookup (an implicit "which `.run` do I call" decision resolved through the receiver's
shape/prototype) rather than an explicit chain of `instanceof` tests. [[25]]'s
bounded-polymorphic guards and [[140-monomorphic-user-call-ic]] already handle *this*
call site generically (cache up to 2-4 observed receiver shapes), but richards' case is
stronger than "observed to be small so far" — the task-type set is closed by
construction (no other `Task`-shaped constructor exists anywhere in the source), the
same closed-world distinction [[235-singleton-call-target-exactness]] already draws
between runtime-observed monomorphism and provably-closed enumeration.

Compile this call site as a direct coproduct case-dispatch (a tag-load-and-jump-table,
per [[220]]'s coproduct universal-property argument, here applied to a 4-way rather than
richer tag set) instead of a shape-guarded polymorphic inline cache: since the
callee set is exactly 4 and closed, the dispatch is a small jump table keyed on the
task's own type tag (already established at construction, reusable from whichever tag
[[07]]/[[135]]'s shape machinery already assigns), not 2-4 sequentially-checked guard
arms falling back past each other.

This is a distinct, additional site for [[231]]'s exactness family beyond the three it
already names, and additional evidence — alongside [[235]]'s existing scope — that
closed-world call-target proofs matter for *method* dispatch specifically, not only for
direct function-value call sites.

Also note for [[221-list-catamorphism-traversal-loops]]: richards' packet queue
(`Packet.prototype.addTo`, `TaskControlBlock.prototype.run`'s `packet.link` chase) is a
second, structurally distinct real-corpus instance of [[221]]'s general cdr-chasing
catamorphism shape (a `.link`-linked queue rather than `sc_Pair`'s `.cdr`), confirming
[[221]]'s recognition rule is general rather than earley-boyer-specific — [[221]]'s
acceptance criteria should be broadened to include a richards-derived test case, not
only earley-boyer's twelve identified sites.

Concrete steps:
1. Confirm the four-member closure of `Task`-shaped types against the full source (no
   other `.run`-bearing constructor exists), the same closure-proof discipline
   [[220]]/[[231]] already require before treating a coproduct as closed.
2. Compile `this.task.run(packet)` as a 4-way tag dispatch, reusing whichever tag
   representation [[07]]/[[135]] already establish at each `Task` subtype's
   construction site, rather than the generic shape-guarded call IC.
3. Add richards' `packet.link` queue-drain loop as a second recognized instance under
   [[221]]'s general shape-matching rule, verifying the rule's generality rather than
   writing richards-specific matching logic.

Acceptance: `this.task.run(packet)` compiles to a direct 4-way jump table with no
sequential guard-chain fallback, verified by disassembly; richards' `packet.link`
queue-drain loop is recognized and fused under [[221]]'s existing (not
richards-specific) mechanism, demonstrating that task's generality; correctness tests
for all four task types (idle, device, worker, handler simulation paths) pass unchanged;
alternating A/B on richards specifically shows a measured gain, with the full V8v7 suite
at or above the standing regression floor.

Source: `/private/tmp/js-engine-benchmark/v8-v7/richards.js:241-260,324-345,368-537`
(local V8v7 corpus checkout).
