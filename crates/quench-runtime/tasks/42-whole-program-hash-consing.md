# 42 — Whole-program structural hash-consing of stencil nodes

Status: planned

[[20-inline-via-node-composition]] and [[14-code-image-reuse]] dedupe by AST function identity. Extend the same sharing idea to structurally identical `StencilNode` subtrees that arise from different source locations — for example, two different call sites that desugar to the same guard-check sequence, or repeated small patterns across a large source file. Build a hash-cons table over `StencilNode` (structural hash plus equality) so `Rc::ptr_eq`-based sharing, already used within [[23-egraph-rewriting]]'s quotient-category rewrite pass, applies across the whole program rather than only within one function's local rewrite.

This shrinks the executable image and improves instruction-cache behavior specifically for large, pattern-repetitive sources such as earley-boyer (4700+ lines), where the same small dispatch/guard idiom recurs across many interpreter primitives.

Correctness constraint (cross-checked against [[48-submorphism-repatch-on-guard-miss]]): a hash-consed node is immutable for the remainder of the program's execution once shared — no later optimization pass, and specifically not [[48-submorphism-repatch-on-guard-miss]]'s guard-miss repatching, may mutate a hash-consed node in place. A site needing a different specialization after sharing must be repointed at a newly allocated node (copy-on-write against the hash-cons table), never by editing the shared node itself, since every other site holding that `Rc` would otherwise be silently and incorrectly repatched too.

Acceptance: two structurally identical subtrees originating from different source locations share one `Rc<StencilNode>` after hash-consing, verified by pointer identity; total executable image size decreases on earley-boyer relative to the non-hash-consed baseline; hash-consing introduces no behavior change on any existing correctness or composition test; a guard-miss patch (per [[48-submorphism-repatch-on-guard-miss]]) at a hash-consed site produces a new node without mutating the shared original, verified by confirming other sites' `Rc` addresses and behavior are unchanged.

## Sharper key: fact-signature identity, not only syntactic structural identity

The dedup key stated above (structural hash/equality of `StencilNode`) only catches two
sites that are *syntactically* identical trees. A strictly stronger and more general key
is the **fact-signature** [[240-fibered-kernel-selection-over-shape-lattice]] already
uses for its fiber-selection criterion: two sites with different source syntax but the
same input/output fact signature (e.g. `Int32 → Int32` addition reached via two
differently-shaped source expressions that both reduce to the identical guarded
arithmetic morphism) should share the same compiled kernel, even though their
`StencilNode` trees before reduction were never syntactically equal. This subsumes the
syntactic-equality case (identical syntax always yields identical fact signatures, never
the reverse) and should be the actual dedup key once [[240]]'s fibration is in place;
until then, this task's syntactic-equality version remains a correct, strictly weaker
approximation worth shipping first. Cross-check against [[48]]'s immutability constraint
applies identically to fact-signature-keyed sharing: a shared kernel reached via
fact-signature equality is exactly as immutable-once-shared as one reached via syntactic
equality, for the same reason.

## Canonical context keys prevent specialization duplication

Hash-cons version requests as well as syntax. Normalize every Task 144 context before it
becomes a key: sort commutative facts, intern shapes and effect summaries, erase dead-value
facts, canonicalize equivalent ranges, and widen through the same deterministic lattice
operation. The key is `ContextKey { function, entry_block, live_fact_signature }`, never a
source address, allocation identity, arrival order, execution count, or benchmark name.
Two requests with the same normalized key reuse one quoted version and, when relocation
closure permits, one immutable instance or kernel.

This imports the useful lesson from context-guided method splitting—many separately
requested versions are redundant—without adopting runtime hotness. In TruffleRuby, reuse
by argument context substantially reduced redundant splitting and compilation/GC cost;
the result motivates measuring canonical reuse but is not projected onto this VM:
<https://kar.kent.ac.uk/109418/>.

Acceptance adds counters for raw requests, normalized unique keys, shared quoted versions,
shared physical images, and widening collisions. Canonicalization must be idempotent, and
two alpha-renamed local regions with identical live fact signatures must produce equal
context fragments before intentional function identity is added to the whole-program key.
