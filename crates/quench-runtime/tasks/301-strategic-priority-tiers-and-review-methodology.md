# 301 — Strategic priority tiers and decision methodology for the score gate

Status: in_progress

**What this task is, and is not.** [[00-optimization-routine]] governs how to validate
*one already-chosen* change (baseline, A/B, ledger). This task governs the layer above
that: given a ~300-item backlog with no natural stopping point, which item to pick up
next, and how to tell whether the backlog's own priority ordering needs to change as new
evidence lands. It is a living document — re-read and re-edit it whenever [[253]]'s
profiles are re-run or a tier boundary closes, not a one-time plan.

## The tiers, grounded in measured evidence, not intuition

**Tier 0 — close the dominant structural boundary before adding breadth.**
[[253]]'s actual measured native profile (`reports/task253-comparative/native-top.tsv`)
is unambiguous: `dyn_block_step_impl` (the generic fallback executor) is **26–29%** of
samples across richards/deltablue/raytrace/earley-boyer/splay, and frame lifecycle
(`make_frame`/`complete_dyn_frame`/`Value` drop glue) adds another **~25–30%** on top —
over half of measured execution still goes through generic machinery. Any corpus-specific
stencil built before this closes is optimizing the smaller half of the pie. Tier 0 is:
[[128]], [[131]], [[146]], [[148]], [[149]], [[158]], [[171]], [[172]], [[173]], [[191]],
[[193]] — the effect/region/SSA/ownership substrate. Do not treat pattern-coverage work
([[36]], [[37]], corpus-specific stencils) as equally urgent while Tier 0 items remain
`in_progress` or `planned` in this category.

**Tier 1 — answer whether the composition-boundary ceiling is real before trusting
further stencil proliferation as the default strategy.** [[251]]'s hypothesis
(cross-stencil register allocation may impose a structural ceiling) has direct
corroborating evidence from [[288]] (V8 inlines crypto's `am3` entirely away — 0.2% of
samples vs. its caller's 28.3%). If confirmed, the unlock is [[20]]/[[163]]/[[290]]
(inlining) and [[292]] (monoidal register-resource model), not more direct-opcode
coverage. Resolve [[251]] before allocating significant new effort to pattern-coverage
tasks whose main value proposition is "another stencil family."

**Tier 2 — apply the category-theory erasure family ([[207]], [[219]], [[223]],
[[231]]–[[242]]) to what Tier 0/1 produce.** This family is a genuine multiplier, not a
substitute: erasing a guard on a fast path that still bottoms out in 30% generic
dispatch does not move the score much. Sequence after Tier 0/1 substantively land, not
in parallel with equal claim on effort.

**Tier 3 — corpus-grounded per-suite wins ([[220]]–[[222]], [[225]], [[227]], [[229]],
[[230]], [[243]]–[[249]]) and systems-level items ([[179]], [[180]], [[208]], [[209]],
[[241]], [[257]], [[295]], [[296]]).** Real, individually verified, individually small
(single-digit percent). Good to parallelize across contributors once Tier 0/1 land —
they compose cleanly on a settled substrate, badly on a shifting one.

**Tier 4 — correctness/scale validation ([[250]], [[252]], [[297]]) and GC
([[38]]/[[148]]/[[162]]/[[198]]).** Do not move [[15]]'s score (the corpus doesn't
exercise cycles or shape-diversity at the scale that would matter) but are the
difference between "wins the benchmark" and "is actually a JS VM." Do not let them block
score work; do not let them rot indefinitely either — [[250]]'s deltablue cycle question
is a real correctness gap on a *scored* suite specifically, which raises its priority
within this tier above the others.

**Explicitly separate track, not this gate's job:** [[22]], [[45]], [[46]], [[47]],
[[48]], and the more ambitious end of the categorical thread pursue "structurally
outperform V8/JSC's tiered design" — a different, longer-horizon goal from hitting
10000, exactly as [[15]]'s own dependency-scope note already states. Do not let work on
that track substitute for Tier 0-3 progress when reporting status against [[15]].

## Decision methodology: what to pick up next

Before starting a `planned` task, check in this order:

