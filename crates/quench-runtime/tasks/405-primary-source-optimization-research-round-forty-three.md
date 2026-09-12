# 405 — Primary-source optimization research, round forty-three

Status: complete

This round searched for additional algorithms after the accepted Task 401/404 checkpoint,
then removed ideas already represented by the task graph. The current engine already has an
eight-byte tagged value, immutable hash-consed shapes, fixed property slots, traced stable
object handles, direct dense elements, and first-execution copy-and-patch code. Repeating
NaN-boxing, hidden classes, or another isolated opcode leaf is therefore not new work.

## Ranked findings

1. **Close the inherited-property-to-call edge first (`368`, `401`).** The new call-region
   census records many entries into the coarse method-call stencil but zero V8v7 direct-call
   attempts: Richards methods are inherited, while the cooked composite accepts only an own
   property cache. JavaScriptCore's Object Property Conditions give the right general form:
   `Presence | Absence | Equivalence`, guarded by shapes/watchpoints. For the first bounded
   implementation, publish an immutable one-level `Presence(receiver_shape, holder_identity,
   holder_shape, slot)` record beside the existing own-property record. The stencil validates
   the receiver and immediate prototype, loads the fixed holder slot, and continues into the
   already-composed call. Longer chains and mutation use the canonical semantic continuation.
   This is the first task because it unlocks code that is already linked; it is not a new
   benchmark-shaped stencil.
2. **Perform final-link AArch64 address-mode relaxation (`189`, `390`, new `406`).** A 2025
   copy-and-patch implementation for R reports that moving from absolute to PC-relative
   addressing improved its compilable workloads by more than 8% on average and reduced code
   size 26%; those numbers do not transfer to this VM, but the mechanism is directly relevant.
   CPython's current AArch64 stencil linker chooses among immediate materialization,
   `adrp+add`, literal load, and the original relocation after final addresses are known.
   Generalize our patch algebra so one logical address obligation owns several equal-width
   encodings, selected only during final link. Keep near-code shared kernels immutable and
   shared; do not copy one kernel per function.
3. **Use a stack-cache/context automaton across whole regions (`158`, `385`).** Vmgen's
   dynamic-superinstruction work shows that superinstructions and register/stack caching are
   complementary: eliminating dispatch exposes stack traffic, while explicit cache-state
   transitions make combined instructions useful. In this register VM the analogous state is
   a bounded map from SSA values to `GPR | FPR | FrameSlot`. Each stencil is a typed transition
   between cache states; edge recipes perform parallel copies, and identity transitions emit
   no code. This is the physical mechanism needed for C-like arithmetic, not a larger catalog
   of boxed leaves.
4. **Fold equivalent IC arms before widening them (`153`, `154`, `159`).** CacheIR separates
   ids, per-stub fields, and immediates, then folds stubs that differ only in `GuardShape` into
   `GuardMultipleShape`. Normalize inherited and own method cases by their effect and result
   recipe. Shapes sharing the same holder/slot/callee use one minimorphic arm; different
   offsets use a bounded PIC; overflow reaches one shared megamorphic kernel.
5. **Use allocation-site policy as a monotone state machine (`162`, `192`).** V8's Memento
   Mori ties allocations to sites and uses the observations for presizing, pretransitioning,
   and pretenuring. This VM must not introduce execution-count hotness: the compatible form is
   a bounded semantic policy updated only by capacity transitions and GC survival. The next
   allocation reads one immutable snapshot, while wrong predictions remain correct slow
   transitions.
6. **Classify RegExp plans before selecting the matcher (`87`).** RE2's one-pass test finds
   patterns for which each input byte determines a unique continuation, eliminating NFA
   thread bookkeeping. Add `OnePass`, `WordNfa`, and `General` constructors to the quoted
   pattern plan. A one-pass matcher becomes a shared immutable kernel or a pattern-patched
   stencil; small epsilon-closed NFAs may update a machine-word state set; the current Rust
   regex kernel remains the total semantic case. This is lower priority because the accepted
   RegExp score is already much higher than Richards/DeltaBlue.
