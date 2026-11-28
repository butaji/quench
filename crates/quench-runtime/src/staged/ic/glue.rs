//! The interpreter's and host's inline-cache reads through `Concrete` over a heap: the
//! `concrete_*` wrappers of `ic.rs`, which name the runtime `Heap` and `Object`.

#![allow(unused_imports)]
use super::*;

/// The own field a site's hint proved: the value at `slot` of `object` where its exact shape
/// is `shape`, else `None` (the interpreter's `GetField` hit: `GuardShape` then `LoadSlot`, the
/// ops `run` interprets, through `Concrete`), through the arena of `heap`.
///
/// # Safety
/// `object` is a live record of `heap`, and its run lies in `heap`'s arena; a hint's slot is one its shape covers.
#[inline(always)]
pub unsafe fn concrete_own_field_hit(object: &crate::heap::Object, heap: &crate::heap::Heap,
    shape: u32, slot: u16) -> Option<crate::Value> {
    let mut stage = crate::staged::Concrete::over(heap);
    let record = object as *const crate::heap::Object as u64;
    own_field(&mut stage, record, shape, slot, ()).map(crate::value_vec::decode_staged_word)
}

/// `concrete_own_field_hit` of the global object, whose values the native tiers' planning moves behind
/// extras (`global_values_table`): the exact word's flat run on one compare, as there, and the extras'
/// run on a second (`proved_own_layout`) instead of the formed walk of `own_field_miss`, which
/// any other word still takes.
/// # Safety
/// As for `concrete_own_field_hit`.
#[inline(always)]
pub unsafe fn concrete_global_field_hit(object: &crate::heap::Object, heap: &crate::heap::Heap,
    shape: u32, slot: u16) -> Option<crate::Value> {
    let mut stage = crate::staged::Concrete::over(heap);
    let record = object as *const crate::heap::Object as u64;
    let word = crate::staged::object::header_u32(&mut stage, Region::ShapeWord.mutable(), record, OBJECT.auxiliary_offset);
    // Each arm's form is a constant, so its layout tests no bit.
    let layout = if word == u64::from(shape) {
        crate::staged::object::proved_own_layout(&mut stage, crate::staged::Shaped::of_guard(record), 0, usize::from(slot) + 1)
    } else if word == u64::from(shape | OBJECT.extras_form) {
        crate::staged::object::proved_own_layout(&mut stage, crate::staged::Shaped::of_guard(record), OBJECT.extras_form, usize::from(slot) + 1)
    } else {
        // Any other form takes the general hit's formed walk.
        return unsafe { concrete_own_field_hit(object, heap, shape, slot) };
    }?;
    Some(crate::value_vec::decode_staged_word(read_slot(&mut stage, layout, slot, Representation::Tagged)))
}

/// `concrete_own_field_hit` through a lane record's own-field hint, whose `hint` is a layout
/// id (`Object::hint_shape`): one compare of the record's own word proves the exact shape and
/// the storage form, which the id's entry then tells.
///
/// A fixed `FORM` is the form of every hint a record of it holds: its handler is chosen from
/// the hint it is installed with (`own_formed_handler`), and a later hint either re-chooses it
/// or names no object's layout (a cleared hint), so equality proves `FORM` too.
///
/// # Safety
/// As for `concrete_own_field_hit`; for a fixed `FORM`, a hint some object's layout equals
/// names `FORM`.
#[inline(always)]
pub unsafe fn concrete_hinted_own_field_hit<const FORM: u32>(object: &crate::heap::Object,
    heap: &crate::heap::Heap, hint: u32, slot: u16) -> Option<crate::Value> {
    let mut stage = crate::staged::Concrete::over(heap);
    let record = object as *const crate::heap::Object as u64;
    let word = layout_id(&mut stage, record);
    // A fixed form's hint names it (the caller's contract), so equality proves that form
    // and its layout is read without testing the bits; no narrow run's layout matches it.
    // The same row a specializer runs (`IcOp::GuardLayout`, then the proved form's layout):
    // one compare proves shape and form, and the layout tests no form bit.
    let at = Reached { word: Some(word), ..Reached::bare(record) };
    let shaped = guard_layout(&mut stage, &at, hint, ())?;
    debug_assert!(FORM == HINTED_FORM || layout_form(hint) == FORM, "a fixed form's matched hint names it");
    let bits = if FORM == HINTED_FORM { layout_form(hint) } else { FORM };
    let layout = slot_layout(&mut stage, &Reached { formed: Some(bits), shaped: Some(shaped), ..at }, form_of_layout(hint), usize::from(slot) + 1, ())?;
    let word = read_slot(&mut stage, layout, slot, Representation::Tagged);
    Some(crate::Value::from_bits(word))
}

