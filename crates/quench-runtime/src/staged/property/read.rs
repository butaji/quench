//! The named-property read rows: receiver and base words, read arms, the chain of prototypes and nested hops.

use super::*;

/// The record of the direct object `object`; anything else, and a live object (which has no record), leaves at `miss`.
#[inline(always)]
pub fn receiver<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, object: S::Word,
    miss: S::Miss) -> Option<Reached<S::Word, S::Cond>> {
    let records = frame.object_records(stage)?;
    let direct = value::is_direct_object(stage, object);
    stage.guard(direct, miss)?;
    Some(Reached::bare(object_record_of(stage, records, object, miss)?))
}

/// The object part a plan's ops read from: a class-keyed plan (its first op guards a class, constant or an ahead image's) starts from the
/// live class cell, every other from the object's record. The key decides it, so no run-time test chooses.
#[inline(always)]
pub fn plan_receiver<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, (owner, readable): (Owner, bool),
    object: S::Word, ops: &[IcOp<'_>], miss: S::Miss) -> Option<Reached<S::Word, S::Cond>> {
    match ops {
        [IcOp::GuardClass(_) | IcOp::GuardImageClass(_), ..] => frame.receiver_cell(stage, object, miss),
        _ => frame.receiver_record(stage, owner, object, readable, miss),
    }
}

/// The base of a field access: the activation's bound `this`, or a register's tagged word;
/// a local or nested base is the tier's own.
#[inline(always)]
pub fn base_word<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, base: FieldBase, miss: S::Miss) -> Option<S::Word> {
    if base == FieldBase::THIS {
        return frame.field_this(stage, miss);
    }
    Some(frame.tagged(stage, base.register_index()?))
}

/// The value `plan` reads from `owner`'s `object`, or a leave at `miss`; `readable` where the
/// plan admits a function's record as the receiver. Inlined, so a tier whose plan is static data
/// (a stencil row's) folds the match to its one arm; the recursive plans re-enter through
/// `read_nested`, so this function never calls itself and stays inlinable.
#[inline(always)]
pub fn read_with<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, (owner, readable): (Owner, bool),
    object: S::Word, plan: FieldPlan<'_>, miss: S::Miss) -> Option<S::Word> {
    match plan {
        FieldPlan::Ops(ops) => {
            let at = plan_receiver(stage, frame, (owner, readable), object, ops, miss)?;
            let records = frame.object_records(stage);
            run_owned(stage, frame, (owner, !readable), object, ops, (at, records), miss)
        }
        FieldPlan::Own { layout, slot } => {
            let at = frame.receiver_record(stage, owner, object, readable, miss)?;
            own_slot(stage, frame, (owner, !readable), object, &at, (layout, slot), miss)
        }
        FieldPlan::Arms(list) => {
            let at = frame.receiver_record(stage, owner, object, readable, miss)?;
            for held in list.iter().filter_map(|arm| arm.holds) { frame.hold_prototype(stage, held, miss)?; }
            let records = frame.object_records(stage);
            let proven = frame.proven_shape(stage, owner);
            if let Some((arm, fact)) = proven.and_then(|(shape, fact)| list.iter().find(|arm| arm.shapes.contains(&shape)).map(|arm| (arm, fact))) {
                return ic::run(stage, arm.ops, Reached { shaped: Some(super::super::Shaped::by_fact(at.record, fact)), ..at }, records, miss);
            }
            let shape = ic::shape_word(stage, &at);
            arms(stage, list, Reached { shape: Some(shape), ..at }, records, miss)
        }
        FieldPlan::Classed { classes, rest } => {
            let records = frame.object_records(stage);
            // The class of a direct object's live cell; any other word reads as `NO_CLASS`, which no arm names, so
            // `rest` takes it. The cell is addressed only where the object is direct.
            let direct = value::is_direct_object(stage, object);
            let class = stage.choose(direct,
                #[inline(always)] |stage| { let cell = super::super::object::cell_of(stage, object); super::super::object::cell_class(stage, cell) },
                #[inline(always)] |stage| stage.constant(u64::from(super::super::object_encoding::NO_CLASS)));
            classed(stage, frame, (owner, readable), object, (class, classes), (records, *rest), miss)
        }
        FieldPlan::Inherited { shape, constants, proof } =>
            inherited(stage, frame, (owner, readable), object, (shape, constants, proof), miss),
        FieldPlan::Readable(inner) => read_nested(stage, frame, (owner, true), object, *inner, miss),
        FieldPlan::Probed { fast, atom, site } => frame.probed(stage, owner, object, (atom, site), miss,
            #[inline(always)] |stage, frame, miss| read_nested(stage, frame, (owner, readable), object, *fast, miss)),
        FieldPlan::Known => frame.known_value(stage, owner),
        FieldPlan::Chain(links) => chain(stage, frame, (owner, readable), object, links, miss),
        FieldPlan::Host { atom, site } => frame.field_probe(stage, owner, object, atom, site, miss),
        FieldPlan::Geometry { atom } => frame.geometry_read(stage, object, atom, miss),
        FieldPlan::Length => frame.length_read(stage, object, miss),
        FieldPlan::Entry(kind) => frame.field_entry(stage, owner, object, kind, miss),
        FieldPlan::Hinted { ops, entry } => frame.entried(stage, owner, object, entry, miss, #[inline(always)] |stage, frame, miss| {
            let at = frame.receiver_record(stage, owner, object, readable, miss)?;
            let records = frame.object_records(stage);
            run_owned(stage, frame, (owner, !readable), object, ops, (at, records), miss)
        }),
        FieldPlan::Private { atom } => frame.private_field(stage, object, atom, miss),
    }
}

