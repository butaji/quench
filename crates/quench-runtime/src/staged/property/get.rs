//! `GetField` as a row: the entry points that run the read over a stage.

use super::*;

/// `GetField`: `base`'s value, as the loop's entry or the record of an OSR region already
/// holds it, else read through `lookup`'s plan.
#[inline(always)]
pub fn get_field<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, base: FieldBase, lookup: FieldLookup,
    miss: S::Miss) -> Option<S::Word> {
    if let Some(word) = frame.hoisted(stage, lookup) { return Some(word); }
    if let Some(word) = frame.entry_field(stage, lookup) { return Some(word); }
    if base.0 == FieldBase::NESTED {
        // A nested lookup's base is the site's, which the frame names; its links keep no proof.
        // Two own hops inline, each one exact-shape compare and one slot load, as the hand read.
        // The two hops written out, not looped, so each hop's guard folds to its own compare.
        let [first, second] = frame.plans().nested_hops()?;
        let base = frame.nested_base(stage, miss)?;
        let middle = nested_hop(stage, frame, base, first, miss)?;
        return nested_hop(stage, frame, middle, second, miss);
    }
    // A local base is read through the frame's local (`PropertyFrame::local_base`), which keeps no
    // proof of its object, as a temporary's.
    let local = base.local_index();
    let owner = match local {
        Some(_) => Owner::Temporary,
        None => Owner::of(base)?,
    };
    let plans = frame.plans();
    let plan = plans.field_plan(lookup)?;
    let object = match local {
        Some(slot) => frame.local_base(stage, slot, miss)?,
        None => base_word(stage, frame, base, miss)?,
    };
    // A plan of ops over the receiver's own record, the one a lane record's hint holds, inline;
    // every other plan through `read_with`, which recurses through nested plans.
    if let FieldPlan::Ops(ops) = plan {
        let at = super::read::plan_receiver(stage, frame, (owner, false), object, ops, miss)?;
        let records = frame.object_records(stage);
        return run_owned(stage, frame, (owner, true), object, ops, (at, records), miss);
    }
    if let FieldPlan::Own { layout, slot } = plan {
        let at = frame.receiver_record(stage, owner, object, false, miss)?;
        return own_slot(stage, frame, (owner, true), object, &at, (layout, slot), miss);
    }
    // A prototype-held read, the other plan a row's holes name, inline too: only nested plans recurse.
    if let FieldPlan::Inherited { shape, constants, proof } = plan {
        return inherited(stage, frame, (owner, false), object, (shape, constants, proof), miss);
    }
    // The polymorphic read (the record's hint, then the VM's entries) and the entry reads inline as
    // well: through `read_with`, out of line, the row built the plan in memory (a 1.5 KB frame,
    // ~165 Ir) at every hint miss.
    if let FieldPlan::Hinted { ops, entry } = plan {
        return frame.entried(stage, owner, object, entry, miss, #[inline(always)] |stage, frame, miss| {
            let at = frame.receiver_record(stage, owner, object, false, miss)?;
            let records = frame.object_records(stage);
            run_owned(stage, frame, (owner, true), object, ops, (at, records), miss)
        });
    }
    if let FieldPlan::Entry(kind) = plan {
        return frame.field_entry(stage, owner, object, kind, miss);
    }
    read_with(stage, frame, (owner, false), object, plan, miss)
}

/// The property operations of `step`: the word each produces, or `Unresolved` where the
/// frame holds no plan or callout for it.
#[inline(always)]
pub fn property_step_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let word = match operation {
        // A read its plan leaves is the VM's named read, where the frame takes it (`CallOut::LoadField`).
        Operation::GetField { base, lookup, destination } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::GET_FIELD]) } { match get_field(stage, frame, base, lookup, miss) {
            Some(word) => Some(word),
            None => return super::super::callout::loading(stage, frame, super::super::CallOut::LoadField { lookup }, destination),
        } } else { unreachable!() },
        // A store its plan leaves is the VM's named store, where the frame takes it (`CallOut::StoreField`). A store
        // to `this` has no base register: the VM takes `this` where the frame holds it (`Frame::this_value`).
        Operation::SetField { value, base, atom, site, strict } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::SET_FIELD]) } { match super::super::property_write::set_field(stage, frame, (value, base), (atom, site), miss) {
            Some(word) => Some(word),
            None => match base {
                Some(object) => super::super::callout::storing(stage, frame,
                    super::super::CallOut::StoreField { object: Some(object), value, atom, site, strict }),
                None => frame.this_value(stage).and_then(|_| super::super::callout::storing(stage, frame,
                    super::super::CallOut::StoreField { object: None, value, atom, site, strict })),
            },
        } } else { unreachable!() },
        Operation::StoreField { base, value, fact, atom, site } =>
            if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::STORE_FIELD]) } { super::super::property_write::store_word(stage, frame, base, (value, fact), (atom, site), miss) } else { unreachable!() },
        Operation::FieldGetHost { object, atom, site } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::FIELD_GET_HOST]) } { frame.field_probe(stage, Owner::Temporary, object, atom, site, miss) } else { unreachable!() },
        Operation::FieldAdd { object, atom, site, value } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::FIELD_ADD]) } { frame.field_add(stage, object, atom, site, value, miss).map(|()| value) } else { unreachable!() },
        Operation::IndexGetHost { base, key } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::INDEX_GET_HOST]) } { frame.index_get_host(stage, base, key, miss) } else { unreachable!() },
        _ => if const { super::super::op_kind::outside(KIND, &[super::super::op_kind::GET_FIELD, super::super::op_kind::SET_FIELD, super::super::op_kind::STORE_FIELD, super::super::op_kind::FIELD_GET_HOST, super::super::op_kind::FIELD_ADD, super::super::op_kind::INDEX_GET_HOST]) } { None } else { unreachable!() },
    };
    word.map_or(Step::Unresolved, Step::Value)
}
