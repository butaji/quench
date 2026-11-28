//! Slot reads over the flat, packed and narrow forms of an object's values.

use super::*;

/// The address of value zero of the wide run a plain object's `payload` word names outside
/// inline storage: `None` (leaving at `miss`) for a narrow run, whose words are halves.
#[inline(always)]
pub(crate) fn flat_run<S: Stage>(stage: &mut S, payload: S::Word, miss: S::Miss) -> Option<S::Word> {
    let narrow = narrow_run(stage, payload);
    let wide = stage.not(narrow);
    stage.guard(wide, miss)?;
    Some(arena_run(stage, payload))
}

/// Whether a plain object's payload word, outside inline storage, names a narrow run.
#[inline(always)]
pub(crate) fn narrow_run<S: Stage>(stage: &mut S, payload: S::Word) -> S::Cond {
    let tag = stage.and_imm(payload, OBJECT.narrow_run_tag);
    stage.ne_imm(tag, 0u32)
}

#[cfg_attr(feature = "test-support", quench_macros::test_visible)]
/// Value `slot` of an object of `layout`, which holds it, as the word its owner's `rep` encodes:
/// a `Value`'s bits for `Representation::Tagged` and `Representation::Int32`, the IEEE double for `Representation::Float64`
/// (`value_word` makes a `Value` of either). Only an arena slot holds a raw double: a packed or
/// narrow slot holds a narrow value whatever its owner's type, and reads as one, a `Value`.
#[inline(always)]
pub(crate) fn read_slot<S: Stage>(stage: &mut S, layout: Layout<S::Word, S::Cond>, slot: u16, rep: Representation) -> S::Word {
    debug_assert!(usize::from(slot) < layout.storage_len);
    match layout.slots {
        Slots::Flat(base) => flat_read(stage, base, slot, rep),
        Slots::Packed(packed) => packed_read(stage, packed, slot, rep),
        Slots::Either { packed: Packed::Record(record), inline, flat } => {
            // One operand for both forms (`site_offset`): a stencil numbers its holes as the row runs.
            let offset = site_offset(stage, slot);
            stage.choose(inline, #[inline(always)] |stage| {
                let half = stage.shr_imm(offset, HALF_SHIFT);
                let address = stage.add(record, half);
                let narrow = stage.load_u32(Region::Payload.mutable(), address, OBJECT.payload_offset as i32);
                decode_narrow(stage, narrow)
            }, #[inline(always)] |stage| {
                let address = stage.add(flat, offset);
                match rep {
                    Representation::Float64 => stage.load_f64(Region::ObjectSlots.mutable(), address, 0),
                    Representation::Int32 | Representation::Int64 | Representation::Tagged => stage.load_u64(Region::ObjectSlots.mutable(), address, 0),
                }
            })
        }
        Slots::Narrow(base) => narrow_read(stage, base, slot, rep),
    }
}

