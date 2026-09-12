# 150 — Cached shape-transition edges

Status: complete

The accepted Task 142 Earley-Boyer sample in
`reports/task147-earley-string-kernel.sample.txt` still shows SipHash, `intern_shape`,
full `Vec<String>` hashing, and property-key cloning in the hottest native stacks.
`PropertyStorage::insert` currently copies every key in the receiver's layout and hashes
the complete vector whenever an object gains a property, even when thousands of objects
repeat the same construction sequence.

Make the hidden-class graph explicit. Cache additions and removals by
`(parent ShapeId, property name) -> child ShapeRef`. Preserve the existing full-layout
interner as the canonical source of shape identity, but consult it only the first time a
transition edge is discovered. Later objects follow the immutable edge without cloning
or hashing the entire layout. Make `PropertyStorage::insert` borrow `&str`, so an existing
slot or cached transition allocates no temporary key.

This is a general object-layout algorithm and does not depend on a benchmark, source
location, property spelling, or hotness. Shapes are immutable shared kernels; object
storage contains only a shape reference plus mutable values. The transition graph is
derived from semantic property-add/remove events.

Acceptance: tests prove repeated construction shares shape pointers, deletion and
re-addition return canonical layouts, property order remains correct, and existing IC
invalidation behavior holds; all release tests and V8v7 smoke pass; alternating A/B
improves aggregate without crossing component floors.

## Result

Accepted. `ShapeRegistry` now retains the canonical full-layout interner but adds direct
addition and removal edge maps indexed first by parent `ShapeId`, then by borrowed
property name. `PropertyStorage::insert` accepts `&str`; existing slots and established
transitions allocate no key and never construct or hash a complete layout. The root
shape is also retained directly instead of repeatedly probing the empty layout.

All 77 release tests and the complete eight-suite smoke pass. The existing shape test
proves root sharing, repeated addition sharing, independent values, deletion, and return
to the canonical prior layout. The first four-run comparison improved aggregate 9.89%.
The six-run, 200 ms confirmation in
`reports/task150-shape-transitions-full-ab-6/comparison.txt` improves aggregate from
1601.31 to **1714.39 (+7.06%)**. RayTrace improves 21.12%, Earley-Boyer 12.97%, Splay
17.60%, and every component remains above the standing -5% floor.

Accepted binary: `/tmp/deegen-task150-shape-transitions`, SHA-256
`863b37fa0753e496d57d74c9c8c34991d21b7909db0ba5008c52b1066fffc682`.

## Refinement candidate, extracted directly from JSC source

Fetched `Source/JavaScriptCore/runtime/StructureTransitionTable.h` directly
(`https://raw.githubusercontent.com/WebKit/WebKit/main/...`) rather than reasoning from
documentation. JSC's own transition table does not allocate a hashmap per shape node
unconditionally the way `additions`/`removals: HashMap<ShapeId, HashMap<String,
ShapeRef>>` does here. It stores one tagged word, `m_data`, that holds *either* a single
`Structure*` directly (tagged via `UsingSingleSlotFlag` in the low bit,
`StructureTransitionTable.h:155,287-303`) for the overwhelmingly common case of a shape
with exactly one child transition, *or* a pointer to a full `WeakGCMap` only once a
shape actually branches into two or more children (`isUsingSingleSlot()`/`setMap()`
switch representation on the first branch). This avoids a HashMap allocation entirely
for every shape node on a purely linear construction chain — the common case for object
literals and constructors that add the same fields in the same order every time — and
only pays the hashmap cost at genuine branch points in the shape tree.

This project's current `additions`/`removals` maps allocate an inner `HashMap<String,
ShapeRef>` per parent `ShapeId` unconditionally, even for shapes with exactly one
observed child. Given [[150]]'s own accepted measurement shows this edge-cache design
already delivered a real win (+7.06% aggregate), this refinement is additive — replacing
the unconditional per-node `HashMap<String, ShapeRef>` with a single/multi tagged
representation (a `SmallVec`-style inline single-entry fast path, or an actual tagged-
pointer scheme matching JSC's) should reduce allocation further for the common linear-
chain case without touching the edge-caching idea's own correctness. This should be a
follow-up experiment, not a reason to reopen [[150]]'s already-accepted status: measure
the additional allocation reduction and gate it with its own A/B before adopting.

