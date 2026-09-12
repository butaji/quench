# 396 — Guard-failure native-cover DAG experiment

Status: in_progress

Replace the current all-or-nothing numeric-region fallback with a bounded, prelinked
decision DAG whose leaves are compatible native context versions. This is the first
executable slice of Tasks 144 and 371, selected by Task 393's honest guard events.

## Canonical quoted form

```text
GuardAtom = Source × Requirement × FailureReason
Context   = Product<Representation, Location, Shape, ElementKind, Range, Ownership, Effects>
GuardDag  = Test(GuardAtom, pass: GuardDag, fail: GuardDag)
          | Enter(ContextId, EntryTag)
NativeCover = ContextPredicate × Arc<StencilInstance | Kernel>
```

Normalize equivalent atoms and context predicates, hash-cons shared suffixes, and order
independent atoms using Task 157's static physical cost. Every leaf is an existing typed
stencil/kernel morphism derived from the same bytecode semantics. The final leaf is the
total `Unknown` semantic kernel, never an AST interpreter.

No runtime count selects code. Link time enumerates the bounded context demands derived from
the bytecode CFG and installs every admitted cover. Runtime guard values only choose an edge
of that already-linked coproduct. A site may cache the first compatible leaf identity, but
the complete decision semantics and all alternatives remain immutable.

Use named constants for `MAX_GUARD_DAG_NODES_PER_REGION`,
`MAX_NATIVE_COVERS_PER_GUARD_EXIT`, `MAX_CONTEXT_FACTS_PER_COVER`, and
`MAX_REPORTED_GUARD_FAILURE_SITES`. Overflow widens facts in the canonical lattice and
selects a weaker cover; it never drops correctness conditions.

## First measurement slice

Extend Task 393's observation plan with guard success/failure events keyed by region start,
requirement source, requirement kind, and `GuardFailure` constructor. Aggregate counts in a
separate diagnostic image without changing direct selection. The report must distinguish:

- missing value;
- non-number;
- non-array-index;
- non-dense array;
- non-packed-number element;
- property owner, presence, inheritance, and numeric-value failures.

The first code candidate chooses the largest general normalized region family where one
failed fact currently discards a nonempty native suffix or alternate numeric cover. Prelink
the original strongest context, one weaker compatible context, and the total generic
kernel. On failure, preserve already-proven live facts and jump native-to-native. Do not add
a source-name, benchmark-name, property spelling, literal value, or execution threshold to
the selector.

## Laws and acceptance

- Context implication is reflexive and transitive; each runtime state selects the unique
  most-specific compatible cover or is rejected at link time as ambiguous.
- Factoring the same guard formula in different orders yields extensionally identical leaf
  selection, subject only to the explicit physical cost order.
- Every DAG edge satisfies its typed connector and canonical side-exit frame-state contract.
- Shared `Kernel` and `StencilInstance` leaves are interchangeable realizations of the same
  morphism and remain immutable after publication.
- Differential tests force every `GuardFailure` variant, overflow, `NaN`, negative zero,
  holes, shape changes, prototype changes, ownership cleanup, and exceptions.
- Disassembly for the alternate arm contains no `execute_region_fallback`,
  `dyn_block_step_impl`, frame-wide reboxing, or unrelated-slot materialization.
- Counters prove fewer semantic-kernel entries and report added code bytes, DAG nodes,
  guards, native-cover selections, generic selections, and materializations.
- Retain the execution path only if the complete alternating exact V8v7 gate improves with
  no component-floor violation. Diagnostic attribution may remain if its disabled form is
  byte-identical.

Primary sources: lazy basic-block versioning <https://arxiv.org/abs/1411.0352>, Deoptless
specialized continuations <https://arxiv.org/abs/2203.02340>, and SpiderMonkey CacheIR's
guard/idempotent/result normal form <https://firefox-source-docs.mozilla.org/js/cacheir.html>.

## First implemented cover refinement

The Task 393 census found that Crypto's largest failure site was not polymorphism. The
region beginning at source byte 3484 failed 151,854 times in one 20 ms diagnostic run at
`var this_array = this.array;` (`crypto.js:109`). `PropertyRequirement` incorrectly gave
every static property result the `Number` representation, even when the result was merely
copied to a local and was intentionally a dense-array object.

Property demands now carry the explicit lattice
`TriviallyCopyable <= Number`. A read used by numeric operations derives `Number`; an
otherwise copied slot derives `TriviallyCopyable`. The latter admits immediates and
GC-managed object handles but rejects the `Rc`-owned string/function/RegExp categories, so
raw stencil copies do not invent ownership tokens. Property shape/prototype/slot guards and
numeric writes remain unchanged. This is a general representation cover derived from uses,
not from a property spelling or benchmark identity.

Admitting the prologue exposed a second false fact: the following loop guarded scratch
locals that have a unique dominating `WriteLocal` inside every executing path. A CFG
fixed-point now computes unique reaching local definitions by intersecting predecessor
maps. Requirement resolution composes through that producer register. A branch that can
skip the store deliberately collapses the fact to `Unknown` and retains the external local
guard.

In `reports/task396-copyable-property-candidate/crypto-reaching-defs.log`, guard failures
fell from 160,922 to 706 for the same 20 ms diagnostic class. The 151,854 property failures
and 2,688 ordinary number failures became successful native entries; the large loop at
byte 3566 succeeded 153,798 times. Remaining failures are 456 non-packed-number arrays and
250 missing properties. The diagnostic Crypto score moved from roughly 1,746 to 1,816.

Correctness passes 152 release tests, including direct laws for a unique reaching
definition, a branch that skips a definition, numeric property-demand derivation, and an
object-valued trivially-copyable property. The full-suite 20 ms smoke is 2,120.47; this is
not an acceptance measurement.

The isolated nine-pair exact comparison against a same-source control is recorded at
`reports/task396-copyable-property-candidate/exact-same-source-control/comparison.md`.
Crypto improves **8.18%** with interval **[+7.30%, +8.93%]**, but aggregate moves only
**+0.72%** with interval **[-0.21%, +1.73%]**. The gate correctly rejects this candidate
because the aggregate interval includes zero. The representation lattice and proof repair
remain correctness infrastructure and become part of the accepted Task 397/399 candidate;
the broader multi-leaf native-cover DAG remains in progress.