/// The own data value the field cache entry of the site `offset` bytes into `entries` (the
/// running program's table, `Vm::active_field_entries`) names in `object`, which the object
/// record table at `records` holds: the interpreter's primary read of a site, `site_own_read`
/// through `Concrete`, the own arm of the row the stencil build compiles from `site_read`. `None`
/// where it misses, or the entry names a prototype holder.
///
/// # Safety
/// `entries` is the table of the running program and `offset` a site's byte offset in it,
/// `records` the object record reservation of `heap`, whose arena holds the record's run; none outlives the call.
#[inline(always)]
pub unsafe fn concrete_site_own_read(heap: &crate::heap::Heap, (entries, records): (u64, u64), object: crate::Value, offset: u64) -> Option<crate::Value> {
    let mut stage = crate::staged::Concrete::over(heap);
    crate::staged::entry::site_own_read(&mut stage, (entries, records), object.to_bits(), offset, ()).map(crate::value_vec::decode_staged_word)
}

/// `site_read` through `Concrete`: own and prototype-held entries alike.
///
/// # Safety
/// As `concrete_site_own_read`.
#[cfg(test)]
pub unsafe fn concrete_site_read(heap: &crate::heap::Heap, (entries, records): (u64, u64), object: crate::Value, offset: u64) -> Option<crate::Value> {
    let mut stage = crate::staged::Concrete::over(heap);
    crate::staged::entry::site_read(&mut stage, (entries, records, 0), object.to_bits(), offset, ()).map(crate::value_vec::decode_staged_word)
}

/// The value `object.first.second` reads through the field cache entries `first` and `second`
/// (byte offsets into `entries`): `site_own_chain` through `Concrete`, one proof of the tables for
/// both reads. `None` where either misses.
///
/// # Safety
/// As `concrete_site_own_read`, for both offsets.
#[inline(always)]
pub unsafe fn concrete_site_own_chain(heap: &crate::heap::Heap, tables: (u64, u64), object: crate::Value, offsets: (u64, u64)) -> Option<crate::Value> {
    let mut stage = crate::staged::Concrete::over(heap);
    crate::staged::entry::site_own_chain(&mut stage, tables, object.to_bits(), offsets, ()).map(crate::value_vec::decode_staged_word)
}

/// The store of `value` into the own data field the site's entry `offset` bytes into `entries`
/// names in `object`: `site_store` through `Concrete` over `heap`, which admits it where the
/// write barrier needs no log. False changes nothing.
///
/// # Safety
/// As `concrete_site_read`, with `heap` the heap whose tables `entries` and `records` are.
#[inline(always)]
#[cfg(not(stencil_build))]
pub unsafe fn concrete_site_store(heap: &crate::heap::Heap, (entries, records): (u64, u64), object: crate::Value, offset: u64, value: crate::Value) -> bool {
    let mut stage = crate::staged::Concrete::over(heap);
    crate::staged::entry::site_store(&mut stage, (entries, records), object.to_bits(), offset, (value.to_bits(), true, None), ()).is_some()
}
