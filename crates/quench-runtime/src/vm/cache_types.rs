//! The field, accessor, add and method cache entries a site keeps, and the sets that hold them.

use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct FieldCache {
    pub(super) receiver: u32,
    pub(super) holder: u32,
    pub(super) slot: u16,
}

impl FieldCache {
    /// The entry of an own data field at `slot` of receivers of exact shape `receiver`.
    #[inline(always)]
    pub(super) const fn own(receiver: u32, slot: u16) -> Self {
        Self { receiver, holder: NO_FIELD_HOLDER, slot }
    }
}
/// The internal keys of a bound function's environment object.
pub(super) const BOUND_TARGET_KEY: &str = "\0quench:bound-target";
pub(super) const BOUND_THIS_KEY: &str = "\0quench:bound-this";
pub(super) const BOUND_ARGS_KEY: &str = "\0quench:bound-args";
pub(super) const NO_FIELD_HOLDER: u32 = u32::MAX;
/// Where an accessor field cache's holder keeps its getter (`AccessorFieldCache::slot_accessor`):
/// in the slot itself (`ShapeAttributes::slot_getter`), in the slot's pair cell
/// (`slot_accessor_pair`), or where its attributes say (the shape's accessor table).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SlotAccessor {
    Getter,
    Pair,
    Attributes,
}
/// A field site's read of an accessor of a shaped receiver, its own or on a prototype: it
/// holds while the receiver keeps its shape and prototype and the chain to `holder` keeps the
/// layout it was proved under, `holder`'s `slot` attributes carrying the getter.
#[derive(Clone, Copy)]
pub(super) struct AccessorFieldCache {
    pub(super) receiver: u32,
    /// The receiver's layout id (`Object::layout_id`): its shape and prototype in one word,
    /// which an ordinary receiver's hit compares instead of each.
    pub(super) layout: u32,
    /// The receiver's prototype, where the chain to `holder` starts.
    pub(super) prototype: Value,
    /// The prototype holding the accessor, or `OWN_ACCESSOR` for the receiver itself, whose
    /// own slot each receiver of the shape fills with its own accessor.
    pub(super) holder: Value,
    pub(super) slot: u16,
    /// Where the holder's shape keeps the accessor at `slot` (`SlotAccessor`): the shape the
    /// entry proves (the receiver's for `OWN_ACCESSOR`, else the prototype holder's, which the
    /// epoch covers) fixes it.
    pub(super) slot_accessor: SlotAccessor,
    /// The `prototype_layout_epoch` the chain was proved under.
    pub(super) epoch: u64,
    /// The site's next older entry in `Vm::accessor_fields`, or `NO_ACCESSOR_FIELD`.
    pub(super) next: u32,
    /// The getter whose call a field read of the receiver answers (`answer`), or undefined:
    /// `Vm::memoized_getter_answer` reads it in place of the call.
    pub(super) getter: Value,
    /// That read's field entry, which proves the receiver's shape itself.
    pub(super) answer: FieldCache,
}
/// A cache site without an accessor entry (`Vm::accessor_field_indices`).
pub(super) const NO_ACCESSOR_FIELD: u32 = object::MRU_CHAIN_END;
/// Receiver shapes one site's inherited entries remember: a method inherited from beyond the
/// immediate prototype read on instances of several subclasses (DeltaBlue's constraints)
/// reaches one site with a shape per class. A further shape reuses the oldest entry.
pub(super) const INHERITED_FIELD_CACHE_WAYS: usize = 4;
// Reusing the oldest entry unlinks it from an older-than-head predecessor.
pub(super) const _: () = assert!(INHERITED_FIELD_CACHE_WAYS > 1);
/// Receiver shapes one site's accessor entries remember (`Vm::record_accessor_entry`). A
/// getter on a prototype is read from instances that differ by which fields their factory
/// set, as Air's `Arg` kinds do (about ten shapes reading `isStack`); a site past its ways
/// leaves the lane at each miss and evicts an entry another shape needs next.
pub(super) const ACCESSOR_FIELD_CACHE_WAYS: usize = 16;
use crate::heap::{INHERITED_FIELD_CACHE_MAX_DEPTH, InheritedFieldAnswer, InheritedFieldCache};
pub(super) const NO_FIELD_RECEIVER: u32 = u32::MAX;
/// A cache site without an inherited entry (`Vm::inherited_field_indices`).
pub(super) const NO_INHERITED_FIELD: u32 = object::MRU_CHAIN_END;
// A cached add proves every prototype up to this depth. Node's stream objects (an
// `IncomingMessage` above `Readable`, `Stream`, `EventEmitter` and `Object.prototype`) add
// their fields five prototypes below the chain's end; deeper chains stay uncached. Every
// entry carries this many shapes, and recorded adds proved at most four (V8-v7, npm
// TypeScript and ESLint census, `field_add_cache_depth_counts`, 2026-10-23), so eight keeps
// the streams' five with room and halves the sixteen an inherited read keeps.
pub(super) const FIELD_ADD_CACHE_MAX_PROTO_DEPTH: usize = 8;
/// Bits of a keyed add's index into `Vm::keyed_adds`.
pub(super) const KEYED_ADD_INDEX_BITS: u32 = 8;
/// Keyed adds the VM remembers, direct-mapped by source shape and key: express's JSON body
/// path makes fifteen a request into header and parameter objects, whose shapes repeat.
pub(super) const KEYED_ADD_ENTRIES: usize = 1 << KEYED_ADD_INDEX_BITS;
/// Source shapes one add site remembers; the oldest is evicted beyond this. Objects whose
/// optional fields are assigned in varying order reach one add site with several shapes.
pub(super) const FIELD_ADD_CACHE_WAYS: usize = 4;
pub(super) const EMPTY_CACHE: FieldCache = FieldCache {
    receiver: NO_FIELD_RECEIVER,
    holder: NO_FIELD_HOLDER,
    slot: 0,
};
/// Stored as `[receiver, holder, slot]` complemented against `EMPTY_CACHE`, which stores
/// as zero words.
impl site_table::SiteEntry for FieldCache {
    type Stored = [u32; 3];
    #[inline(always)]
    fn store(self) -> [u32; 3] {
        [
            self.receiver ^ EMPTY_CACHE.receiver,
            self.holder ^ EMPTY_CACHE.holder,
            u32::from(self.slot ^ EMPTY_CACHE.slot),
        ]
    }
    #[inline(always)]
    fn load([receiver, holder, slot]: [u32; 3]) -> Self {
        Self {
            receiver: receiver ^ EMPTY_CACHE.receiver,
            holder: holder ^ EMPTY_CACHE.holder,
            slot: slot as u16 ^ EMPTY_CACHE.slot,
        }
    }
}
/// Layout facts for generated code that reads a field cache entry as `SiteEntry for
/// FieldCache` stores it: three words, each complemented against `EMPTY_CACHE`.
pub(crate) struct NativeFieldCacheLayout {
    pub(crate) entry_size: usize,
    pub(crate) receiver_offset: usize,
    pub(crate) holder_offset: usize,
    pub(crate) slot_offset: usize,
    pub(crate) receiver_complement: u32,
    pub(crate) holder_complement: u32,
    pub(crate) slot_complement: u32,
    /// The holder an own data field's entry names.
    pub(crate) own_holder: u32,
}