/// A prototype-held read (`FieldPlan::Inherited`) of `owner`'s `object`: the receiver's exact shape and the
/// holder its proof names, then the constant the holder held.
#[inline(always)]
pub fn inherited<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, (owner, readable): (Owner, bool), object: S::Word,
    (shape, constants, proof): (u32, &[(u32, u64)], PrototypeProof), miss: S::Miss) -> Option<S::Word> {
    let at = frame.receiver_record(stage, owner, object, readable, miss)?;
    match proof {
        PrototypeProof::Watched { holder } => {
            let shaped = guard_receiver_shape(stage, frame, (owner, !readable), &at, shape, miss)?;
            frame.hold_prototype(stage, holder, miss)?;
            ic::run(stage, &[IcOp::PrototypeConstants(constants)], Reached { shaped: Some(shaped), ..Reached::bare(at.record) }, None, miss)
        }
        PrototypeProof::Linked { expected_link } => {
            let &[(_, value)] = constants else { return None };
            super::super::entry::shape_link_matches(stage, at.record, (u64::from(shape), u64::from(expected_link)), miss).map(|linked| super::super::entry::held_constant(stage, linked, value))
        }
    }
}

/// `read_with` of a plan nested in another (`Readable`, `Probed`, a `Chain` link): the one
/// out-of-line entry of the recursion.
fn read_nested<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, owner: (Owner, bool),
    object: S::Word, plan: FieldPlan<'_>, miss: S::Miss) -> Option<S::Word> {
    read_with(stage, frame, owner, object, plan, miss)
}

/// Continues where `owner`'s object has exact shape `shape`: a proof the frame holds emits
/// nothing, else the guard is made and recorded. `direct` where the receiver is a direct object.
#[inline(always)]
pub fn guard_receiver_shape<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, (owner, direct): (Owner, bool),
    at: &Reached<S::Word, S::Cond>, shape: u32, miss: S::Miss) -> Option<super::super::Shaped<S::Word>> {
    if let Some((proven, fact)) = frame.proven_shape(stage, owner) && proven == shape { return Some(super::super::Shaped::by_fact(at.record, fact)); }
    let shaped = ic::guard_shape(stage, at, shape, miss)?;
    frame.prove_shape(stage, owner, (shape, shaped), direct);
    Some(shaped)
}

/// `run_owned` of `FieldPlan::Own`: the layout guard and the slot it proves.
#[inline(always)]
pub fn own_slot<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, owned: (Owner, bool), object: S::Word,
    at: &Reached<S::Word, S::Cond>, (layout, slot): (u32, u16), miss: S::Miss) -> Option<S::Word> {
    // `run_owned`'s arm of the same two ops.
    word_formed_slot(stage, frame, owned, object, at, layout, (slot, crate::typed_ir::Representation::Tagged, usize::from(slot) + 1), miss)
}

