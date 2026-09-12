# 195 — Effect-aware GVN and partial redundancy elimination

Status: planned

Partition quoted SSA values into congruence classes keyed by opcode, operand value
numbers, representation/shape facts, and the Task 173 effect token. Reuse dominating pure
results. For partial redundancy, insert the expression only on predecessors where absent
and join through an SSA block argument.

Begin with constants, local loads, raw guarded numeric operations, tag/shape guards, and
immutable shape metadata. Heap loads are ineligible until MemorySSA proves the same
clobber token. Ownership SSA inserts any required copy or move; coercive arithmetic,
allocation, and throwing operations are never assumed pure.

All fixed-point, congruence-class, and PRE insertion budgets are named constants.
Acceptance: property tests compare optimized/unoptimized terms including NaN, signed
zero, overflow, throws, and ownership; counters report dynamic operations eliminated;
compile/link overhead remains bounded; full V8v7 A/B improves.

Primary source: LLVM NewGVN and scalar/load PRE,
<https://www.llvm.org/docs/doxygen/NewGVN_8cpp.html> and
<https://llvm.org/doxygen/GVN_8h_source.html>.

