# Heap identity and storage architecture

## Decision

Use a stable identity-bearing header for each mutable JS object/array and separately
managed backing storage. Shape, element representation and backing validity are
different facts. Derive IC guards, native access contracts and invalidation from
the canonical mutations; no subsystem owns a parallel replacement/shape universe.
This is the recommended direction given current evidence, not proof of universal
optimality or an instruction to rewrite the collector before task075 completes.

Current ArrayData has identity, DenseElements, length/kind, named properties and
descriptor vectors. DenseElements already distinguishes numeric and generic data;
shared numeric writes coexist with structural COW. ObjectData has identity/layout,
replacement state and properties. The missing architectural invariant is stable
identity across ordinary structural mutation without repeated whole-object copies
and alias repair. Existing shape and numeric mechanisms should be retained.

Evidence:49.53M stale-array guard misses in Crypto,3.73M object alias rebuilds in
RayTrace, and slice destination controls separating large time/RSS costs from
generic source reads. These motivate the change; they do not quantify every cost
or prove that a particular allocation strategy will win on all workloads.

## Alternatives and trade-offs

| Choice | Benefit | Cost |
| --- | --- | --- |
| COW header plus forwarding/replacement maps | Convenient immutable Rust APIs | Mutable JS aliases require canonicalization; copies/retention/invalidation can dominate |
| Stable nonmoving header, mutable backing | Cheap common identity check; direct region access | Requires disciplined interior mutation and explicit collector ownership |
| Compact generational handle table | Stable references even if objects move; compact slots possible | Extra lookup, handle-table footprint and ABA/reuse checks |
| Moving direct pointers | Strong compaction/locality potential | Precise roots and pointer updates at every relocation boundary |

Prefer stable headers for the current migration. Rc can temporarily own a header;
that does not require Rc cloning per scalar operation or a RefCell check per native
load. Do not introduce a second wrapper allocation around every existing object.
Compare header size/indirections with handle-table designs before pointer compression
or a moving nursery. Ordinary JS object lifetime stays separate from Rust code-store
and native-code leases. A tracing-GC migration remains a separately justified choice.

## Mutation contracts

- Value-only store: preserve layout and backing validity where physically true.
  Invalidate any dependency on that value being constant; run required GC barriers.
- Shape transition: update named-property layout and relevant dependencies, without
  changing JS identity. Prototype identity alone does not prove prototype contents.
- Element-kind change: invalidate incompatible numeric/native views. Keep holes,
  undefined, integer conversions, NaN and negative zero semantically distinct.
- Backing resize/detach: invalidate raw base pointers and bounds assumptions even
  when shape/kind is unchanged. Reacquire pointers after callback-capable boundaries.
- Descriptor/prototype/extensibility mutation: invalidate only dependent sites,
  retaining complete generic semantics. Unknown effects kill the relevant facts.

Versions need not mean a separate counter for every field on every object. Prefer
immutable shape IDs and a minimal storage generation where sufficient; add sparse
watchpoints for assumptions that need them. Define wrap/reuse behavior. A hot value
write must not globally invalidate all regions just because one counter changed.

## Physical storage and Rust boundaries

Default named-property attributes belong in shared layouts; default element
attributes are implicit. Store exceptional overrides sparsely, and create public
descriptor objects only for observable APIs. Dictionary fallback must be deliberate
for churn/sparsity, not the ordinary array or stable-record representation.

Unboxed fields require an invariant about their representation, not just an observed
shape or allocation site. Generalization must preserve all aliases and invalidate
old specialized accesses. Keep tagged storage where precision/lifetime evidence is
insufficient. Allocation-site predictions are guarded hints, never hidden JS types.

Acquire a short-lived storage view after semantic checks. Native code may hold a
raw base through an effect-free bounded region; helper calls, reentry and collection
require explicit root/materialization and pointer refresh. Do not create overlapping
Rust mutable references to aliases. Raw pointer use and interior mutability need a
documented safety invariant, not a blanket UnsafeCell exemption.

Rust macros should derive access descriptors, mutation effect classes, guard/kill
rules and diagnostic labels from one layout contract. Do not generate thousands of
offset-specific stencils: offsets are patches unless encoding requires another form.

## Migration and validation

1. Specify identity, backing lifetime, mutation effects and root ownership for
   existing types; identify all IC/native consumers and historical alias mechanisms.
2. Migrate one ordinary representation vertically through generic operations,
   public descriptors, ICs, native views and tracing. Remove its superseded forwarding
   mechanics; do not keep two authoritative stores indefinitely.
3. Measure structural copying, canonicalization, descriptors, allocation/live bytes
   and stale-view exits independently. Preserve released and genuinely retained
   workloads; pool high-water memory is not automatically a leak or a success.
4. Add guarded copy/allocation/property/loop stencils once storage makes them cheap.
   Keep custom species, proxies, inherited indexed properties and callbacks ordinary.

Compare with current storage using fixed work, matched builds and representative
object/array/call workloads, not only one originating fixture. A successful design
reduces mutation-induced copies and stale-view exits without raising cold RSS or
header/cache footprint beyond the measured benefit. Collector replacement and
compressed references require their own throughput/latency/RSS comparison.

[V8 fast properties](https://v8.dev/blog/fast-properties) illustrates shared named
layout metadata and separate indexed storage. [Elements kinds](https://v8.dev/blog/elements-kinds)
illustrates representation-specific optimization and hole checks. Borrow these
mechanisms, not their exact enum count, transition policy or collector design.
This contract complements [activation ownership](activation-architecture.md).

## Layout identity versus derived views

Current `ObjectData::semantic_layout_id` caches an interned layout; named ICs
already reuse it. Separately, `ObjectData::shape` hashes all visible property
names, and `is_dictionary` calls that scan. Trace-off Richards/RayTrace samples
include `is_dictionary`, so this is not only unused descriptive code. Do not
claim every IC still hashes shapes: the named IC explicitly avoids that work.

The views differ: the visible shape excludes internal names, while canonical
layout includes descriptor/deletion entries. Preserve those semantics. Prefer one
immutable layout record with derived visible-slot count, dictionary eligibility
and physical-slot mapping. Compute structural facts on layout creation/transition,
not ordinary reads. Value mutation should invalidate only facts it can change.

Do not conflate interned layout IDs and deterministic diagnostic fingerprints
merely because both currently fit a `ShapeId(u32)`. Use explicit identity domains
or derive diagnostic fingerprints from canonical records. Hash equality alone is
not a semantic proof; interning needs collision resolution and code/realm lifetime
rules. Audit consumers before migration, including legacy shape-slot helpers.

Likewise, `PropertyEntries::entries` yields owned decoded Values; the default
descriptor search therefore loads values before checking names. Existing direct
value lookup already specializes this for ObjectProperties. Prefer key/metadata
selection followed by loading only the selected value, or a bounded non-reentrant
borrowed query. Never hold slot references across callbacks or allocation that can
invalidate them. Keep public descriptor materialization at observable boundaries.
Tests compare insertion order, internal metadata, delete/re-add, descriptor value
mutation, dictionary transitions and collision handling. Measure shape traversal,
unnecessary value loads and retained metadata bytes, not just cache hit counts.
