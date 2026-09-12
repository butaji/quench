# 368 — Prototype and property watchpoint stencils

Status: in_progress

Task 374 tested a correct raw prototype-chain projection in the general native `GetStatic`
leaf and rejected it at -0.05% aggregate because important inherited accesses remain inside
blocks containing unsupported name/call effects. Do not retry the isolated leaf. First
close the enclosing effect boundary as one coarser stencil/kernel composition; only then
consume the already-validated chain projection.

Make stable inherited property and method IC arms a direct receiver-shape guard plus fixed
load/call, without walking the prototype chain on every hit. Add a monotone realm/prototype
epoch or per-shape watchpoint dependency to the canonical shape fact. Property add, replace,
delete, descriptor change, prototype replacement and relevant global mutation invalidate
dependent IC arms/function images before the next successful use.

The immutable fact is `WatchDependency { receiver_shape, holder_shape, key_id, slot,
epoch }`. A `StencilTemplate` patches it into a copied arm when burning the dependency wins;
a shared `Kernel` consumes the same record by reference when code sharing wins. Several
receiver shapes with the same holder/offset may fold into one bounded minimorphic arm.
Miss/invalidation publication is the edge-confined mutation; the fast morphism remains a
pure checked transition over the connector state.

First experiment: inherited monomorphic method loads already counted by Tasks 185/306.
Disassembly on success must contain receiver/dependency checks, one fixed load, and direct
continuation only—no prototype walk, string lookup, Rust ABI call or `RefCell`. Counters
separate hit, invalidated, shape miss, epoch miss, polymorphic saturation and generic cases.
Mutation/descriptor/proxy tests, complete correctness and alternating V8v7 A/B are required.

Primary sources: JSC's structure/watchpoint and IC design
<https://webkit.org/blog/10308/speculation-in-javascriptcore/>, SpiderMonkey CacheIR stub
folding <https://firefox-source-docs.mozilla.org/js/cacheir.html>, and V8 fast properties
<https://v8.dev/blog/fast-properties>.

## Property-condition algebra

Generalize the dependency from positive inherited loads to the closed sum used by JSC's
Object Property Conditions:

`PropertyCondition = Presence(holder, slot) | Absence(chain_end) | Equivalence(value)`.

`Presence` licenses a fixed-slot load, `Absence` licenses a proven-missing result or a
prototype-chain skip, and `Equivalence` licenses a constant or direct method target while
the property continues to denote the same value. Each variant carries the receiver shape,
key, relevant chain identities/shapes, and invalidation epoch needed to validate precisely
that promise. These are data constructors consumed by one guard-selection rewrite, not
three ad-hoc IC implementations.

Make watchpoints adaptive: mutation may relocate or widen a still-valid condition by
publishing a replacement immutable arm; an invalid condition falls through to the generic
property stencil. Never mutate a shared kernel or `StencilInstance`. Negative-lookup tests
must cover property creation anywhere on the guarded prototype chain, deletion, descriptor
changes, `__proto__` replacement, proxies, and same-value replacement. Count avoided chain
walks separately for all three constructors.

Primary source for Presence, Absence, Equivalence and adaptive watchpoints:
<https://webkit.org/blog/6756/es6-feature-complete/>.

## Task 401 reach evidence and bounded first slice

Task 401 now supplies the coarser surrounding call morphism that Task 374 lacked. Its
post-gate census enters the fused method-call templates but records zero direct-call attempts
in Richards because the method is inherited and `read_cached_own_property` fails first.
Promote the smallest general `Presence` slice: publish a POD one-level inherited record
`{ receiver_shape, holder_identity, holder_shape, slot }` beside the own-property IC. The
cooked property/call recipe validates the receiver and immediate prototype, loads the fixed
holder slot, and composes directly into the existing call. Longer chains and every mutation
fall back to canonical semantics. This is not a retry of Task 374's isolated leaf: the
consumer is now the complete `property ; call ; continuation` region whose prior absence was
the recorded rejection reason.

## Implemented one-level `Presence` slice

`PropertyIcSite` now owns one canonical runtime fact with an own prefix and a POD inherited
projection. A successful semantic lookup publishes
`PublishedInheritedPropertyIc { receiver_shape, holder_identity, holder_shape, slot }` only
for an exact one-edge prototype chain. Longer chains publish the empty identity value. Own
fills, writes, misses, and invalid guarded identities retire the inherited projection.

The rustc/LLVM AOT source consumes that same layout through `read_cached_property`. Every
cooked static-property reader first tries the existing own shape/slot arm, then checks the
receiver shape, immediate prototype identity, holder shape, and fixed holder slot. Success
therefore performs raw layout loads and composes into the existing continuation or call
stencil without a prototype walk, string lookup, `RefCell`, or Rust semantic helper. Setters
remain deliberately own-only. The representation is general bytecode-site data: it contains
no source name, benchmark name, property key, PC selector, or hotness threshold.

GC invalidation is derived from the traced heap fact instead of clearing every inherited IC.
Immediately before sweep, an inherited projection survives only when every guarded object
cell is marked; otherwise it is erased before any cell can be reclaimed and reused. Tests
cover own and inherited machine-code hits, receiver/holder shape misses, shadowing, longer
chains, live/dead GC identities, and an inherited method call that reaches the native call
continuation. Both the normal and collect-every-frame release suites pass all 157 tests.

The live Richards census at
`reports/task368-inherited-property/native-census.log` records **3,725,363** direct-call
attempts and **3,502,225** hits (**94.0%**), replacing Task 401's prior zero-attempt result.
The O2/O3 differential cooker audit passes in `reports/task368-stencil-cooker-audit/` over
167 catalog symbols, 51 placeholder-sensitive stencils, and 110 typed holes.

The three-pair screen at `reports/task368-inherited-property/quick-ab/comparison.txt`
improves aggregate **2434.97 -> 2469.77 (+1.43%)**, including Richards **+8.22%**. The
nine-pair upstream-exact checkpoint at
`reports/task368-inherited-property/exact-vs-accepted/checkpoint-comparison.md` passes:
aggregate **2352.14 -> 2401.58 (+2.10%)**, paired-bootstrap interval
**[+1.04%, +3.14%]**, Richards **+11.61%**, DeltaBlue **+8.28%**, and every suite above the
-5% floor. The accepted binary is `/tmp/deegen-task368-inherited-property-candidate`,
SHA-256 `06207d687c4ee53edde788a91950c9d49423322627c98f9ba9681ee73cb397ae`.

Task 368 remains in progress for longer-chain/minimorphic conditions, `Absence` and
`Equivalence`, and mutation-driven adaptive watchpoint replacement. The bounded one-level
`Presence` slice is accepted.
