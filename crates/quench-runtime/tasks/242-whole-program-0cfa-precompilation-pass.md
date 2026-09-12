# 242 — Whole-program 0-CFA as a single pre-compilation analysis pass

Status: planned

Names the concrete mechanism for "having the whole program script available before
execution starts lets you make safe assumptions before execution begins" — a claim
several already-planned tasks rely on ([[46]]'s closed-world mode, [[69]]'s
whole-call-graph colimit, [[177]]'s interprocedural summaries, [[235]]'s singleton
call-target proof) but none of which states the actual whole-program static-analysis
algorithm that computes those facts in one pass, ahead of any per-site guard work. The
standard technique for exactly this — computing, for every variable/parameter/call site
in a whole first-class-functions program, the *set* of values/functions/shapes that
could ever reach it — is **0-CFA** (Shivers' zeroth-order Control Flow Analysis, the
foundational whole-program flow analysis for languages with first-class functions and
dynamic dispatch, of which points-to analysis for objects — Andersen-style inclusion-
based analysis — is the object-oriented specialization relevant to this project's shape
lattice).

**Why this belongs as its own task rather than folded into [[46]]/[[69]].** Those tasks
describe the *result* (a closed-world shape colimit, a singleton callee set) as
something [[46]]'s pass "computes," without naming the algorithm or its complexity
class, cost, and precision trade-offs — 0-CFA is famously cubic in naive form (and its
context-sensitive refinements more expensive still), which matters directly for
[[00-optimization-routine]]'s standing discipline that this is an AOT compile-time cost,
not a runtime one, but still needs a stated bound so [[04-bytecode-coverage-map]]'s
compile-time budget for the full V8v7 corpus (largest file: earley-boyer.js at 4686
lines) is a known, not a discovered-too-late, cost.

Concrete steps:
1. Run monovariant (0-CFA proper, not a context-sensitive refinement — start with the
   cheapest sound version, matching this project's standing practice of using the
   smallest mechanism that closes a measured gap before reaching for a more expensive
   one) whole-program flow analysis once, before any per-function stencil compilation
   begins, over the closed-world program [[46]] already scopes (no `eval`/dynamic
   `load`).
2. From its result, derive directly: [[235]]'s singleton-call-target sites (a call
   site's 0-CFA-computed reachable-callee set has size 1), [[220]]'s closed-coproduct
   membership (a constructor's 0-CFA-computed reachable-construction-site set is exactly
   the tag set already assumed), and [[46]]'s shape-colimit inputs (which construction
   sites can reach which property-access sites, replacing whatever ad hoc reachability
   argument [[46]] currently uses with the analysis's actual output).
3. State the analysis's own soundness/precision boundary explicitly and feed it to
   [[224]]'s blame-calculus discipline: a 0-CFA result is a sound *over-approximation*
   (it may report more reachable values than actually occur, never fewer), which is
   exactly the property that makes every fact it produces safe to guard on and, where
   the approximation happens to be exact (a singleton set that really is a singleton,
   not merely bounded), safe to erase per [[231]].
4. Measure and record the pass's own compile-time cost against the full V8v7 corpus, so
   it is a known, bounded part of this project's AOT pipeline rather than an
   unaccounted-for cost discovered during [[15]]'s acceptance gate.

Acceptance: one 0-CFA pass runs once per closed-world program before stencil
compilation and produces, as reusable data, the reachable-callee sets and
reachable-construction-site sets that [[220]], [[235]], and [[46]] currently derive
separately or assume; at least one of those three tasks is re-pointed at this pass's
actual output rather than its own ad hoc discovery, with disposition documented (agrees,
or a discrepancy found and resolved); the pass's compile-time cost on the full V8v7
corpus is measured and recorded as a named budget line, not left implicit; a program
using `eval`/dynamic `load` (outside [[46]]'s closed-world scope) is confirmed to
correctly skip this pass and fall back to the existing per-site local guard machinery,
not attempt unsound whole-program analysis on an open program.

Primary sources:
- Shivers, *Control-Flow Analysis of Higher-Order Languages* (PhD thesis, 1991) — the
  original 0-CFA formulation: <https://www.cs.cmu.edu/~fp/courses/15819-f09/lectures/0-shivers-cfa.pdf>
- Andersen, *Program Analysis and Specialization for the C Programming Language* (PhD
  thesis, 1994) — the inclusion-based points-to analysis specialization relevant to this
  project's shape/object lattice.