/// The `Value` bits of the word `read_slot` read at `slot` of `layout` in `rep`: a raw double
/// of an arena run boxed as a raw double word (`Stage::box_raw_number`), any other word as it is.
#[inline(always)]
pub(crate) fn read_slot_value<S: Stage>(stage: &mut S, layout: Layout<S::Word, S::Cond>, slot: u16, rep: Representation) -> S::Word {
    match (rep, layout.slots) {
        (Representation::Float64, Slots::Flat(_)) => { let number = read_slot(stage, layout, slot, rep); stage.box_raw_number(super::super::Float64::of_row(number)) }
        (Representation::Float64, Slots::Either { packed, inline, flat }) => stage.choose(inline,
            #[inline(always)] |stage| packed_read(stage, packed, slot, Representation::Tagged),
            #[inline(always)] |stage| { let number = flat_read(stage, flat, slot, rep); stage.box_raw_number(super::super::Float64::of_row(number)) }),
        _ => read_slot(stage, layout, slot, Representation::Tagged),
    }
}

/// `read_slot` at a run-time `index` (a for-in key's listed slot): value `index` of an object
/// of `layout`, which the caller proved holds it (`index < layout.storage_len`), as the word
/// its owner's `rep` encodes.
#[inline(always)]
pub(crate) fn read_slot_at<S: Stage>(stage: &mut S, layout: Layout<S::Word, S::Cond>, index: S::Word, rep: Representation) -> S::Word {
    match layout.slots {
        Slots::Flat(base) => flat_read_at(stage, base, index, rep),
        Slots::Packed(packed) => packed_read_at(stage, packed, index, rep),
        Slots::Either { packed, inline, flat } => stage.choose(inline,
            #[inline(always)] |stage| packed_read_at(stage, packed, index, rep), #[inline(always)] |stage| flat_read_at(stage, flat, index, rep)),
        Slots::Narrow(base) => {
            debug_assert!(rep != Representation::Float64, "a narrow run holds narrow values, never a raw double");
            let offset = stage.mul_imm(index, u64::from(u32::BITS / 8));
            let address = stage.add(base, offset);
            let narrow = stage.load_u32(Region::ObjectSlots.mutable(), address, 0);
            decode_narrow(stage, narrow)
        }
    }
}

/// Flat value `index` of the run at `base`, `index` a run-time word.
#[inline(always)]
fn flat_read_at<S: Stage>(stage: &mut S, base: S::Word, index: S::Word, rep: Representation) -> S::Word {
    let offset = stage.mul_imm(index, OBJECT.property_value_size as u64);
    let address = stage.add(base, offset);
    match rep {
        Representation::Float64 => stage.load_f64(Region::ObjectSlots.mutable(), address, 0),
        Representation::Int32 | Representation::Int64 | Representation::Tagged => stage.load_u64(Region::ObjectSlots.mutable(), address, 0),
    }
}

/// Packed value `index` (0 or 1, a run-time word): its half of the payload word, decoded.
#[inline(always)]
fn packed_read_at<S: Stage>(stage: &mut S, packed: Packed<S::Word>, index: S::Word, rep: Representation) -> S::Word {
    const _: () = assert!(OBJECT.inline_property_count == 2, "a packed index selects one of two halves");
    debug_assert!(rep != Representation::Float64, "a packed slot holds a narrow value, never a raw double");
    let payload = match packed {
        Packed::Record(record) => stage.load_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32),
    };
    let upper = stage.shr_imm(payload, u64::from(u32::BITS));
    let second = stage.ne_imm(index, 0u32);
    let half = stage.select(second, upper, payload);
    let narrow = stage.and_imm(half, u64::from(u32::MAX));
    decode_narrow(stage, narrow)
}

/// Narrow value `slot` of the run whose word zero is at `base`, decoded.
#[inline(always)]
fn narrow_read<S: Stage>(stage: &mut S, base: S::Word, slot: u16, _rep: Representation) -> S::Word {
    const _: () = assert!(cfg!(target_endian = "little"), "narrow slot i is the u32 at byte 4 * i");
    let offset = stage.site(u64::from(slot) * u64::from(u32::BITS / 8));
    let address = stage.add_imm(base, offset);
    let narrow = stage.load_u32(Region::ObjectSlots.mutable(), address, 0);
    decode_narrow(stage, narrow)
}

/// The shift from a slot's byte offset in a flat run to its half's in the packed payload word.
const HALF_SHIFT: u64 = (OBJECT.property_value_size as u64 / HALF_BYTES).trailing_zeros() as u64;

/// The byte offset of `slot` in a flat run, a per-site operand, as a word: the one operand a read of
/// either form takes, the packed half's offset being it shifted (`HALF_SHIFT`).
#[inline(always)]
fn site_offset<S: Stage>(stage: &mut S, slot: u16) -> S::Word {
    let offset = stage.site(u64::from(slot) * OBJECT.property_value_size as u64);
    let zero = stage.constant(0);
    stage.add_imm(zero, offset)
}

/// Flat value `slot`: the slot is a per-site operand, so its byte offset is a hole.
#[inline(always)]
fn flat_read<S: Stage>(stage: &mut S, base: S::Word, slot: u16, rep: Representation) -> S::Word {
    let offset = stage.site(u64::from(slot) * OBJECT.property_value_size as u64);
    match rep {
        Representation::Float64 => {
            let address = stage.add_imm(base, offset);
            stage.load_f64(Region::ObjectSlots.mutable(), address, 0)
        }
        Representation::Int32 | Representation::Int64 | Representation::Tagged => stage.load_site(Region::ObjectSlots.mutable(), base, offset),
    }
}

/// Packed value `slot`: its half of the payload word, decoded.
#[inline(always)]
fn packed_read<S: Stage>(stage: &mut S, packed: Packed<S::Word>, slot: u16, _rep: Representation) -> S::Word {
    debug_assert!(usize::from(slot) < OBJECT.inline_property_count);
    let payload = match packed {
        Packed::Record(record) => stage.load_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32),
    };
    let shift = stage.site(u64::from(slot) * u64::from(u32::BITS));
    let half = stage.shr_imm(payload, shift);
    let narrow = stage.and_imm(half, u64::from(u32::MAX));
    decode_narrow(stage, narrow)
}
