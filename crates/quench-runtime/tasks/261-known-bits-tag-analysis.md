# 261 — Known-bits analysis for NaN-box tag and payload erasure

Status: planned

Add a small, immutable abstract domain for the existing 64-bit `RawValue` encoding:

`KnownBits { zero: u64, one: u64 }`

The two masks are disjoint. Unknown is both masks empty; a constant has every bit known;
join retains only facts true on every incoming edge. Tag guards refine successor facts,
and macro-generated transfer functions cover the finite raw operations used by semantic
recipes: mask, or, xor, shifts, tag insertion/removal, payload extraction, constants,
boxing, and unboxing.

This is one component of Task 171's canonical `AbstractValue`, not a parallel type
system. Task 174 owns CFG reachability and the propagation worklist; this task owns the
bit-level lattice and rewrites. Task 149's semantic macro generates transfer metadata so
tag semantics are not implemented twice. Every mask, tag, shift, bit width, and payload
range comes from named constants in `raw_value.rs`; no numeric encoding literal may
appear in analysis or lowering code.

Use the result to:

- fold tag comparisons proven true/false;
- erase redundant tag masks and payload extraction/reinsertion;
- keep an unboxed number or raw heap reference across a composed region;
- eliminate box→guard→unbox pairs exposed by guarded call/IC inlining;
- classify GC roots at materialization edges without re-testing a proven value.

Unknown/conflicting facts conservatively select the generic tagged stencil. A fact may
cross only effects whose Task 173 alias/effect summary preserves it. No known-bit object
survives in executable memory and no runtime analyzer is introduced.

Acceptance: exhaustive tests over every defined immediate tag and representative pointer
and double payloads; property tests for join/transfer monotonicity; differential execution
with the analysis disabled; counters for tag checks, masks, boxes, and unboxes erased;
disassembly proves a guarded multi-operation region checks its tag once; complete V8v7
A/B improves without component regression.

Primary sources: LLVM's two-mask `KnownBits` abstraction and context-sensitive value
tracking (<https://llvm.org/doxygen/KnownBits_8h_source.html>,
<https://llvm.org/doxygen/ValueTracking_8h.html>); weval's proposed partial-known-bits
optimization for NaN-boxing after guarded inlining
(<https://cfallin.org/pubs/pldi2025_weval.pdf>).