/// `ops` over the record `at` reached, where a leading shape guard is the owner's proof: its
/// own-slot load reads through the layout the frame kept, or proves and keeps it.
#[inline(always)]
pub fn run_owned<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, (owner, direct): (Owner, bool), object: S::Word,
    ops: &[IcOp<'_>], (at, records): (Reached<S::Word, S::Cond>, Option<S::Word>), miss: S::Miss) -> Option<S::Word> {
    if let [IcOp::GuardLayout(word), IcOp::LoadSlot { slot, storage_len, rep, .. }] = ops {
        return word_formed_slot(stage, frame, (owner, direct), object, &at, *word, (*slot, *rep, *storage_len), miss);
    }
    let [IcOp::GuardShape(shape), rest @ ..] = ops else { return ic::run(stage, ops, at, records, miss) };
    if direct && let [IcOp::LoadSlot { slot, form: Form::Arena, rep, storage_len }] = rest
        && frame.proven_shape(stage, owner).map(|(proven, _)| proven) != Some(*shape)
        && frame.kept_layout(owner, object, *shape).is_none() {
        return formed_slot(stage, frame, (owner, direct, *shape), object, &at, (*slot, *rep, *storage_len), miss);
    }
    let at = Reached { shaped: Some(guard_receiver_shape(stage, frame, (owner, direct), &at, *shape, miss)?), ..at };
    match rest {
        [IcOp::LoadSlot { slot, form, storage_len, rep }] =>
            kept_slot(stage, frame, (owner, *shape), object, &at, (*slot, *form, *storage_len, *rep), miss),
        _ => ic::run_unrolled(stage, rest, at, records, miss),
    }
}

/// The first of `plans` whose shape set holds the record's shape reads it; the last is the guard of the
/// whole. The member test is the arm's shape guard (`Shaped`): its shapes share the slots it reads.
#[inline(always)]
pub fn arms<S: Stage>(stage: &mut S, plans: &[Arm<'_>], at: Reached<S::Word, S::Cond>, records: Option<S::Word>,
    miss: S::Miss) -> Option<S::Word> {
    let ((arm, rest), shaped) = (plans.split_first()?, Reached { shaped: Some(super::super::Shaped::of_guard(at.record)), ..at });
    let exact = ic::shape_word(stage, &at);
    let member = stage.shape_in_site(exact, arm.shapes);
    if rest.is_empty() {
        stage.guard(member, miss)?;
        return ic::run(stage, arm.ops, shaped, records, miss);
    }
    stage.choose_leaving(member, #[inline(always)] |stage| ic::run(stage, arm.ops, shaped, records, miss),
        #[inline(always)] |stage| arms(stage, rest, at, records, miss))
}

/// The first of `classes` whose class is `class` reads its ops on the live cell `object` is (`Shaped::of_cell`); none
/// does, `rest` reads. The class is `choose`n once by `read_with`, so each arm costs one compare, as `arms` does for shapes.
#[inline(always)]
pub fn classed<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, (owner, readable): (Owner, bool), object: S::Word,
    (class, classes): (S::Word, &[ClassArm<'_>]), (records, rest): (Option<S::Word>, FieldPlan<'_>), miss: S::Miss) -> Option<S::Word> {
    let Some((arm, others)) = classes.split_first() else { return read_nested(stage, frame, (owner, readable), object, rest, miss) };
    let same = stage.eq_imm(class, u64::from(arm.class));
    stage.choose_leaving(same,
        #[inline(always)] |stage| {
            let cell = super::super::object::cell_of(stage, object);
            ic::run(stage, arm.ops, Reached { shaped: Some(super::super::Shaped::of_cell(cell)), ..Reached::bare(cell) }, records, miss)
        },
        #[inline(always)] |stage| classed(stage, frame, (owner, readable), object, (class, others), (records, rest), miss))
}

/// A nested lookup: each link reads from the previous link's value, the first from `owner`'s
/// object and the rest from objects no register names.
#[inline(always)]
pub fn chain<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, first: (Owner, bool), base: S::Word,
    links: &[FieldPlan<'_>], miss: S::Miss) -> Option<S::Word> {
    let mut object = base;
    let mut from = first;
    for link in links {
        object = read_nested(stage, frame, from, object, *link, miss)?;
        from = (Owner::Temporary, true);
    }
    Some(object)
}

/// One own hop of a nested read: `object`'s record, under `guard`, its `slot` in `form`.
#[inline(always)]
pub fn nested_hop<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, object: S::Word,
    (guard, slot, form): (IcOp<'static>, u16, Form), miss: S::Miss) -> Option<S::Word> {
    let at = frame.receiver_record(stage, Owner::Temporary, object, false, miss)?;
    let ops = [guard, IcOp::LoadSlot { slot, form, storage_len: usize::from(slot) + 1, rep: crate::typed_ir::Representation::Tagged }];
    ic::run(stage, &ops, at, None, miss)
}
