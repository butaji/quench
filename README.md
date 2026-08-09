# Quench

Quench is OXC program data plus unified facts, partial evaluation, a tiny
semantic algebra, macro-generated physical specialization, and a compact heap.

The governing principle is: **AST is data. Types are data. Shapes are data.
Semantics are combinators. The VM exists only for uncertainty.**

1. Never represent the same semantic fact twice.
2. OXC owns syntax; Quench does not create another syntax tree.
3. Static structure remains data or disappears.
4. VM code represents only dynamic uncertainty.
5. Semantic abstractions do not imply runtime allocations.
6. Share semantic mechanisms; specialize physical execution.
7. One declaration generates every mechanical consequence.
8. Generated LOC is cheap; handwritten semantic LOC is expensive.
9. Facts are `Proven`, `Guarded`, or `Unknown`.
10. Never optimize through observable JavaScript behavior.
11. Heap references remain compact.
12. No subsystem gets an independent universe without semantic need.
13. Types are facts, not another runtime.
14. Profiles are facts, not another optimizer.
15. A future JIT consumes the exact same residual operations.
16. Work that can disappear before runtime must justify remaining.

The canonical path is OXC AST and semantic data to `ProgramDb`, then a reducer
to residual operations. The VM exists only for dynamic uncertainty.