1. **Does it address a Tier 0 boundary, or does it sit on top of one?** If it's built on
   a Tier 0 boundary that hasn't closed yet (a corpus-specific stencil composed through
   the still-generic executor), its measured gain will be smaller and less durable than
   its task text implies — not wrong to do, but should not be reported as equivalent
   priority to closing the boundary itself.
2. **Is it corpus-verified, or speculative?** Prefer a task whose evidence section cites
   an actual line number in the actual V8v7 corpus or an actual measured profile entry
   over one justified by "this is a well-known technique in other engines." Both are
   legitimate, but the former has already survived the "does this suite actually
   exercise this pattern" check this session's strongest findings (220-222, 227, 246-248,
   288) were built on.
3. **Does completing it change [[253]]'s profile in a way worth re-measuring?** If yes,
   re-run [[253]]'s comparative profiling after landing it and update this task's tier
   assignments if the picture has shifted — a Tier 0 item that closes can promote
   previously-secondary Tier 3 items if they now sit on a settled substrate.
4. **Is there an open validation task (Tier 4) blocking confidence in the result?** A
   change that interacts with cyclic data ([[250]]'s deltablue concern) or shape
   diversity ([[297]]'s registry concern) should not be accepted on suite-score evidence
   alone if it plausibly touches those paths.
5. **Does it satisfy [[398]]'s bottom-up stage gate?** Check [[388]]'s headroom curve
   for the `deegen-curriculum` complexity stage the candidate primarily targets. If a
   lower stage the candidate's mechanism depends on is still below its named
   near-optimal threshold, the candidate is out of order — either the lower stage's
   work is the actual next priority, or the candidate must explicitly argue (per
   [[398]]'s exception clause) that its target is independent of that stage's slack.
   This check is mandatory, not advisory, per [[00]]'s preflight requirement — it is not
   satisfied by checks 1-4 alone, since Tier 0-4 groups by *boundary type* while [[398]]
   gates by *complexity order*, and a candidate can pass one axis while failing the other.

## Standing review cadence

Re-read this task whenever: (a) [[253]] is re-run and produces a materially different
profile, (b) a Tier 0 or Tier 1 item reaches `complete`, or (c) three consecutive
accepted tasks come from the same tier without the expected score movement — the last
case is itself a signal this tiering's assumptions need re-checking, not merely that the
suite is hard.

Acceptance: this task's tier assignments are re-validated (confirmed or revised) at each
of the three review triggers above, with the specific evidence for any revision cited
the same way this task's own initial tiering cites [[253]]/[[288]]; [[00]]'s per-change
routine and this task's backlog-selection routine are cross-referenced from each other
so a future contributor reading either finds the other.

## 2026-09-10 review trigger: two fine-grained experiments rejected

Tasks 299 and 306 are consecutive completed negative experiments from the Tier 0 substrate.
Bump-first allocation was exactly neutral (-0.01%) with unchanged Splay RSS. Extending the
already-selected property blocks with an immediate-prototype cooked arm was -0.26% while
adding 200 catalog bytes. Both were correctly wired, measured, recorded, and removed.

The tier ordering is confirmed but sharpened: "Tier 0" does not mean every low-level
micro-optimization has equal priority. Current 500 ms measurements pair inherited-property
hits with direct-call hits at 8.56m/7.75m in Richards, 12.62m/10.19m in DeltaBlue, and
1.97m/1.82m in RayTrace. Native profiles attribute roughly 17–32% of these suites to the
call/frame machinery. The next gate is therefore specifically Tasks 146/163/181's coarse
method-call and guest-frame continuum. Task 300's lazy sweep and other allocator refinements
remain conditional on collector-specific profiles; Task 145's standalone property slabs
remain below the boundary-removal work.

Review again after the first call continuum candidate, even if it is rejected: that result
will decide whether Tier 1 should pivot immediately to cross-stencil region formation and
register allocation rather than continue the call-stack migration.

## 2026-09-10 direct-call diagnostic and research round twenty-one

The current direct-call-region feature is rejected as evidence for the architecture, not
as evidence against direct calls: it regresses the four measured call-heavy suites by
4.68% despite high cache-hit rates because it retains every Rust frame/helper boundary.
Task 307 establishes the offset-zero AOT frame prefix without a regression; Task 146 now
owns the POD recipe/activation split and direct return continuum.

Primary-source round twenty-one adds one missing compiler representation, Task 309's
semantic micro-op algebra. The priority order is now:

`146/181 POD guest call -> 309 micro-op quote -> 171/144/152/158 reductions ->`
`163/290 bounded inlining -> 172/148 ownership and heap completion`.

This is a boundary-first order. Layout-only, isolated property, allocator, or tiny-opcode
experiments stay conditional until the call path no longer crosses Rust or the micro-op
reducer can remove a cost across several semantic operations.

## 2026-09-10 research round twenty-two refinement

Task 310 completed the shared POD header/result ownership boundary and measured neutral,
so the call priority is unchanged: the next call candidate must alter the executed edge.
Task 311 adds one newly concrete stencil defect to the main line: many ordinary operands
are still read from `InlineSite`, so Task 316 must burn typed register/local/literal/IC
operands into the instance or relocation-closed data. It also makes Task 157's cover
algorithm urgent: exact whole-block matching lets one unsupported micro-op demote a
coverable block to the generic Rust range.

Revised main line:

`146/181 native guest edge -> 309 semantic quote -> 316 operand holes + 157 total cover ->`
`171/144/152/158 reductions -> 163/290 inlining -> 172/148 ownership/heap`.

Tasks 312–315 are valid, general algorithms but remain secondary: the single-child
transition table is the only bounded representation A/B worth attempting opportunistically;
Sethi-Ullman ordering, pure-diamond if-conversion, and path-lazy sinking need the quoted
SSA/effect and connector-resource substrate first.

## 2026-09-11 static-block experiment and research round twenty-five

Task 326's first two-version block slice is useful but not independently sufficient. It
improves Crypto 3.42% and Navier-Stokes 6.43%, yet moves the full aggregate only +0.14%
and regresses Richards 3.54%. Runtime diagnostics explain the mixed result: Navier-Stokes
almost always satisfies its region guards, while Earley-Boyer almost always fails them and
Crypto fails roughly 39% of entries. Keep Task 326 in progress, not accepted, and do not
broaden static block selection yet.

Round twenty-five therefore inserts two bounded steps before another broad region A/B:
[[329-block-local-value-numbering-and-effect-forwarding]] removes repeated semantic work
inside the quote, then [[328-aarch64-conditional-compare-guard-products]] makes accepted
multi-input guards one target-native branch product. [[330-zero-code-frame-state-hints]]
remains the path from region register residence to cheap recovery. [[331]] adds the missing
critical-path measurement discipline after repeated cases where fewer instructions or loads
did not improve wall time.

The main structural order remains unchanged:

`181 native call edge -> 309/329 reduced semantic quote -> 144/328 typed versions/guards ->`
`158/330 register residence and lazy materialization -> 163/290 inlining -> 172/148 heap`.

The new items refine Tier 0/1 rather than creating a new tier. Research evidence and exact
measurements are recorded in [[327-primary-source-algorithm-research-round-twenty-five]].

## Research round twenty-seven: erase whole boundaries, then optimize the exposed region

Task 332's valid full-suite result is positive but small: 16 eliminated local-load sites
raise the aggregate only 0.49%, to 2133.61. This triggers the standing review rule after
another fine-grained change. The Tier 0 diagnosis is reconfirmed and narrowed further.

The next implementation order is:

`317 canonical call recipe -> 20 SCC-bottom-up leaf inlining -> 144 interprocedural context`
`-> 158 cold-use-aware register residence -> 176 edge materialization`.

Task 181 remains the required general direct-call continuum for calls that cannot be
inlined. The first score experiment, however, should be Task 20's exact-target,
noncapturing, exact-arity leaf slice: it can remove `make_frame` and `complete_dyn_frame`
at once and expose one larger graph to the already-built block reducer. Merely splitting
generic blocks around a Rust call was already rejected by Task 181 and must not be tried
again.

The algorithm is deliberately hierarchical: function graphs compose from block graphs,
which compose from stencil templates and shared kernels. SCC order and named budgets keep
the rewrite finite; a single post-inline macroexpansion and emission pass preserves the
Lisp quote/rewrite/eval boundary. Evidence and sources are recorded in
[[335-primary-source-algorithm-research-round-twenty-seven]].

## Task 336 result: reuse confirms the boundary diagnosis

Retaining one cleared non-capturing activation per monomorphic call site improves the
exact complete-suite aggregate 3.84%, including +9.69% Richards and +17.26% DeltaBlue.
A Richards run reuses activations 5,291,282 times after only 68 allocations. This is the
first recent result large enough to validate the boundary-first priority directly.

Do not generalize this into a family of mutable frame caches. The cache is a bounded
bridge to the intended representation: immutable `FunctionCallRecipe` data followed by
either Task 20's SCC-bottom-up quote-level inlining or Tasks 146/181's native guest-stack
continuation. Both erase the Rust recursive entry/completion edge; the cache merely avoids
reconstructing its payload. The next score experiment remains exact-target,
non-capturing, exact-arity leaf inlining derived from the same recipe.

Task 337 closes the tempting intermediate alternative. An AOT inline call-target guard
removed all Rust helper entries on IC misses and a guard-certified helper removed duplicate
checks, yet the complete aggregate regressed 0.59%. Therefore the next call experiment
must not be another guarded Rust helper. Proceed directly to Task 20's quote-level leaf
inlining; Tasks 146/181's guest-stack native continuum remains the fallback for edges that
cannot be inlined.

Task 338 is a second control on the methodology. `reset_value_slots` became a large sampled
leaf after Task 336, yet replacing its contiguous walk with a statically sparse plan
regressed the complete aggregate 1.43%. Profiles select hypotheses; only alternating
wall-time gates select implementations. This reinforces the whole-boundary priority over
another frame-cleanup refinement.

## Round twenty-eight: current-edge floor and measured inline reach

Tasks 340 and 341 remove a redundant environment retain/release and memoize the immutable
function `has_loop` projection. The decisive compound comparison improves 2183.02 ->
2234.06 (+2.34%), so both remain in the baseline. Task 342's source-context microstate is
neutral at +0.11% and is reverted. This establishes the floor: remaining individual
sidecar operations are not a credible route to 10000.

Task 343's feature-gated census now quantifies Task 20's initial reach without burdening
the default binary. Exact straight-line leaves cover 37.88% of DeltaBlue calls, 15.81% of
Richards, 14.02% of RayTrace, 10.54% of Splay, and 95.33% of RegExp. Implement that rewrite
next, then widen SCC-bottom-up to nested calls and control flow; those two rejection classes
dominate Crypto, Richards, DeltaBlue, and Splay. Task 146 remains the required continuation
path for the non-inlined complement. Full sources and the corrected current-state audit are
in [[339-primary-source-performance-gap-audit]].

Task 344 removes one duplicate call-IC entry lookup but regresses 0.28% and is reverted.
Do not continue current-edge pointer-chase experiments. The next implementation must
change the executed abstraction level: Task 20's inlined function graph or Task 146's
native guest-stack continuation.

## Research round twenty-nine: one canonical coarse-to-fine queue

Task 345 reconciles the newest primary-source research with the existing ledger. It does
not create duplicate object, array, call, or stencil systems. The current implementation
order is now explicit:

`20 leaf graph inlining -> 146/181 native non-inlined calls -> 316 typed operand holes ->`
`157/272 total costed cover -> 144/158/326 typed register-resident regions ->`
`145 inline IC slabs -> 189/276/322 locality and layout -> 265 packed numeric arrays`.

This is a hierarchy of the same composable value: functions contain loop/block plans,
which contain superinstructions and primitive stencils or shared kernels. Stencil
composition remains necessary infrastructure; a candidate only claims an execution win
when it erases helper edges, dynamic operand loads, boxing, frame traffic, or branches.
Task 20 remains first because Task 343 measured substantial exact-leaf reach and Tasks
336/340/341 proved the current call boundary is expensive. V8v7 remains a holdout gate,
not a template-generation corpus.

The same audit adds three missing but non-blocking infrastructure items: [[348]] makes
final linking a two-pass size/layout then copy/patch operation; [[346]] exports typed
loop-header native entries without a hotness policy; [[347]] broadens the cooker matrix
beyond Task 273's optimization levels. They join the queue at their dependency points
and do not displace Task 20 as the next score experiment.

## Task 349 and research round thirty: resolve reach before emitting code

Task 349 implemented and semantically validated the first same-owner-hoisted leaf rewrite,
but complete-suite statistics reported zero applied inline calls. Its short A/B result
(-0.44%) is therefore timing noise, and the implementation was removed. This corrects the
interpretation of Task 343: that task measured dynamic call-IC candidates, not targets the
current static compiler can resolve before linking.

Keep Task 20 first, but insert a hard reach gate before its next code-generation attempt:
split Task 317's canonical call recipe into pre-link binding/environment identity and linked
entry data, resolve global/sibling/prototype-installed functions, and run a static applied-
site census. Do not build another guard or cloned body until the census is nonzero on the
holdout suite.

Round thirty otherwise confirms the structural queue. Compiler-worklist BBV, single-pass
SSA known facts, CacheIR-style recipe expansion, register residence, and costed kernel-
versus-instance selection all have canonical homes in Tasks 144/152, 171, 153, 158, and
157. The primary-source crosswalk is [[350-primary-source-algorithm-research-round-thirty]].

## Task 351: the reach gate passes narrowly

The pre-link direct-binding census now intersects static identity, closure-environment
compatibility, and the canonical Task 343 classifier. It finds 283,051 safe initial-subset
executions in Earley-Boyer, so a bounded direct-binding leaf experiment is justified.
There were zero identity changes in every suite, supporting an exact-value guard as the
first arm while retaining the ordinary call morphism as fallback.

The same census prevents over-prioritizing that slice: Crypto's 321,011 and Splay's
87,121 exact compatible executions all require nested-call/control-flow composition,
while Richards and RayTrace expose zero executed direct-binding user targets. The ordered
call queue is therefore:

`20 direct-binding leaf proof -> property-call target recipes -> hierarchical nested-call`
`composition -> 146/181 native continuation for the non-inline complement`.

The leaf proof must be evaluated primarily on Earley-Boyer and rejected if its exact
applied counter is zero or its longer A/B is neutral. It is an evidence-bounded step, not
the claimed route from 2.2K to 10K by itself.

## Tasks 352/353 and research round thirty-one: IC arms must be machine-state code

Two independently wired leaf experiments are neutral. Task 352 expanded 16 exact sites
and Task 353 eliminated 83,394 activations in a 20 ms Earley run; their long aggregate
results were +0.12% and -0.02%. This closes direct-binding leaf work as the next priority.
The issue is both reach and abstraction: neither candidate removed a multi-million
property/call continuum, and Task 353 still entered a Rust callback.

Primary sources now make the required boundary precise. Deegen calls polymorphic ICs the
baseline JIT's most important optimization and says its stubs are not functions: they
operate on the main JIT's exact machine state and branch directly to continuations. JSC
likewise puts property/call ICs in baseline machine code, while SpiderMonkey's CacheIR
provides the small `Guard* ; Pure* ; Result` recipe algebra and shared-code/per-site-field
split. Static BBV and typed shapes are the next multiplier only after this hit path exists;
their published gains come from propagating one proof across blocks, not from calling a
generic helper behind each operation.

Revised order:

`153 minimal IC recipe -> 145 + 146/181 fused machine-state property-call slab ->`
`316 burned operands -> 144/152/158 typed shape/value block versions ->`
`157 costed multi-granularity cover -> 276/322 layout`.

Task 145's preflight reach floor and no-host-call disassembly check apply before coding.

## Tasks 373–376: composition works; choose capability sets, not isolated opcodes

Task 373 is the first broad proof that the primitive category is executable at useful
scale: maximal all-or-nothing block composition increased native cover from 903/2,299 to
1,755/5,533 blocks/opcodes and passed the exact gate at +2.42%. Task 375 then added one
general equality family and passed at +0.64%. Task 374's correctly wired inherited-property
arm was neutral because unsupported name/call operations kept the important enclosing
blocks generic.

The priority model is therefore refined, not reversed. Individual capabilities matter
when they close complete weighted block frontiers; otherwise they are latent ingredients.
Task 377 mechanizes this as a residual-frontier hypergraph and must precede speculative
catalog growth. Its canonical report measures call alone at 4,060,419 closable entries,
call plus name at 5,502,818, name alone at 438,003, and computed alone at 460,479. Calls
therefore own the largest structural frontier. Prior helper-calling call trials establish
that reach is not physical cost: Task 379/145/181 may be measured only after their caller
prefix and return suffix stay native. Task 378's lexical-address family and Task 12's
direct computed family are smaller, immediately buildable surrounding-cover pieces. Task
309/316/348/366 remains the general solution to the all-or-nothing cliff.

Current queue:

`377 measured-cost frontier planner -> 378 lexical address + 12 dense computed ->`
`309/316/348/366 total side-edge cover + 379/145/181 native call continuum ->`
`144/152/171/203 state propagation and guard erasure`.

This queue uses profiles to choose engineering work, never to select runtime code. The
always-on stencil-only policy and exact randomized A/B gate remain unchanged.
The existing activation reuse remains as fallback infrastructure; no more leaf-specific
call expansion or Rust callback wrapper is justified. Research details are in [[354]].

## Task 355 and research round thirty-two: the host ABI is the boundary

Task 355 gives the decisive control experiment. A roughly 900-byte call instance regressed
the aggregate 6.22%, so large per-site copying is rejected. Moving that algorithm into one
shared kernel restored the copied connector to 112 bytes and recorded 1,787,922 native hits
in 100 ms of Richards, yet still regressed the aggregate 5.04%. The losses concentrate in
the call-dense suites: Earley-Boyer -11.79%, RayTrace -8.79%, Splay -6.69%, and DeltaBlue
-5.79%.

Therefore neither “more activation reuse” nor “share the same Rust helper” is a Tier 0
candidate. The next call implementation is exclusively Task 146/181's in-place guest
stack and patched CPS return continuum. Only after it exists can Task 145/153 fuse a
property/call IC arm without a host ABI seam. The next queue is:

`146/181 guest call continuum -> 145/153 machine-state IC slab ->`
`171/144/152/158 typed SSA regions -> 316/348 burned operands/linking ->`
`157/189/291 physical realization and locality -> 162/193/320 nursery`.

V8's argument-adaptor removal supplies the variable-frame layout, Deegen supplies the
in-place call and explicit continuation model, and V8 CSA supplies the custom-register-ABI
requirement. The complete source synthesis is [[356]]. Do not attempt another call helper,
sidecar cache, leaf wrapper, or per-call-site copy before the direct continuum lands.

## Tasks 357–359 and research round thirty-three: effects, not larger syntax

Task 357 adds the explicit guest return continuation and post-prologue entry to every
function image without changing the score. Task 358 then repairs the stale traced-object
ownership classification and is accepted at +0.30%. Task 359 supplies the decisive next
failure: a correctly selected two-property terminal stencil adds three direct blocks, but
all 1,280,750 target executions still enter the generic block helper because reference-
counted sources or displaced values make the store effect unsupported. Its -0.31% full
aggregate candidate is removed.

The queue is refined, not replaced:

`172 StoreTake/DestroyValue effect proof -> 146/181 native guest calls ->`
`145/153 machine-state IC arms -> 36/157/316/348 total burned-operand cover ->`
`171/144/152/158 typed register-resident regions -> 148/193/320/321 unified heap`.

The first ownership slice is deliberately bounded and general: prove a last-use source,
transfer it into a property slot, and schedule destruction of the displaced value as an
explicit effect. It earns one short experiment because Task 359 establishes exact dynamic
reach. The permanent fix is Task 148's remaining heap-domain migration, not an expanding
set of tag predicates. Research and primary sources are recorded in [[360]].

## Task 361: shape and ownership are independent obligations

Task 361 corrects Task 359's diagnosis with executable evidence. Reference ownership was
one blocker, but the high-frequency stores were also adding absent fields to fresh
constructor receivers, so no own-slot cache existed. A statically derived, hash-consed
constructor shape makes those slots fixed; an ownership-permuting StoreTake stencil then
swaps source owners with displaced slot owners without a helper or reference-count update.
The exact residual falls from 784,948 entries to 2, Earley-Boyer improves 16.95%, and the
complete aggregate improves 0.72%, so the implementation is retained.

This validates the product-context model from Task 360: a tile fires only after both the
`Shape` and `Ownership` components are discharged. The next queue is now:

`146/181 native guest calls -> 145/153 machine-state IC arms ->`
`172 general per-edge ownership SSA -> 36/157/316/348 total burned-operand cover ->`
`171/144/152/158 typed register-resident regions -> 148/193/320/321 unified heap`.

## Research round thirty-four: total native cover before deeper specialization

Task 363 reconciles another primary-source pass with the Task 358 residual census. The
result is convergence rather than a new architecture. Depending on suite, 61.98% to
91.26% of counted residual-plus-native block entries still enter residual block kernels;
one unsupported effect can demote an otherwise native-capable block to
`dyn_block_step_impl`. These are entry counts rather than cycle attribution, but they
prove that isolated exact stencils cannot close the current gap.

Keep Task 362 as the active experiment. After its hostless call result, the queue is:

`309 semantic micro-ops -> 157 total costed cover + 316 burned operands ->`
`171/144/152/164/158 bounded typed regions and terminal stencil side exits ->`
`145/153 machine-state IC arms -> 172/329/191 effect and allocation elimination ->`
`165/214/265 typed fields and arrays -> 148/193/320/321 unified heap ->`
`189/291/322/347 physical locality and cooker quality`.

The categorical/Lisp discipline is now concrete: one immutable `RegionPlan<MicroOp>` is
the fact, abstract reduction and tiling are pure transformations, and one final linker
chooses shared `Kernel` versus patched `StencilInstance`. Stencil composability is
necessary infrastructure; C-like speed appears only when a coarse region carries proofs
and unboxed machine locations across many leaves. Full sources and acceptance rules are
recorded in [[363]].

## Task 362 result: call leaves cannot precede total cover

Task 362 implemented and executed the hostless call/return edge, then rejected it. The
narrow proven subset produced zero native hits in Richards, RayTrace, RegExp, and Splay,
and only 1,210 hits in a 495,967-attempt DeltaBlue run. More than 90% of warmed attempts in
the call-heavy suites targeted callees outside the straight-line ownership-safe semantic
subset. A control-flow widening found hits but failed RayTrace correctness, so it is not a
valid shortcut.

The smaller 20-byte eligibility probe confirmed the remaining structural cost: splitting
one unsupported call from a generic block multiplies native/Rust/native boundaries and
loses the batching that currently makes the coarse kernel tolerable. The enabled complete
smoke scored 2184.65; even leaving the dormant fields in the default representation lost
0.96% in a three-pair 200 ms median, so the experiment was removed completely.

This triggers the review rule and changes the immediate order. Do not attempt another
isolated call leaf, continuation field, or per-site call slab. The next queue is:

`309 semantic micro-op quote -> 157 total multigranularity cover + 316 burned operands ->`
`171/144/152/164/158 typed regions -> 146/181 whole-region guest continuation ->`
`145/153 machine-state IC arms -> 172/329/191 effect erasure`.

“Stencil-only” means the selected cover consists entirely of compatible morphisms at all
granularities; it does not mean selecting the smallest opcode morphism when a block or loop
kernel has lower transition cost. The cost model must charge connector/host boundaries
highly enough that it cannot reproduce Task 362's losing fragmentation.

## Research round thirty-five: remove the serialized state, not just dispatch

Task 364 adds one newly identified hot dependency to the total-cover plan: a closed region
must carry guest PC symbolically and materialize `InlineSite` only at a slow/throw/exit edge.
Per-leaf site advancement can serialize otherwise direct native composition. Task 366 owns
that experiment; Task 367 first makes every rustc/LLVM-derived typed hole fail closed under
compiler drift.

The same research adds two later representation experiments: Task 368 uses prototype and
property watchpoints to turn stable inherited hits into fixed loads/calls, and Task 369
splits the oversized universal object payload into compact ordinary/array/function kinds.
Neither precedes total native cover. Task 365 is an immediate measurement prerequisite:
the current split-process, rounded-score aggregate is a development metric, not an exact
upstream V8v7 acceptance score.

The immediate queue is therefore:

`365 exact score lane + 367 cooker audit ->`
`309/157/316/348 total cover + 366 symbolic PC ->`
`171/144/152/164/158/203 typed register-resident regions ->`
`145/153/154/368 machine-state IC arms -> 146/181 whole-region guest calls ->`
`172/329/191/176 effect and allocation erasure -> 165/214/265/369 compact storage`.

Full sources, proof counters, suite attribution and deferred ideas are recorded in [[364]].

## Task 378 and research round thirty-eight: lazy obligations beat eager state

Task 378 tested four realizations of the same structural name capability. Three eager
activation snapshots lost 5.84%, 5.22%, and 0.23%. A lazy POD lexical-address IC passed
the exact gate at +1.45% `[+0.67%, +2.13%]`, with Richards +4.65% and DeltaBlue +6.47%.
This is direct evidence for the Lisp staging rule: preserve the symbolic name/address
obligation as immutable site data, resolve it at the first demanded use, and derive the
machine view. Do not materialize every possible value at frame entry.

Research round thirty-eight keeps the architecture and changes the next queue:

`377 refresh frontier with physical Task 378 cost ->`
`12/165 dense computed + bitwise/numeric-unary joint closure ->`
`145/153 fused property-call IC recipe with native return ->`
`309/157/316/348/366 total mixed-region cover and symbolic PC ->`
`171/144/152/204 typed context propagation ->`
`191/321/369 inline allocation and contiguous payload`.

Do not prioritize pointer compression, eager snapshots, another helper-calling call leaf,
or benchmark-trained superinstructions. The current value is already one word; the
measured gap is continuous native dependency length. Sources and trial contracts are in
[[380]].

## Task 381: persistent contexts before isolated conversions

Direct dense computed access passed the exact gate at +1.57%
`[+0.03%, +3.36%]`. The apparently complementary isolated bitwise/numeric-unary catalog
then failed at -3.45% `[-8.65%, +0.49%]`, despite a positive short screen, and was
reverted. Priority therefore moves from more conversion-heavy opcode leaves to
context-carrying mixed regions: preserve I32/F64 facts across dense access and arithmetic,
or close a fully native property-load/direct-call continuum. Static closure still ranks
reach, but code size, conversions, and persistent representation facts decide the
realization.

## Research round thirty-nine: make evidence and catalog construction compositional

Task 391 adds two process dependencies and one offline code-generation mechanism without
changing the architectural critical path. First, Task 393 instruments the actual native
graph by composing observation endomorphisms at link time; diagnostic builds must stop
disabling direct stencils. Second, Task 392 derives mutable feedback fields only for
surviving recipe consumers. Third, Task 394 lets pinned rustc/LLVM evaluate bounded typed
micro-op forms offline and feeds only validated Pareto recipes into Task 157's existing
selector.

The immediate queue is:

`390 finish/measure patch algebra -> 393 honest native-path instrumentation ->`
`385 register-resident I32 physical cover -> 309/157 total micro-op cover ->`
`20/145/379 property-call plus direct return continuum ->`
`171/144/152/158 typed context propagation ->`
`392 demand-projected feedback -> 394 offline catalog expansion`.

Task 394 may prototype alongside the region work only after Task 309's semantic grammar is
canonical; it must not become a second hand-written opcode catalog. Task 392 moves earlier
only if Task 393's census proves feedback writes or footprint are on the measured critical
path. Neither item displaces the physical region and call-boundary work needed for the
remaining 4.35x aggregate gap at Task 381's accepted 2297.84 score.
