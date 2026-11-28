//! The entry words and backing an IC entry is read through, and the test-only concrete read.

use super::*;

/// A nested lookup's chain of two reads, `object.first.second` (`FieldLookup::Site`): the value
/// the first entry reads is the object the second reads from, and the tables are proved once.
#[inline(always)]
pub(crate) fn entry_word<S: Stage>(stage: &mut S, entry: S::Word, offset: S::Imm) -> S::Word {
    stage.load_site(Region::LoopEntry.readonly(), entry, offset)
}

/// A store of `value` through the cache entry `offset` bytes into the field cache table `entries`
/// of the object `object` names in the record table at `records`: where the entry is an own entry
/// for the record's exact shape, its values are plain and the store gate admits the write, the
/// value goes to the entry's slot (`site_store`). `None` (leaving at `miss`) otherwise, or where
/// either table is absent or the word is no direct object, nothing written. Whether the program
/// may store in place at all (a specialized run, no method cache naming the field) is the
/// caller's.
#[inline(always)]
pub(crate) fn entry_backing<S: Stage>(stage: &mut S, entry: S::Word, offset: S::Imm) -> (S::Word, S::Word) {
    use crate::heap::NATIVE_ARRAY_LAYOUT as ARRAY;
    let address = stage.add_imm(entry, offset);
    let values = stage.load_table(Region::LoopEntry.readonly(), address);
    let length = stage.load_u32(Region::Elements.mutable(), values, ARRAY.header_length_offset as i32 - ARRAY.header_values_offset as i32);
    // The entry proved a wide backing; one the VM's store has since turned double in place
    // (`ArrayElements::replace_in_place`) flags its length word (`ENCODED_LEN`), and its words
    // are no `Value`s: the view then reads as empty, so every access leaves at its bounds test.
    let encoded = stage.ugt_imm(length, u64::from(!crate::heap::ENCODED_LEN));
    let empty = stage.constant(0);
    (values, stage.select(encoded, empty, length))
}

/// The interpreter's `site_read` of `object` through the stored field cache entry `entry` (at
/// byte 0 of its table), over the heap's own tables.
#[cfg(test)]
pub(super) fn concrete_read_entry(heap: &crate::heap::Heap, object: crate::Value, entry: &[u32; 4]) -> Option<crate::Value> {
    let records = heap.object_record_base()? as u64;
    // SAFETY: a stored entry, and the heap's live record table and arena.
    unsafe { super::super::ic::concrete_site_read(heap, (entry.as_ptr() as u64, records), object, 0) }
}
