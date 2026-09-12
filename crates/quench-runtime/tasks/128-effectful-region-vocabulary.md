# 128 — Effectful coarse-region stencil vocabulary

Status: in_progress

The accepted runtime still enters `dyn_block_step_impl` millions of times because pure
numeric traced loops cover almost none of the V8v7 object/call loops, while Tasks 113,
114, 117, and 127 show that another decoding helper or small guarded island cannot pay
for its connectors. Derive the next candidates from fresh runtime block frequencies on
the Task 126 binary, grouping by semantic opcode family without source names, property
names, literal values, or benchmark identity.

Design a coarse quoted region whose leaves include effectful kernels (property access,
calls, allocation) alongside direct numeric/local stencil segments. A region is still a
single `Connector -> Connector` morphism: effect leaves rejoin its internal continuation,
and only exceptional/guard-failure edges leave for the canonical slow adapter. This
raises the abstraction level from thousands of primitive fragments to one loop or
effectful basic-block instance while keeping immutable kernels and copy-patched stencil
instances categorically compatible.

Region construction must maximize the statically valid connected CFG under context,
effect, ownership, and code-size budgets. HHVM's tracelet experience is the relevant
negative result: small independently compiled units repeatedly shuffle state, repeat
guards, and block GVN, load elimination, reference-count elimination, and LICM. Reuse
that region-size lesson without HHVM's runtime PGO policy; Task 144's static context
versioning chooses every stencil region before execution.

First acceptance checkpoint: record a structured frequency artifact and select a
general bytecode grammar with broad dynamic coverage. Before implementation, specify
its state/effect contract, patch obligations, and a coverage estimate. Later acceptance
requires direct-entry counters, semantic tests, full smoke, and a stable alternating
full-suite improvement; reject any form selected by source identity or exact V8v7 code.

## Frequency checkpoint and decomposition

`reports/task128-effectful-region-frequency.jsonl` is a complete eight-suite, 20 ms
runtime semantic-block record from the exact Task 126 binary. It contains 20,657,992
generic block entries. General semantic classes account for:

- 4,502,865 static-property blocks with no call/construct;
- 2,419,507 static-property predicates ending in `JumpIfFalse`;
- 3,827,173 computed-property blocks with no call/construct; and
- 2,781,168 blocks containing a call or construct.

The first implementation grammar is therefore not an exact repeated shape. It is a
typed sequence over `LoadLocal`, guarded numeric `GetStatic`/`SetStatic`, numeric
arithmetic/comparison, local writes, and CFG transfers. Property names become link-time
slot obligations and never selection keys. Task 129 defines the stable guarded slot
view; Task 130 adds its region IR and AOT leaves. Task 131 separately introduces an
opaque effect connector so calls can compose without weakening the property region's
borrowed/immediate state contract.

Task 130 is now complete: static property leaves expanded Crypto from 15 to 29 linked
regions and from 1,704,849 to 1,796,041 native iterations in the short counter probe,
while its final full-suite A/B was score-neutral (+0.07%). This validates the quoted
effect vocabulary and shared-view connector but does not explain the remaining score
gap. Task 131 now provides the shared one-step effect kernel and reduces copied images by
2,740 bytes, but is performance-neutral (-0.01%). Task 132's selector-preserving residual
profile then measured 9,714,173 remaining generic entries: 4,319,715 static-property and
2,736,545 call/construct entries dominate. Task 133 removed all 121,459 standalone return
fallbacks by making terminal register return an ownership-transfer stencil and improved the
full aggregate 0.47%. The next coarse work should use escape proofs for local ownership and
whole property/call block templates; per-op effect chains remain explicitly rejected.
Task 137 supplied a stable AOT-visible object/shape/slot layout and accepted a
helper-free two-property strict-equality stencil at +0.85% aggregate. Task 138 then
proved that the broader local-versus-property numeric-condition family was selected
and halved its targeted residual shapes, but rejected it at -0.39% aggregate: repeating
shape and numeric guards inside small blocks does not pay. Further property work must
hoist validation across a larger basic block or loop rather than proliferating guarded
five-op fragments.
Task 139 tested call-level ownership transfer: it executed 2.9 million transfers in a
short Earley-Boyer run but confirmed at -1.16% aggregate and was reverted. Immediate
numeric values dominate those call edges, so adding metadata branches to avoid
heap-only retain/release work is the wrong granularity. Call optimization must remove
the Rust dispatcher/frame boundary or specialize a whole call path, not decorate each
argument copy.
Task 140 moved to the correct call-level obligation: a write-once monomorphic user-call
IC bypasses repeated function-kind, `RefCell`, and code-image ownership probes while
leaving argument semantics alone. It was accepted at +0.86% in the six-run confirmation;
an uninstrumented Earley sample halved top-of-stack dispatcher samples from 67 to 34.
Task 141 measured 3,532,614 dynamic calls and 4,578,319 directly lowerable suffix
opcodes after them, then rejected the smallest call/return split at -2.22% aggregate.
This rules out one-op boundary fragments. The remaining viable form is a single
effectful region instance that owns the call and a fixed multi-op continuation; boolean
result branches and return-producing suffixes are the largest coherent families to
evaluate first.
Task 160 identifies one sufficiently cheap property predicate: own-slot nullish or
strict null/undefined comparison with all intermediates dead. Its one shape guard and
raw tag comparison improve Richards 10.00% and the full aggregate 1.84%. This does not
reverse Task 138's conclusion for numeric/coercing property fragments; larger typed-
shape regions remain necessary for arithmetic and repeated property work.

Primary region-size source: <https://hhvm.com/blog/2017/02/17/region-jit.html>.
