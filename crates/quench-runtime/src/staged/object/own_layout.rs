//! The layout an object record proves for its own values, from its shape and storage form.

use super::*;

#[cfg_attr(feature = "test-support", quench_macros::test_visible)]
/// The layout of the object record at `record`, whose exact shape the caller proved and which
/// holds `storage_len` values in `form`; `inline` is its `inline_storage`.
///
/// An object whose values extras displaced is read where the stage knows it has extras (the
/// interpreter); a specializer leaves through `miss` for it, so its residual handles the two
/// common forms only. A form the site proved is guarded, not decoded.
#[inline(always)]
pub(crate) fn own_layout<S: Stage>(stage: &mut S, record: S::Word, inline: S::Cond, form: Form,
    storage_len: usize, access: PackedAccess, miss: S::Miss) -> Option<Layout<S::Word, S::Cond>> {
    let payload = stage.load_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32);
    let extras = has_extras(stage, payload);
    layout_of(stage, record, payload, inline, extras, form, storage_len, access, miss)
}

/// `own_layout` of the record at `record` whose auxiliary word `word` the caller loaded to prove
/// its exact shape: the word's storage-form bits (`storage_form`) stand for the header's inline
/// flag and the payload's extras tag, so neither is loaded to be tested.
#[inline(always)]
pub(crate) fn formed_own_layout<S: Stage>(stage: &mut S, record: S::Word, word: S::Word, form: Form,
    storage_len: usize, access: PackedAccess, miss: S::Miss) -> Option<Layout<S::Word, S::Cond>> {
    // A stage that decides the form takes the arena run, the common form, on one test.
    let bits = stage.and_imm(word, u64::from(OBJECT.inline_form | OBJECT.extras_form | OBJECT.narrow_form));
    let arena = stage.eq_imm(bits, 0u32);
    if stage.known(arena) == Some(true) && form != Form::Packed {
        let payload = stage.load_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32);
        return Some(Layout { slots: Slots::Flat(arena_run(stage, payload)), storage_len });
    }
    let extras = storage_form(stage, word, OBJECT.extras_form);
    let inline = storage_form(stage, word, OBJECT.inline_form);
    let payload = stage.load_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32);
    layout_of(stage, record, payload, inline, extras, form, storage_len, access, miss)
}

/// The layout of the record at `record` whose whole auxiliary word a guard proved equal to a
/// word with storage-form bits `bits` (`Object::hint_shape`): the form is a static input, so
/// neither the inline flag, the extras tag nor a narrow run is tested. Values behind extras (a
/// watched prototype's, a global object's) are the extras' wide inline values or the run whose
/// start the extras hold, as `extras_slots` reads them. `None` for any other bits.
#[inline(always)]
pub(crate) fn proved_own_layout<S: Stage>(stage: &mut S, shaped: super::super::Shaped<S::Word>, bits: u32, storage_len: usize)
    -> Option<Layout<S::Word, S::Cond>> {
    let slots = match bits {
        bits if bits == OBJECT.inline_form => Slots::Packed(Packed::Record(shaped.record())),
        0 => {
            let payload = stage.load_u64(Region::Payload.mutable(), shaped.record(), OBJECT.payload_offset as i32);
            Slots::Flat(arena_run(stage, payload))
        }
        bits if bits == OBJECT.narrow_form => {
            let payload = stage.load_u64(Region::Payload.mutable(), shaped.record(), OBJECT.payload_offset as i32);
            Slots::Narrow(arena_run(stage, payload))
        }
        bits if bits == OBJECT.extras_form || bits == OBJECT.extras_form | OBJECT.inline_form => {
            let payload = stage.load_u64(Region::Payload.mutable(), shaped.record(), OBJECT.payload_offset as i32);
            let pointer = extras_pointer(stage, payload);
            if bits == OBJECT.extras_form {
                let start = stage.load_u32(Region::Payload.mutable(), pointer, OBJECT.extras_arena_start_offset as i32);
                Slots::Flat(arena_run(stage, start))
            } else {
                Slots::Flat(stage.add_imm(pointer, OBJECT.extras_inline_values_offset as u64))
            }
        }
        _ => return None,
    };
    Some(Layout { slots, storage_len })
}

