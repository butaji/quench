# 191 — Dominance-based allocation folding

Status: in_progress

After the VM heap exists, group dominated allocations into one bump-pointer reservation
with one limit check. Patch each object header/field offset inside the reserved range.
Objects in a group share a generation, so initialization stores between them can omit
write barriers.

Model the proof in connector contexts: `AllocateGroup` produces typed `Fresh<T>` handles;
`Initialize` consumes them; `Publish` yields ordinary references. Any `MayGc` or unknown
effect consumes freshness and ends the group. Allocation grouping is derived from the
SSA/effect graph and needs no hotness or benchmark identity.

Maximum group bytes/objects and dominance-walk limits are named constants. Acceptance:
groups never cross calls, safepoints, observable allocation failures, or exceptional
effects; barriers are removed only for fresh same-group stores; allocation-heavy tests
show one limit check for multiple objects; full V8v7 A/B improves.

Primary source: <https://research.google/pubs/allocation-folding-based-on-dominance/>.

## First implementation slice: allocation-site shape folding

Before the region heap is available, apply the same freshness proof to hidden-class
initialization. For each `NewObject` in a basic block, scan until the result register is
read, overwritten, or otherwise published. Static property writes to that still-fresh
object form an ordered, duplicate-free key sequence. The JIT interns that shape once,
stores its immutable pointer in the allocation site's descriptor, and allocates the
object with its final slot capacity. Subsequent `SetStatic` operations overwrite fixed
slots instead of performing shape transitions and growing `PropertySlots` per object.

The scan is conservative across every bytecode through the shared read/write predicates;
it is independent of source syntax, benchmark identity, and execution count. This is a
preparatory stencil/kernel split: the immutable shape is a shared kernel fact and the
site descriptor is its patched instance. Direct allocation stencils and multi-object
bump reservations remain incomplete.

Before delaying or fusing initialization, reject a static write whose source register is
the same register as the fresh allocation destination. Such an instruction reads the
object being initialized and therefore is a publication/use boundary, not a pure field
initializer. Keep this condition in the canonical read/write/freshness facts so every
future construction-recipe matcher derives the same safety decision.

The first implementation exposed one important representation invariant: an ordinary
`NewObject` site must carry the named `NO_ALLOCATION_SHAPE_POINTER` sentinel. Reusing the
generic unused-operand value (`usize::MAX`) as though it were a `ShapeRef` produced an
invalid raw pointer. The linker now writes the zero sentinel explicitly, and focused
tests cover both ordinary allocation/re-entry and the folded-versus-escaped proof.
All 87 unit tests pass.

Measurements are recorded in:

- `reports/task191-allocation-shape-focused-ab-5/comparison.txt`: four object-oriented
  suites improved geometrically by 2.51%, including Splay +7.84%.
- `reports/task191-allocation-shape-full-ab-5/comparison.txt`: the noisy complete suite
  was aggregate-neutral (1795.29 to 1795.07, -0.01%), with Splay +9.38% but RegExp
  -6.41%.
- `reports/task191-allocation-shape-splay-regexp-ab-7/comparison.txt`: a longer
  seven-repetition confirmation resolved that apparent tradeoff at Splay +10.31%,
  RegExp -0.29%, and a two-suite geometric mean of +4.87%.

Retain this slice for its repeatable object-workload gain. Task 191 remains in progress:
it has not yet produced direct allocation stencils, a region heap, or grouped bump
reservations, and it does not claim a complete-suite aggregate improvement.
