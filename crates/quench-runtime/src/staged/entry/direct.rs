//! Direct reads of an IC entry: the direct record, its shape link and the record and class reads.

use super::*;

/// The id and object record that `object` names in the record table at `records`, whose presence
/// the caller proved: `None` (leaving at `miss`) where the word is no direct object or its object is live.
#[inline(always)]
pub fn direct_record<S: Stage>(stage: &mut S, records: S::Word, object: S::Word, miss: S::Miss) -> Option<(S::Word, S::Word)> {
    let direct = super::super::value::is_direct_object(stage, object);
    stage.guard(direct, miss)?;
    let id = super::super::object::object_record_id_of(stage, object, miss)?;
    Some((id, super::super::object::object_record_at(stage, records, id)))
}

/// The folded value of a prototype-held read (`PrototypeProof::Linked`): the receiver `record` has
/// exact shape `shape` and prototype link `link`, so its holder is the one the read was folded
/// against and `value` is the folded constant (a Stable fact a code's live check holds). `None`
/// (leaving at `miss`) for any other receiver. Shape and link are the caller's values, compared here.
#[inline(always)]
pub fn held_constant<S: Stage>(stage: &mut S, _receiver: super::super::Linked<S::Word>, value: u64) -> S::Word {
    stage.constant(value)
}

/// The receiver check of a prototype-held read (`held_constant`, `guard::prototype`): the record `record` has
/// exact shape `shape` and prototype link `link`, else the caller leaves at `miss`. The one definition of the pair.
#[inline(always)]
pub fn shape_link_matches<S: Stage>(stage: &mut S, record: S::Word, (shape, link): (u64, u64), miss: S::Miss) -> Option<super::super::Linked<S::Word>> {
    let exact = super::super::object::exact_shape(stage, record);
    let same = stage.eq_imm(exact, shape);
    stage.guard(same, miss)?;
    let prototype = super::super::object::proto_link(stage, record);
    let same = stage.eq_imm(prototype, link);
    stage.guard(same, miss)?;
    Some(super::super::Linked::of_guard(record))
}

/// The read of the direct object `object` through `entry`, routed by the entry's key: a class key reads
/// the live cell (`direct_class_read`); a shape key reads the object's record, which leaves at `miss`
/// for a live cell (`object_record_of`), as `record_read_in` does for a record.
#[inline(always)]
pub fn direct_read_in<S: Stage>(stage: &mut S, records: S::Word, object: S::Word, entry: S::Word, miss: S::Miss) -> Option<S::Word> {
    let keyed = entry_keys_class(stage, entry);
    stage.choose_leaving(keyed, #[inline(always)] |stage| direct_class_read(stage, object, entry, miss),
        #[inline(always)] |stage| {
            let record = super::super::object::object_record_of(stage, records, object, miss)?;
            record_read_in(stage, records, record, entry, miss)
        })
}

/// The read of the live cell `object` through the class-keyed `entry`: its own value (`direct_own_value`)
/// or its held value (`class_held_value`).
#[inline(always)]
fn direct_class_read<S: Stage>(stage: &mut S, object: S::Word, entry: S::Word, miss: S::Miss) -> Option<S::Word> {
    let own = own_entry(stage, entry);
    stage.choose_leaving(own, #[inline(always)] |stage| direct_own_value(stage, object, entry, miss),
        #[inline(always)] |stage| class_held_value(stage, object, entry, miss))
}

/// The read of the record `record` (a function's object) through `entry`: its own value (`own_entry_value`)
/// or its prototype-held value (`holder_entry_value`).
#[inline(always)]
pub fn record_read_in<S: Stage>(stage: &mut S, records: S::Word, record: S::Word, entry: S::Word, miss: S::Miss) -> Option<S::Word> {
    let own = own_entry(stage, entry);
    stage.choose_leaving(own, #[inline(always)] |stage| own_entry_value(stage, record, entry, miss),
        #[inline(always)] |stage| holder_entry_value(stage, records, record, entry, miss))
}

/// The own value of the direct object `object` through the own entry `entry` (`class_own_value`), at the
/// live cell its granule names. The caller proved `object` a direct object.
#[inline(always)]
pub fn direct_own_value<S: Stage>(stage: &mut S, object: S::Word, entry: S::Word, miss: S::Miss) -> Option<S::Word> {
    let granule = super::super::value::direct_object_id(stage, object);
    let cell = super::super::object::cell_at_granule(stage, granule);
    class_own_value(stage, cell, entry, miss)
}