/// `own_layout` past its loads: `payload` is the record's payload word, `inline` and `extras`
/// its inline flag and extras tag.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn layout_of<S: Stage>(stage: &mut S, record: S::Word, payload: S::Word, inline: S::Cond, extras: S::Cond, form: Form,
    storage_len: usize, access: PackedAccess, miss: S::Miss) -> Option<Layout<S::Word, S::Cond>> {
    if stage.known(extras) == Some(true) {
        return Some(Layout { slots: extras_slots(stage, payload, inline), storage_len });
    }
    // The extras test already decided it: one compare of the masked tag, not a second one.
    let plain = stage.not(extras);
    stage.guard(plain, miss)?;
    let packed = match access {
        PackedAccess::Reload => Packed::Record(record),
    };
    Some(Layout { slots: plain_slots(stage, payload, packed, inline, form, miss)?, storage_len })
}

/// The layout of the object record at `record` whose `storage_len` values the arena holds behind
/// its extras (`Object::extras_storage_start`), as a global object by name does, `inline` being
/// its `inline_storage`: a record without extras, or with its values inline, leaves at `miss`.
#[inline(always)]
pub fn arena_extras_layout<S: Stage>(stage: &mut S, record: S::Word, inline: S::Cond, storage_len: usize,
    miss: S::Miss) -> Option<Layout<S::Word, S::Cond>> {
    let payload = stage.load_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32);
    let extras = has_extras(stage, payload);
    stage.guard(extras, miss)?;
    let flat = stage.not(inline);
    stage.guard(flat, miss)?;
    Some(Layout { slots: extras_slots(stage, payload, inline), storage_len })
}

/// The slots of an object whose payload word points at its extras, which hold the inline
/// values wide or the run's start (`Object::extras_storage_start`).
#[inline(always)]
fn extras_slots<S: Stage>(stage: &mut S, payload: S::Word, inline: S::Cond) -> Slots<S::Word, S::Cond> {
    let pointer = extras_pointer(stage, payload);
    if stage.known(inline) == Some(true) {
        Slots::Flat(stage.add_imm(pointer, OBJECT.extras_inline_values_offset as u64))
    } else {
        let start = stage.load_u32(Region::Payload.mutable(), pointer, OBJECT.extras_arena_start_offset as i32);
        Slots::Flat(arena_run(stage, start))
    }
}

/// The slots of a plain object in `form`: packed in its payload or a run in the arena, the
/// inline flag decided by the stage, guarded, or left to the read.
#[inline(always)]
fn plain_slots<S: Stage>(stage: &mut S, payload: S::Word, packed: Packed<S::Word>, inline: S::Cond,
    form: Form, miss: S::Miss) -> Option<Slots<S::Word, S::Cond>> {
    let flat = match stage.known(inline) {
        Some(inline) => !inline,
        None => match form {
            Form::Packed => { stage.guard(inline, miss)?; false }
            Form::Arena => {
                let flat = stage.not(inline);
                stage.guard(flat, miss)?;
                true
            }
            Form::Either => {
                // A narrow run on the flat side leaves: only the interpreter reads one, and its
                // stage decides the flag.
                let narrow = narrow_run(stage, payload);
                let wide = stage.not(narrow);
                let admitted = stage.any(inline, wide);
                stage.guard(admitted, miss)?;
                // The run is addressed on the flat side of the flag only.
                let flat = stage.choose(inline, #[inline(always)] |stage| stage.constant(0), #[inline(always)] |stage| arena_run(stage, payload));
                return Some(Slots::Either { packed, inline, flat });
            }
        },
    };
    if !flat {
        return Some(Slots::Packed(packed));
    }
    // A flat plain object's run may be narrow (`NARROW_RUN_TAG` beside its start): the
    // interpreter reads it, a specializer leaves.
    let narrow = narrow_run(stage, payload);
    if stage.known(narrow) == Some(true) {
        return Some(Slots::Narrow(arena_run(stage, payload)));
    }
    let wide = stage.not(narrow);
    stage.guard(wide, miss)?;
    Some(Slots::Flat(arena_run(stage, payload)))
}
