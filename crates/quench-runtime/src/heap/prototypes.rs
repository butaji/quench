//! Interned prototype table: design for removing the per-family record-space limits.
//!
//! Status: design only. Nothing in this module is implemented yet, and the code it describes
//! still lives in `cell.rs` (`PROTO_*_RECORD_BASE`, `ProtoLink`, `Object::encode_proto`),
//! `records.rs` (`*_RECORD_CAPACITY`), `layouts.rs`, `record_layouts.rs`, `staged/object.rs`
//! (`RECORD_LINKS`), and `quench-bjit/stencils/layout.rs`.
//!
//! # Problem
//!
//! An object's prototype is a 28-bit link (`PROTO_INDEX_BITS`) stored in the `meta` word of its
//! layout entry (`heap::layouts`). The link is one flat index space carved into windows, one per
//! record family (legacy, array, object, function, promise, iterator, collection). Each window
//! is sized to its family's capacity, so every capacity is capped by how many links the family
//! can own: object and function records get 2^26, array 2^25, and promise, iterator and
//! collection 2^24. A record space can never issue an id at or past its window.
//!
//! The link is not the constraint on the record ids themselves. A direct handle keeps its index
//! in the low 32 bits of the value (`value.rs`, `LOWER_WORD`), and `RecordId` is `u32`
//! (`records.rs`). Removing the windows is therefore a matter of moving prototype identity out
//! of the link's bit space.
//!
//! # Design
//!
//! 1. Prototype table. `Heap` gets `prototypes: Vec<Value>` plus a reverse map from `Value`
//!    bits to index. The map is the only place that interns, so equal indices name the same
//!    `Value`. Two sentinel codes stay outside the table: null and undefined keep their
//!    `PROTO_NULL` and `PROTO_UNDEFINED` values, so a null prototype needs no entry.
//!
//!    `Heap::proto_link_of(proto: Value) -> ProtoLink` interns `proto` on first use.
//!    `Heap::prototype_of_link(link: ProtoLink) -> Value` is a table load. `Object` keeps the
//!    raw link, and only the callers that need the prototype object itself go through the heap.
//!
//! 2. Hot paths compare links, not values. Holder guards, chain walks, and `instanceof` checks
//!    compare `ProtoLink` indices directly wherever equality is enough, with no table load.
//!    A link is decoded to a `Value` only where the prototype object is actually needed (a
//!    property lookup on it, a `Value` result, a snapshot). Equal links name the same prototype
//!    because the interning map gives each `Value` one index.
//!
//! 3. Monotonic table, no reuse. An index is never handed out again within the process. When
//!    the collector finds a prototype's `Value` dead, its entry becomes a tombstone: the slot
//!    stays, the `Value` is cleared, and the reverse-map key is removed. A tombstone is never
//!    reused. Reuse would let a stale reference pass on a different prototype. A layout id, a
//!    code dependency (holder watch, construct plan), or a compiled guard may still hold the
//!    old index after its prototype died. If that index were reissued, the stale guard would
//!    compare equal to a new prototype and pass. Without reuse, equal indices always mean the
//!    same prototype. The cost is a `u32`-sized slot per distinct prototype ever created,
//!    bounded by the 2^28 index width. Real programs never approach that, and the table
//!    reports exhaustion as a fatal error rather than wrapping.
//!
//! 4. Windows gone. `PROTO_*_RECORD_BASE`, `PROTO_RECORD_END`, `LEGACY_SLOT_CAPACITY` as a link
//!    bound, and the per-family `*_RECORD_CAPACITY` link asserts are removed. Every record space
//!    may take the full `u32` id width, with the mmap `MAP_NORESERVE` reservation already in
//!    place. `records.rs` `next_id` keeps a guard against `u32` overflow.
//!
//! 5. JIT and native readers. `NATIVE_OBJECT_PROTO_*` in `record_layouts.rs`, the mirrored
//!    constants in `quench-bjit/stencils/layout.rs`, and `RECORD_LINKS` in `staged/object.rs`
//!    describe the old window and mask arithmetic. Where compiled code decodes a link into a
//!    record handle by base arithmetic, it becomes a table load through a table base pointer,
//!    passed the same way the other heap table bases are passed (`JS_STEP_*_RECORD_BASE`). A
//!    path that cannot take that change takes the slow exit.
//!
//! 6. Snapshots. A prototype link is written as a `Value`, or the table is serialized in full.
//!    Either way `FORMAT_VERSION` is bumped, since an old snapshot's link indices mean nothing
//!    under the new table.
//! 7. The auxiliary word is unchanged. Only the `meta` proto bits change meaning. Bit 25 of the
//!    auxiliary word is `CONSTANT_FORM`, and shapes must stay below `1 << 25`, so the
//!    `auxiliary` layout of `heap::layouts` keeps its current encoding.
//!
//! # Open decision: the layout table is process-wide
//!
//! `heap::layouts` is one process-wide, append-only table, and an entry stores the `meta` word
//! with the prototype link inside it. Two heaps share an entry whenever their headers are equal.
//! A prototype table owned by `Heap` breaks that sharing: the same index names different
//! prototypes in different heaps, so one layout entry cannot be valid for both. Three options:
//!
//! - A. Make the prototype table process-wide too. Every heap then marks and reclaims entries
//!   through shared state, which has to be coordinated across threads (worker isolates).
//! - B. Move the prototype link back out of the interned layout into each record. This costs
//!   four bytes per object (`Object` grows from 12 to 16 bytes under `packed(4)`) and keeps the
//!   per-heap table. It also drops the per-object dedupe the layout table gives.
//! - C. Key the layout by heap, which gives up most of the sharing the table exists for.
//!
//! Option A or B has to be chosen before the rest of the design can be implemented. Options
//! 1 through 6 above all assume one of them.
//!
//! # Risky sites
//!
//! - `heap/layouts.rs`: the `meta` word is written at intern time, so a prototype index must
//!   be stable before the layout is interned.
//! - `cell.rs`: `Object::encode_proto` and `set_proto`. About 66 files call `.proto()`,
//!   `set_proto(` or `encode_proto(`, so the signature change is wide.
//! - `staged/object.rs` (`proto_names_record`, `proto_record_id`): these are used by the
//!   instanceof and prototype-walk stages, so they change with the table.
//! - `quench-bjit/stencils/*_rows.rs`: `JS_STEP_OBJECT_RECORD_BASE` is read by several rows,
//!   and a table base must be added to the step payload if the table is kept in a new place.
//! - `snapshot.rs`: links are persisted, so the format version must change.
//! - The `stencil_layout_matches` test in `quench-bjit` checks the mirrored constants and must
//!   be updated in the same change.
