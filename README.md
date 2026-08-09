# Quench

Quench is OXC program data plus unified facts, direct residualization, one
completion-aware semantic path, generated runtime mechanics, and a compact
indexed heap.

The governing principle is: **AST is data. Types are data. Shapes are data.
Semantics are combinators. The VM exists only for uncertainty.**

1. Never represent the same semantic fact twice.
2. OXC owns syntax; Quench does not create another syntax tree.
3. Static structure remains data or disappears.
4. VM code represents only dynamic uncertainty.
5. Semantic abstractions do not imply runtime allocations.
6. Share semantic mechanisms; specialize physical execution.
7. One declaration generates every mechanical consequence.
8. Generate mechanics, handwrite observable algorithms, and budget both the
   generated binary and handwritten source.
9. Facts are `Proven`, `Guarded`, or `Unknown`.
10. Never optimize through observable JavaScript behavior.
11. Heap references remain compact.
12. No subsystem gets an independent universe without semantic need.
13. Types are facts, not another runtime.
14. Profiles are facts, not another optimizer.
15. Optional native execution is disposable and consumes the exact same
    residual operations.
16. Work that can disappear before runtime must justify remaining.
17. Complete semantics precede specialization; unknown behavior stays cheap.
18. Ordinary calls use compact stack frames; continuations exist only for
    genuine suspension.
19. Runtime text, static data, caches, and generated code count toward RSS.

The canonical implementation order is:

```text
canonical semantics and completions
  -> compact indexed heap and shapes
  -> flat residual Ops and compact frames
  -> five reducer contexts and operational ProgramDb facts
  -> generated mechanical declarations
  -> bounded guarded Ops
  -> measured interpreter fusion
  -> optional disposable baseline-native execution
```

The performance claim is workload-specific rather than universal: target
V8-class execution where reduction, compact storage, or bounded specialization
provides a structural advantage, while keeping cold start and RSS dramatically
smaller. Full conformance always uses the same complete semantic path.
