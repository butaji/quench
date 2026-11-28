//! The megamorphic read stub cache: one table for the whole VM, direct-mapped by receiver
//! shape and name, that answers a read past a site's inline entries with one probe
//! (`Vm::megamorphic_field_entry_hit`), as V8's megamorphic stub cache does. A site's own
//! hashed entries (`FieldCacheSet::overflow`) stay the authority: an entry here is a copy of
//! one that answered a read of the same name, so it states only what a shape proves (an own
//! data slot of the receiver's exact shape, or the receiver's lack of the name with a data
//! slot of its immediate prototype's exact shape), which holds at every site that reads that
//! name, and sites that read one name from many shapes (an AST walk's `node.type`) share it.
//!
//! It is cleared with the per-site entries (`Vm::invalidate_field_caches`, which shape
//! compaction and every descriptor change run, and a program reset), so no entry outlives a
//! shape id or the slot facts it was copied under.
use super::*;
use super::object::PolymorphicHit;

/// Bits of an entry's index (`stub_index`).
const FIELD_STUB_BITS: u32 = 12;
/// Entries of the table once its first entry is recorded: 4096 of 20 bytes, 80 KiB for the VM,
/// none before a read reaches it.
const FIELD_STUB_ENTRIES: usize = 1 << FIELD_STUB_BITS;

/// One entry: a read of `atom` answered by `entry`, a field cache entry of exact shapes.
#[derive(Clone, Copy)]
struct FieldStub {
    atom: Atom,
    entry: FieldCache,
}

impl FieldStub {
    /// No object's exact shape is `NO_FIELD_RECEIVER` (`EMPTY_CACHE`), so no read matches it.
    const EMPTY: Self = Self { atom: 0, entry: EMPTY_CACHE };
}

/// The profile tier a polymorphic site's hit counts in (`Profile::field_cache_hit`), which a
/// hit here, answering in its place, counts in too.
const POLYMORPHIC_HIT_TIER: usize = 2;

const _: () = assert!(std::mem::size_of::<FieldStub>() == 20);

/// The table (`Vm::field_stubs`): empty until its first entry, and `written` while any entry
/// may be recorded, so a clear of an untouched table writes nothing.
#[derive(Default)]
pub(super) struct FieldStubs {
    entries: Box<[FieldStub]>,
    written: bool,
}

/// The entry of a read of `atom` from a receiver of exact shape `shape`: the high bits of the
/// pair's Fx hash, below `FIELD_STUB_ENTRIES`.
#[inline(always)]
fn stub_index(shape: u32, atom: Atom) -> usize {
    use std::hash::Hasher;
    let mut hasher = rustc_hash::FxHasher::default();
    hasher.write_u64(u64::from(shape) << u32::BITS | u64::from(atom));
    (hasher.finish() >> (u64::BITS - FIELD_STUB_BITS)) as usize
}

impl FieldStubs {
    /// The entry recorded for a read of `atom` from a receiver of exact shape `shape`, if any.
    #[inline(always)]
    fn get(&self, shape: u32, atom: Atom) -> Option<FieldCache> {
        // An empty table has no entry at any index, so the probe needs no test of its own.
        let stub = self.entries.get(stub_index(shape, atom))?;
        (stub.entry.receiver == shape && stub.atom == atom).then_some(stub.entry)
    }

    /// Records `entry` for reads of `atom`, in place of the entry its index held.
    #[cold]
    #[inline(never)]
    fn insert(&mut self, atom: Atom, entry: FieldCache) {
        if self.entries.is_empty() {
            self.entries = vec![FieldStub::EMPTY; FIELD_STUB_ENTRIES].into_boxed_slice();
        }
        if let Some(stub) = self.entries.get_mut(stub_index(entry.receiver, atom)) {
            *stub = FieldStub { atom, entry };
            self.written = true;
        }
    }

    /// Drops every entry, keeping the table's memory.
    pub(super) fn clear(&mut self) {
        if std::mem::take(&mut self.written) {
            self.entries.fill(FieldStub::EMPTY);
        }
    }
}

impl Vm {
    /// `polymorphic_field_entry_hit` of a read of `atom` at `site` behind the stub cache: an
    /// entry recorded for the receiver's exact shape and `atom` answers first, with one probe
    /// (`field_stub_hit`); any other read takes the site's entries, inline or hashed, whose own
    /// or prototype hit is recorded for the reads that follow, this site's or another reading
    /// `atom`. A private name is never recorded: its key is the class's brand too, which a
    /// shape and an atom do not name.
    #[inline(always)]
    pub fn megamorphic_field_entry_hit(
        &mut self,
        object: Value,
        atom: Atom,
        site: u16,
    ) -> Option<(Value, PolymorphicHit)> {
        if let Some(hit) = self.field_stub_hit(object, atom) {
            return Some(hit);
        }
        let (value, hit) = self.polymorphic_field_entry_hit(object, site)?;
        if let PolymorphicHit::Own(entry) = hit
            && !self.is_private_name(atom)
        {
            self.field_stubs.insert(atom, entry);
        }
        Some((value, hit))
    }

    /// `megamorphic_field_entry_hit`'s value alone.
    #[inline(always)]
    pub fn megamorphic_field_hit(&mut self, object: Value, atom: Atom, site: u16) -> Option<Value> {
        self.megamorphic_field_entry_hit(object, atom, site).map(|(value, _)| value)
    }

    /// The value the stub cache's entry for a read of `atom` from the record object `object`
    /// proves, with that entry: its own data slot where the entry names no holder, else that of
    /// its immediate prototype, which must be a record of the entry's holder shape. `None`
    /// where no entry of the receiver's exact shape and `atom` is recorded or the prototype
    /// differs.
    #[inline(always)]
    fn field_stub_hit(&mut self, object: Value, atom: Atom) -> Option<(Value, PolymorphicHit)> {
        let receiver = self.heap.record_object_value(object)?;
        let entry = self.field_stubs.get(receiver.exact_shape(), atom)?;
        if entry.holder == NO_FIELD_HOLDER {
            // SAFETY: the entry is a copy of a site's own entry for this exact shape (`insert`),
            // which matches only outside dictionary storage and names a data slot the shape
            // holds, as for `cached_field_hit`.
            let value = unsafe { self.heap.property_get_flat(receiver, entry.slot as usize) };
            self.profile.field_cache_hit(POLYMORPHIC_HIT_TIER, 0);
            return Some((value, PolymorphicHit::Own(entry)));
        }
        // A holder entry names a record prototype, which no object has: the stub misses.
        None
    }
}