pub(crate) const NATIVE_FIELD_CACHE_LAYOUT: NativeFieldCacheLayout = NativeFieldCacheLayout {
    entry_size: std::mem::size_of::<<FieldCache as site_table::SiteEntry>::Stored>(),
    receiver_offset: 0,
    holder_offset: std::mem::size_of::<u32>(),
    slot_offset: 2 * std::mem::size_of::<u32>(),
    receiver_complement: EMPTY_CACHE.receiver,
    holder_complement: EMPTY_CACHE.holder,
    slot_complement: EMPTY_CACHE.slot as u32,
    own_holder: NO_FIELD_HOLDER,
};

/// One cached property-add transition: an extensible ordinary object of `source_shape` whose
/// prototype chain still has exactly `prototype_shapes[..prototype_depth]` gains `atom` at `slot`
/// and moves to `target_shape`.
#[derive(Clone, Copy)]
pub(super) struct FieldAddCache {
    pub(super) atom: Atom,
    pub(super) source_shape: u32,
    pub(super) target_shape: u32,
    pub(super) slot: u16,
    /// The layouts of the receiver that recorded the add and of the record it moves to
    /// (`Transition::layouts`), which the staged rows guard and store.
    pub(super) layouts: [u32; 2],
    pub(super) prototype_depth: u8,
    pub(super) prototype_shapes: [u32; FIELD_ADD_CACHE_MAX_PROTO_DEPTH],
    /// The `prototype_layout_epoch` under which a walk last proved the chain from
    /// `proved_prototype`, or `NO_PROVED_EPOCH`; while both hold the chain needs no walk.
    pub(super) proved_epoch: u64,
    /// The receiver's prototype link the proof holds for, compared undecoded; a collection
    /// advances the epoch, so it never outlives the record it names.
    pub(super) proved_prototype: ProtoLink,
}
/// A `FieldAddCache::proved_epoch` no epoch reaches: the chain is unproven.
pub(super) const NO_PROVED_EPOCH: u64 = u64::MAX;
impl FieldAddCache {
    pub(super) const EMPTY: Self = Self {
        atom: u32::MAX,
        source_shape: NO_FIELD_RECEIVER,
        target_shape: NO_FIELD_RECEIVER,
        slot: 0,
        layouts: [0; 2],
        prototype_depth: 0,
        prototype_shapes: [NO_FIELD_RECEIVER; FIELD_ADD_CACHE_MAX_PROTO_DEPTH],
        proved_epoch: NO_PROVED_EPOCH,
        proved_prototype: ProtoLink::NULL,
    };
    pub(super) fn prototype_shapes(&self) -> &[u32] {
        &self.prototype_shapes[..usize::from(self.prototype_depth)]
    }
    /// The shape transition the entry caches, as the staged add row takes it.
    #[inline(always)]
    pub(super) fn transition(&self) -> Transition {
        Transition { before: self.source_shape, after: self.target_shape, slot: self.slot, layouts: self.layouts }
    }
    /// Whether the chain proof (`proved_epoch`, `proved_prototype`) holds for a receiver
    /// whose prototype `link` names: then the chain needs no walk.
    #[inline(always)]
    pub(super) fn proof_holds(&self, link: ProtoLink) -> bool {
        self.proved_epoch == crate::heap::prototype_layout_epoch() && self.proved_prototype == link
    }
}
/// The add transitions one site remembers, oldest first. The first lives in the set and the
/// rest, up to `FIELD_ADD_CACHE_WAYS`, in a box made when a second arrives: a site is nearly
/// always monomorphic (1413 of 1426 sets in npm TypeScript, every set but one in V8-v7, census
/// 2026-10-23), and four inline ways cost each site the room of four entries.
pub(super) struct FieldAddCacheSet {
    pub(super) len: u8,
    pub(super) first: FieldAddCache,
    pub(super) rest: Option<Box<[FieldAddCache; FIELD_ADD_CACHE_WAYS - 1]>>,
}
impl FieldAddCacheSet {
    pub(super) const EMPTY: Self = Self {
        len: 0,
        first: FieldAddCache::EMPTY,
        rest: None,
    };
    /// The entries after the first, as many as the set holds.
    #[inline(always)]
    pub(super) fn rest(&self) -> &[FieldAddCache] {
        match &self.rest {
            Some(rest) => &rest[..usize::from(self.len).saturating_sub(1)],
            None => &[],
        }
    }
    /// Every entry, oldest first.
    pub(super) fn entries(&self) -> impl Iterator<Item = &FieldAddCache> {
        let first = (self.len > 0).then_some(&self.first);
        first.into_iter().chain(self.rest())
    }
    #[inline(always)]
    pub(super) fn find(&self, source_shape: u32, atom: Atom) -> Option<&FieldAddCache> {
        let matches = |cache: &&FieldAddCache| cache.source_shape == source_shape && cache.atom == atom;
        if self.len > 0 && matches(&&self.first) {
            return Some(&self.first);
        }
        self.rest().iter().find(matches)
    }
    /// `find`, preferring the entry whose chain proof holds for a receiver whose prototype
    /// `link` names: receivers of one shape under different prototypes (subclasses whose
    /// constructors share their parent's stores) each keep an entry of their chain.
    #[inline(always)]
    pub(super) fn find_for(&self, source_shape: u32, atom: Atom, link: ProtoLink) -> Option<&FieldAddCache> {
        let first = self.find(source_shape, atom)?;
        if first.proof_holds(link) {
            return Some(first);
        }
        let proved = |cache: &&FieldAddCache| {
            cache.source_shape == source_shape && cache.atom == atom && cache.proof_holds(link)
        };
        Some(self.rest().iter().find(proved).unwrap_or(first))
    }
    /// The indexes of the entries for receivers of `source_shape` adding `atom`, one per
    /// prototype chain the site recorded.
    pub(super) fn matching(&self, source_shape: u32, atom: Atom) -> impl Iterator<Item = usize> + '_ {
        self.entries()
            .enumerate()
            .filter(move |(_, cache)| cache.source_shape == source_shape && cache.atom == atom)
            .map(|(index, _)| index)
    }
    pub(super) fn entry(&self, index: usize) -> &FieldAddCache {
        if index == 0 { &self.first } else { &self.rest()[index - 1] }
    }
    pub(super) fn entry_mut(&mut self, index: usize) -> &mut FieldAddCache {
        if index == 0 {
            &mut self.first
        } else {
            &mut self.rest.as_deref_mut().expect("a later entry lives in the box")[index - 1]
        }
    }
    /// Replaces the entry for the same source shape, atom and prototype chain shapes, else
    /// appends, evicting the oldest entry when the set is full.
    pub(super) fn insert(&mut self, cache: FieldAddCache) {
        let mut entries = [FieldAddCache::EMPTY; FIELD_ADD_CACHE_WAYS];
        let len = usize::from(self.len);
        for (slot, entry) in entries.iter_mut().zip(self.entries()) {
            *slot = *entry;
        }
        let kept = match entries[..len]
            .iter()
            .position(|entry| {
                entry.source_shape == cache.source_shape
                    && entry.atom == cache.atom
                    && entry.prototype_shapes() == cache.prototype_shapes()
            })
        {
            Some(index) => {
                entries.copy_within(index + 1..len, index);
                len - 1
            }
            None if len == FIELD_ADD_CACHE_WAYS => {
                entries.copy_within(1..len, 0);
                len - 1
            }
            None => len,
        };
        entries[kept] = cache;
        let len = kept + 1;
        self.first = entries[0];
        if len > 1 {
            let rest = self.rest.get_or_insert_with(|| Box::new([FieldAddCache::EMPTY; FIELD_ADD_CACHE_WAYS - 1]));
            rest[..len - 1].copy_from_slice(&entries[1..len]);
        }
        self.len = len as u8;
    }
}
pub(super) const NO_FIELD_ADD_SET: u32 = u32::MAX;
pub(super) const NO_MEGAMORPHIC_FIELD: u32 = u32::MAX;
/// `object_shapes` entry of an object literal site that has recorded no shape.
pub(super) const NO_OBJECT_SHAPE: u32 = u32::MAX;
/// The share of all cache sites, as a divisor, up to which an invalidation resets the
/// written sites one by one: each costs four scattered stores against a sequential fill of
/// every site's entries (Node's library wires prototypes hundreds of times at load, each an
/// invalidation that had refilled every table).
pub(super) const FIELD_CACHE_SPARSE_RESET_SHARE: usize = 8;
// Bound each site's overflow to 256 recorded shapes; unseen shapes keep the generic fallback.
pub(super) const FIELD_MEGAMORPHIC_LIMIT: usize = 256;
pub(super) struct FieldCacheSet {
    pub(super) len: u8,
    /// Whether any entry recorded an own field (`NO_FIELD_HOLDER`); a set without one names
    /// only prototype holders, so a read probes no own entry.
    pub(super) holds_own: bool,
    pub(super) entries: [FieldCache; compiled::tier_policy::POLYMORPHISM],
    pub(super) overflow: Option<Box<FxHashMap<u64, FieldCache>>>,
}
#[derive(Clone, Copy)]
pub(super) struct MethodCache {
    pub(super) shape: u32,
    pub(super) atom: Atom,
    pub(super) proto: Value,
    pub(super) callee: Value,
    pub(super) guard: MethodGuard,
    pub(super) target: Option<CallTarget>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct MethodGuard {
    pub(super) owner: Value,
    pub(super) owner_shape: u32,
    pub(super) slot: u16,
    pub(super) depth: u16,
}
pub(super) struct MethodCacheSet {
    pub(super) site: usize,
    pub(super) len: u8,
    pub(super) entries: [MethodCache; compiled::tier_policy::POLYMORPHISM],
}