7. **Use proved numeric-text algorithms only after erasing their call seam (`280`).** Ryū
   converts binary floats to shortest round-trippable decimals using fixed-size integer
   operations; Eisel–Lemire parses the common decimal case with a 64-bit significand and a
   proved fallback. These are better candidates than ad-hoc formatting, but Task 280 already
   showed that a faster inner conversion does not matter while the generic builtin call
   dominates. First compose the builtin recipe and borrowed string/result allocation edge;
   then compare algorithms behind the same `NumericTextRecipe` interface.
8. **Keep argument count in the canonical guest frame (`146`, `187`, `188`).** V8's
   adaptor-frame removal demonstrates a general exact/variable-arity layout: reverse argument
   order, pad under-application once, store actual count in the callee header, and let return
   pop the correct extent. Exact-arity images can omit even that adaptation through a tagged
   entry point. This matters after inherited method calls reach the native call arm.

## Lisp/category shape

No finding adds an independent optimizer. Extend the existing quoted sums:

```text
PropertyCondition = Own | Presence | Absence | Equivalence | Unknown
AddressPlan       = DirectBranch | PageRelative | Literal | Absolute
MatcherPlan       = OnePass | WordNfa | General
AllocationPolicy = Initial | Presized | Pretransitioned | Pretenured
```

Macroexpansion derives and normalizes these values; costed selection chooses a lawful
representation; final linking is the single effect. Kernels are immutable shared morphisms.
Stencil templates become private patched instances. Both implement the same typed connector,
so a property condition, call, matcher, or allocation result composes at operation, block,
loop, function, and interprocedural levels without demoting the result to raw bytes.

## Recommended execution order

1. Finish the one-level inherited `Presence` projection inside Task 401/368 and prove that
   V8v7 `direct_call_entries` lead to nonzero attempts/hits.
2. Implement arbitrary register-resident I32/Word32 region state (`385` over `158`) rather
   than more per-op bitwise leaves.
3. Run the near-code plus final-link address-relaxation experiment (`189`, `406`) with code
   bytes, indirect branches, and exact A/B as co-equal gates.
4. Add recipe folding and bounded context versions (`153`, `154`, `144`, `152`).
5. Move allocation policy/GC and the RegExp/numeric-text specialists only after fresh native
   profiles show their boundary is exposed.

Execution update: item 1 is implemented as the accepted bounded slice of Task 368. Richards
now reaches 3,502,225 successful direct-call hits in the recorded census, and the nine-pair
exact checkpoint improves aggregate 2.10% with a wholly positive confidence interval. The
next implementation target is therefore item 2, the register-resident context automaton in
Task 385, unless a fresh profile invalidates that ordering.

## Primary sources

- Deegen, first submitted 18 November 2024: <https://arxiv.org/abs/2411.11469>
- Copy-and-Patch Compilation: <https://arxiv.org/abs/2011.13127>
- Copy-and-patch JIT for R (2025):
  <https://www.itspy.cz/wp-content/uploads/2025/09/it_spy_2025_diplomova_prace_50.pdf>
- CPython's AArch64 stencil relaxation: <https://github.com/python/cpython/blob/main/Python/jit.c>
- JavaScriptCore property conditions and adaptive watchpoints:
  <https://webkit.org/blog/6756/es6-feature-complete/>
- Firefox CacheIR and stub folding: <https://firefox-source-docs.mozilla.org/js/cacheir.html>
- Vmgen and register-cached superinstructions: <https://doi.org/10.1002/spe.434>
- V8 allocation-site policy: <https://research.google/pubs/memento-mori-dynamic-allocation-site-based-optimizations/>
- RE2 one-pass matcher: <https://github.com/google/re2/blob/main/re2/onepass.cc>
- V8 RegExp native/threaded execution: <https://v8.dev/blog/regexp-tier-up>
- Ryū shortest conversion: <https://doi.org/10.1145/3192366.3192369>
- Eisel–Lemire number parsing: <https://arxiv.org/abs/2101.11408>
- V8 adaptor-frame removal: <https://v8.dev/blog/adaptor-frame>

## Post-research execution result

Task 407 rejected three smaller substitutes for the register-resident context: loop-local
forwarding, burned frame-resident bitwise/shift leaves, and a stable direct-call activation
lease. The first two did not erase conversion/frame seams; the third measured +0.10% with a
confidence interval spanning zero. The recommended execution order therefore remains
unchanged: build Task 385's physical register-resident context before adding more arithmetic
leaves.
