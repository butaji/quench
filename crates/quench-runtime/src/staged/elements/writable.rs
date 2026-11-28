//! Writable and resizable backings, and the length of an array.

use super::*;

/// `flagged_backing` and `writable_backing` in one: the array's payload word is proved plain (no
/// extras) first, so it is the backing's header with no choice between the two words
/// (`array_header`), and the plain test is made once. The length word keeps its form's flags.
#[inline(always)]
pub(super) fn writable_array_backing<S: Stage>(stage: &mut S, record_base: S::Word, base: S::Word, miss: S::Miss) -> Option<Backing<S::Word>> {
    use crate::heap::{NATIVE_ARRAY_LAYOUT as ARRAY, NATIVE_OBJECT_LAYOUT as OBJECT};
    let proved = super::super::proof::ArrayRecord::guard(stage, record_base, base, miss)?;
    let (record, header) = (proved.record(), proved.plain_header(stage, miss)?);
    // The length word keeps its flags: `prove_view` reads the backing's form from them.
    let length = stage.load_u32(Region::Elements.mutable(), header, ARRAY.header_length_offset as i32);
    let owners = stage.load_u32(Region::Elements.mutable(), header, ARRAY.header_owners_offset as i32);
    let unique = stage.eq_imm(owners, 1u32);
    stage.guard(unique, miss)?;
    let object = stage.add_imm(record, ARRAY.object_offset as u64);
    let meta = super::super::object::header_u32(stage, Region::Records.mutable(), object, OBJECT.meta_offset);
    let held = stage.and_imm(meta, u64::from(ARRAY.frozen_mask));
    let clear = stage.eq_imm(held, 0u32);
    stage.guard(clear, miss)?;
    let values = stage.add_imm(header, ARRAY.header_values_offset as u64);
    Some(Backing { record, header, values, length })
}

/// `backing` as an in-place replacement needs it: its elements owned by this array alone, the
/// array not frozen and without extras (indexed descriptors, an arguments map, sparse
/// elements or a prototype watch), as `Vm::try_replace_record_element` requires. `None`
/// leaves at `miss`.
#[inline(always)]
pub fn writable_backing<S: Stage>(stage: &mut S, backing: Backing<S::Word>, miss: S::Miss) -> Option<()> {
    use crate::heap::NATIVE_ARRAY_LAYOUT as ARRAY;
    unbarred_backing(stage, backing, ARRAY.frozen_mask, miss)
}

/// `writable_backing` of an array that is also extensible, which a change of its length
/// (`push`, `pop`) needs: `Object::holds_ordinary_elements`' integrity bits are all clear.
#[inline(always)]
pub fn resizable_backing<S: Stage>(stage: &mut S, backing: Backing<S::Word>, miss: S::Miss) -> Option<()> {
    use crate::heap::NATIVE_ARRAY_LAYOUT as ARRAY;
    unbarred_backing(stage, backing, ARRAY.integrity_mask, miss)
}

/// `backing` unshared, plain, and with none of the meta bits `barred`.
#[inline(always)]
fn unbarred_backing<S: Stage>(stage: &mut S, backing: Backing<S::Word>, barred: u32, miss: S::Miss) -> Option<()> {
    use crate::heap::{NATIVE_ARRAY_LAYOUT as ARRAY, NATIVE_OBJECT_LAYOUT as OBJECT};
    let owners = stage.load_u32(Region::Elements.mutable(), backing.header, ARRAY.header_owners_offset as i32);
    let unique = stage.eq_imm(owners, 1u32);
    stage.guard(unique, miss)?;
    let object = stage.add_imm(backing.record, ARRAY.object_offset as u64);
    let meta = super::super::object::header_u32(stage, Region::ShapeWord.mutable(), object, OBJECT.meta_offset);
    let held = stage.and_imm(meta, u64::from(barred));
    let clear = stage.eq_imm(held, 0u32);
    stage.guard(clear, miss)?;
    // Without extras, this word is the backing's header (`array_header`).
    let payload = stage.load_u64(Region::Payload.mutable(), backing.record, (ARRAY.object_offset + OBJECT.payload_offset) as i32);
    let plain = super::super::object::plain_payload(stage, payload);
    stage.guard(plain, miss)
}

/// The `length` of the direct array `base` as the int32 word it reads as: its dense backing's
/// count, of any form, which an array without extras (an arguments object, sparse elements) has
/// as its length. `None` (leaving at `miss`) for any other value, an array with extras, or a
/// length past int32.
#[inline(always)]
pub fn array_length<S: Stage>(stage: &mut S, record_base: S::Word, base: S::Word, miss: S::Miss) -> Option<S::Word> {
    array_length_as(stage, record_base, base, miss, Region::Elements.mutable())
}

/// `array_length` reading the length word with `length`.
pub fn array_length_as<S: Stage>(stage: &mut S, record_base: S::Word, base: S::Word, miss: S::Miss, length_access: Access) -> Option<S::Word> {
    use crate::heap::NATIVE_ARRAY_LAYOUT as ARRAY;
    // Only an array without extras takes this read (`ArrayRecord::plain_header`).
    let header = super::super::proof::ArrayRecord::guard(stage, record_base, base, miss)?.plain_header(stage, miss)?;
    let length = stage.load_u32(length_access, header, ARRAY.header_length_offset as i32);
    // A narrow or double backing flags its length's top bits (capacities stay below them): the
    // count is the word without them, as the interpreter reads it.
    let length = stage.and_imm(length, u64::from(!crate::heap::ENCODED_LEN));
    super::super::value::length_value(stage, length, miss)
}
