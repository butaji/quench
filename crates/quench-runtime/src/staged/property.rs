//! The named-property family as staged rows: `GetField` in every form (own slot, prototype
//! slot, prototype constant, polymorphic arms, nested chains, a VM probe behind a fast plan),
//! `SetField` and `StoreField` (own slots in place by shape arms, a loop's projected slot, the
//! VM's own-field, bound-slot and adding stores), `Delete` and `in`. A site's proved plan is
//! static data (`FieldPlan`, `StorePlan`); each row runs it once over a stage, so the interpreter
//! (`Concrete`) and every specializer (`Emit`) read one definition of each read and store.
//! Whatever the plan cannot prove is an explicit `PropertyFrame` call-out to the VM's helper,
//! never a second copy of the lookup. What a tier proved of an object before the row runs (its
//! record, its shape, its layout) is read back through the frame, so a guard the frame holds
//! the proof of is not emitted twice.

use super::ic::{self, IcOp, Reached};
use super::object::{object_record_at, read_slot, Form, Layout};
use super::{value, Frame, NumericFrame, NumericStage, Operation, Stage, Step};
use crate::bytecode::{FieldBase, FieldLookup, Register};
use crate::typed_ir::Type;

/// Whose object a read starts from: what a frame keys its proofs of an object by.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Owner {
    /// The activation's bound `this`.
    This,
    Register(Register),
    /// A value no register names (a nested lookup's intermediate), which keeps no proof.
    Temporary,
}

impl Owner {
    /// The owner of a field base: `this` or a register; any other base is the tier's own.
    pub(crate) fn of(base: FieldBase) -> Option<Self> {
        if base == FieldBase::THIS { return Some(Self::This); }
        base.register_index().map(Self::Register)
    }
}

/// One shape family of a polymorphic read: the sorted exact shapes that take `ops`, which
/// read from the record the shapes already matched.
#[derive(Clone, Copy)]
pub(crate) struct Arm<'a> {
    pub(crate) shapes: &'a [u32],
    pub(crate) ops: &'a [IcOp<'a>],
    /// The exact shape of the prototypes whose values `ops` fold as constants, which a code
    /// dependency holds (`PropertyFrame::hold_prototype`).
    pub(crate) holds: Option<u32>,
}

/// How a `GetField` site reads, as the proof its tier holds of the site.
#[derive(Clone, Copy)]
pub(crate) enum FieldPlan<'a> {
    /// The ops of `staged::ic` over the receiver's record: own slot, prototype slot, or a
    /// prototype constant.
    Ops(&'a [IcOp<'a>]),
    /// The first arm whose shape set holds the receiver's exact shape reads; none leaves.
    Arms(&'a [Arm<'a>]),
    /// A receiver of exact `shape` whose prototype `holder` holds the values `constants` pair
    /// with the receiver's prototype link, folded under a code dependency on the holder.
    Inherited { shape: u32, holder: u32, constants: &'a [(u32, u64)] },
    /// The plan, whose receiver may be a function's record: its own fields answer too, and
    /// no direct object is proved of the receiver.
    Readable(&'a FieldPlan<'a>),
    /// The plan; where it misses, the VM's own read of `atom` through cache `site`.
    Probed { fast: &'a FieldPlan<'a>, atom: u32, site: u16 },
    /// The value an earlier read of this owner's field proved, which the frame holds
    /// (`PropertyFrame::known_value`).
    Known,
    /// A nested lookup `a.b.c`: each plan reads from the value the previous one produced.
    Chain(&'a [FieldPlan<'a>]),
    /// The VM's own cached read of `atom` through cache `site`: a dictionary object, a
    /// deeper holder, an accessor, a proxy.
    Host { atom: u32, site: u16 },
}

/// One own data slot a store site proved: objects of exact `shape` keep the field at `slot`.
/// `holds` is `None` where the value stored is of a type the slot's type holds. Otherwise it
/// is the type the row tests the value for before it stores in place, so the slot's
/// representation never widens under the raw write; a value of any other type stores through
/// the VM, which joins the slot's type.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct StoreArm {
    pub(crate) shape: u32,
    pub(crate) slot: u16,
    pub(crate) holds: Option<Type>,
}

impl StoreArm {
    /// The arm for the slot `slot` of objects of `shape`, whose type is `slot_ty`, under a
    /// value of type `fact`. A slot that holds the value's type stores it as is. Otherwise
    /// the arm admits an int32, or for a slot that holds any direct object, that type; `None`
    /// where the slot's type holds no such value, so every store of it goes through the VM.
    pub(crate) fn of((shape, slot): (u32, u16), slot_ty: Type, fact: Type) -> Option<Self> {
        if fact.is_subtype(slot_ty) { return Some(Self { shape, slot, holds: None }); }
        let holds = if slot_ty.direct_object_part() == Some(Type::DIRECT_OBJECT_FAMILY) { slot_ty } else { Type::INT32 };
        holds.is_subtype(slot_ty).then_some(Self { shape, slot, holds: Some(holds) })
    }
}

/// How the VM completes a named store that an in-place arm does not: where no arm holds the
/// receiver's shape, or its value, its barrier or its storage.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum StoreJoin {
    /// The VM's own-field store hit, which a failure leaves at the miss edge.
    Host,
    /// The same hit through the site's bound slot first, which the frame addressed before the
    /// run (`PropertyFrame::field_store_bound`).
    Bound,
    /// The VM's adding entries, which complete a store of any shape without leaving; a frame
    /// that uses it holds no object proof past the store.
    Add,
}

/// How a named store site writes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum StorePlan<'a> {
    /// The first of `arms` whose exact shape the receiver has stores in place where the
    /// value, the barrier and the storage admit it; every other store runs through `join`.
    Own { arms: &'a [StoreArm], join: StoreJoin },
    /// The slot a loop's entry projected for this store, which the frame addresses
    /// (`PropertyFrame::entry_slot_store`).
    Entry,
    /// A store that may add the property or change the shape: the VM's adding entry.
    Add,
    /// A setter, a read-only or dictionary property, a proxy: the VM's store.
    Host,
    /// The VM's store through the slot the entry bound for the site.
    Bound,
}

impl<'a> StorePlan<'a> {
    /// The plan that stores in place by `arms` and otherwise by `join`; by `join` alone with
    /// no arm.
    pub(crate) fn of(arms: &'a [StoreArm], join: StoreJoin) -> Self {
        match (arms.is_empty(), join) {
            (false, join) => Self::Own { arms, join },
            (true, StoreJoin::Host) => Self::Host,
            (true, StoreJoin::Bound) => Self::Bound,
            (true, StoreJoin::Add) => Self::Add,
        }
    }
}

/// The plans a frame's tier proved, by site. A frame with none answers `None`, and the
/// operation is left to the host path.
pub(crate) trait PlanSource {
    /// The read plan of `lookup`.
    fn field_plan(&self, lookup: FieldLookup) -> Option<FieldPlan<'_>>;
    /// The store plan of cache site `site`.
    fn store_plan(&self, site: u16) -> Option<StorePlan<'_>>;
}

impl<P: PlanSource> PlanSource for &P {
    fn field_plan(&self, lookup: FieldLookup) -> Option<FieldPlan<'_>> { (**self).field_plan(lookup) }
    fn store_plan(&self, site: u16) -> Option<StorePlan<'_>> { (**self).store_plan(site) }
}

/// The plans of a frame that proved none.
#[derive(Clone, Copy, Default)]
pub(crate) struct NoPlans;

impl PlanSource for NoPlans {
    fn field_plan(&self, _lookup: FieldLookup) -> Option<FieldPlan<'_>> { None }
    fn store_plan(&self, _site: u16) -> Option<StorePlan<'_>> { None }
}

/// A frame's property callouts. Every default declines, leaving the operation to the host
/// path; a miss edge of the stage is each callout's failure.
pub(crate) trait PropertyFrame<S: Stage>: Frame<S> {
    /// The handle to the plans, copied out so a row holds a plan while it calls the frame.
    type Plans: PlanSource + Copy;

    /// The plans this frame's tier proved.
    fn plans(&self) -> Self::Plans;

    /// What the frame proved of the type of `register`'s value: `Type::ANY` where it proved
    /// nothing, which a frame that knows no static facts (the interpreter) always answers. A
    /// store folds what the type decides (the barrier of an immediate, the narrow encoder of an
    /// int32) and tests the rest dynamically.
    #[inline(always)]
    fn value_fact(&self, _register: Register) -> Type { Type::ANY }

    /// The base of the object record table, which prototype reads and stores address.
    #[inline(always)]
    fn object_records(&mut self, _stage: &mut S) -> Option<S::Word> { None }

    /// The bound `this` a field access names; a frame whose entry proved it bound answers it
    /// without the test. An unbound `this` leaves at `miss`.
    #[inline(always)]
    fn field_this(&mut self, stage: &mut S, miss: S::Miss) -> Option<S::Word> where Self: Sized {
        super::steps::bound_this(stage, self, miss)
    }

    /// The record of `owner`'s `object`, with the words the frame already holds of it (its
    /// shape and inline flag). A direct object, else a leave at `miss`; where `readable`, a
    /// function's record answers too, for a frame that proves one.
    #[inline(always)]
    fn receiver_record(&mut self, stage: &mut S, _owner: Owner, object: S::Word, _readable: bool,
        miss: S::Miss) -> Option<Reached<S::Word, S::Cond>> where Self: Sized {
        receiver(stage, self, object, miss)
    }

    /// The exact shape `owner`'s object is proved to have, which makes a guard of it redundant.
    #[inline(always)]
    fn proven_shape(&mut self, _stage: &mut S, _owner: Owner) -> Option<u32> { None }

    /// Records that `owner`'s object has `shape`, after the guard that tested it; `direct` where
    /// the plan proves the receiver a direct object, not a function's record.
    #[inline(always)]
    fn prove_shape(&mut self, _stage: &mut S, _owner: Owner, _shape: u32, _direct: bool) {}

    /// The layout proved of `owner`'s object of exact `shape`, which a read reuses.
    #[inline(always)]
    fn kept_layout(&self, _owner: Owner, _shape: u32) -> Option<Layout<S::Word, S::Cond>> { None }

    /// Records the layout a read proved of `owner`'s object of exact `shape`.
    #[inline(always)]
    fn keep_layout(&mut self, _owner: Owner, _shape: u32, _layout: Layout<S::Word, S::Cond>) {}

    /// Holds the values of the prototypes of exact shape `shape` as constants, under a code
    /// dependency. A frame that takes none answers `None`.
    #[inline(always)]
    fn hold_prototype(&mut self, _stage: &mut S, _shape: u32, _miss: S::Miss) -> Option<()> { None }

    /// The value an earlier read of `owner`'s field proved, which no write has retired.
    #[inline(always)]
    fn known_value(&mut self, _stage: &mut S, _owner: Owner) -> Option<S::Word> { None }

    /// The value a loop's entry read of `lookup` holds, for an access the loop repeats unchanged.
    #[inline(always)]
    fn hoisted(&mut self, _stage: &mut S, _lookup: FieldLookup) -> Option<S::Word> { None }

    /// The value the field `lookup` named held in the entry record of an OSR region.
    #[inline(always)]
    fn entry_field(&mut self, _stage: &mut S, _lookup: FieldLookup) -> Option<S::Word> { None }

    /// The VM's read of `atom` of `owner`'s `object` through `site`, which a frame that bound
    /// the site's slot answers inline first.
    #[inline(always)]
    fn field_probe(&mut self, stage: &mut S, _owner: Owner, object: S::Word, atom: u32, site: u16,
        miss: S::Miss) -> Option<S::Word> {
        self.field_get_host(stage, object, atom, site, miss)
    }

    /// What `fast` reads, which leaves at the edge it is given; where it misses, the VM's
    /// probe instead (`field_probe`), and past the join only what both prove holds.
    #[inline(always)]
    fn probed(&mut self, stage: &mut S, owner: Owner, object: S::Word, (atom, site): (u32, u16), miss: S::Miss,
        fast: impl FnOnce(&mut S, &mut Self, S::Miss) -> Option<S::Word>) -> Option<S::Word> {
        match fast(stage, self, miss) {
            Some(word) => Some(word),
            None => self.field_probe(stage, owner, object, atom, site, miss),
        }
    }

    /// The VM's cached read of `atom` of `object` through `site` (`LaneHelper::FieldGet`); a
    /// miss leaves at `miss`.
    #[inline(always)]
    fn field_get_host(&mut self, _stage: &mut S, _object: S::Word, _atom: u32, _site: u16, _miss: S::Miss) -> Option<S::Word> { None }

    /// The VM's store of `value` to `atom` of `object` through `site`, which may add the
    /// property (`LaneHelper::FieldAdd`); a failure leaves at `miss`.
    #[inline(always)]
    fn field_add(&mut self, _stage: &mut S, _object: S::Word, _atom: u32, _site: u16, _value: S::Word, _miss: S::Miss) -> Option<()> { None }

    /// The VM's own-field store hit (`LaneHelper::FieldSet`); a failure leaves at `miss`.
    #[inline(always)]
    fn field_store_host(&mut self, _stage: &mut S, _object: S::Word, _atom: u32, _site: u16, _value: S::Word, _miss: S::Miss) -> Option<()> { None }

    /// The VM's own-field store hit of `value`, of type `fact`, through the slot the entry bound
    /// for `site`, which a frame that bound one stores into inline first (a value its writes
    /// admit); the plain hit otherwise.
    #[inline(always)]
    fn field_store_bound(&mut self, stage: &mut S, _owner: Owner, object: S::Word, (atom, site): (u32, u16),
        (value, _fact): (S::Word, Type), miss: S::Miss) -> Option<()> {
        self.field_store_host(stage, object, atom, site, value, miss)
    }

    /// Stores `value` through the slot a loop's entry projected for the access at `site`, which
    /// a frame that made the projection (`StorePlan::Entry`) addresses; a failure leaves at `miss`.
    #[inline(always)]
    fn entry_slot_store(&mut self, _stage: &mut S, _site: u16, _value: S::Word, _miss: S::Miss) -> Option<()> { None }

    /// Continues where the run admits a direct store of the atom the access names (no method
    /// cache names it); a frame whose run changes that between accesses tests it here.
    #[inline(always)]
    fn store_admitted(&mut self, _stage: &mut S, _miss: S::Miss) -> Option<()> { Some(()) }

    /// What `fast` stores, which leaves at the edge it is given; where it misses, `slow` (the
    /// VM's store) instead, and past the join only what both prove holds.
    #[inline(always)]
    fn stored(&mut self, stage: &mut S, _owner: Owner, miss: S::Miss,
        fast: impl FnOnce(&mut S, &mut Self, S::Miss) -> Option<()>,
        slow: impl FnOnce(&mut S, &mut Self, S::Miss) -> Option<()>) -> Option<()> {
        match fast(stage, self, miss) {
            Some(()) => Some(()),
            None => slow(stage, self, miss),
        }
    }

    /// `delete base[key]` where `strict` throws on a refusal: the boolean word, or a leave
    /// at `miss`.
    #[inline(always)]
    fn delete_property(&mut self, _stage: &mut S, _base: S::Word, _key: S::Word, _strict: bool, _miss: S::Miss) -> Option<S::Word> { None }

    /// `base[key]` where no dense or typed element read answers it: a hole read through the
    /// prototype chain, an index past a typed array, a string's character, a keyed own read.
    #[inline(always)]
    fn index_get_host(&mut self, _stage: &mut S, _base: S::Word, _key: S::Word, _miss: S::Miss) -> Option<S::Word> { None }
}

/// The record of the direct object `object`; anything else leaves at `miss`.
#[inline(always)]
fn receiver<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, object: S::Word,
    miss: S::Miss) -> Option<Reached<S::Word, S::Cond>> {
    let records = frame.object_records(stage)?;
    let direct = value::is_direct_object(stage, object);
    stage.guard(direct, miss)?;
    let id = stage.and_imm(object, u64::from(u32::MAX));
    Some(Reached { record: object_record_at(stage, records, id), shape: None, inline: None })
}

/// The base of a field access: the activation's bound `this`, or a register's tagged word;
/// a local or nested base is the tier's own.
#[inline(always)]
pub(super) fn base_word<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, base: FieldBase, miss: S::Miss) -> Option<S::Word> {
    if base == FieldBase::THIS {
        return frame.field_this(stage, miss);
    }
    Some(frame.tagged(stage, base.register_index()?))
}

/// The value `plan` reads from `owner`'s `object`, or a leave at `miss`; `readable` where the
/// plan admits a function's record as the receiver.
pub(super) fn read_with<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, (owner, readable): (Owner, bool),
    object: S::Word, plan: FieldPlan<'_>, miss: S::Miss) -> Option<S::Word> {
    match plan {
        FieldPlan::Ops(ops) => {
            let at = frame.receiver_record(stage, owner, object, readable, miss)?;
            let records = frame.object_records(stage);
            run_owned(stage, frame, (owner, !readable), ops, (at, records), miss)
        }
        FieldPlan::Arms(list) => {
            let at = frame.receiver_record(stage, owner, object, readable, miss)?;
            for held in list.iter().filter_map(|arm| arm.holds) { frame.hold_prototype(stage, held, miss)?; }
            let records = frame.object_records(stage);
            let proven = frame.proven_shape(stage, owner);
            if let Some(arm) = proven.and_then(|shape| list.iter().find(|arm| arm.shapes.contains(&shape))) {
                return ic::run(stage, arm.ops, at, records, miss);
            }
            let shape = ic::shape_word(stage, &at);
            arms(stage, list, Reached { shape: Some(shape), ..at }, records, miss)
        }
        FieldPlan::Inherited { shape, holder, constants } => {
            let at = frame.receiver_record(stage, owner, object, readable, miss)?;
            guard_receiver_shape(stage, frame, (owner, !readable), &at, shape, miss)?;
            frame.hold_prototype(stage, holder, miss)?;
            let from = Reached { record: at.record, shape: None, inline: None };
            ic::run(stage, &[IcOp::PrototypeConstants(constants)], from, None, miss)
        }
        FieldPlan::Readable(inner) => read_with(stage, frame, (owner, true), object, *inner, miss),
        FieldPlan::Probed { fast, atom, site } => frame.probed(stage, owner, object, (atom, site), miss,
            |stage, frame, miss| read_with(stage, frame, (owner, readable), object, *fast, miss)),
        FieldPlan::Known => frame.known_value(stage, owner),
        FieldPlan::Chain(links) => chain(stage, frame, (owner, readable), object, links, miss),
        FieldPlan::Host { atom, site } => frame.field_probe(stage, owner, object, atom, site, miss),
    }
}

/// Continues where `owner`'s object has exact shape `shape`: a proof the frame holds emits
/// nothing, else the guard is made and recorded. `direct` where the receiver is a direct object.
pub(super) fn guard_receiver_shape<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, (owner, direct): (Owner, bool),
    at: &Reached<S::Word, S::Cond>, shape: u32, miss: S::Miss) -> Option<()> {
    if frame.proven_shape(stage, owner) == Some(shape) { return Some(()); }
    ic::guard_shape(stage, at, shape, miss)?;
    frame.prove_shape(stage, owner, shape, direct);
    Some(())
}

/// `ops` over the record `at` reached, where a leading shape guard is the owner's proof: its
/// own-slot load reads through the layout the frame kept, or proves and keeps it.
fn run_owned<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, (owner, direct): (Owner, bool), ops: &[IcOp<'_>],
    (at, records): (Reached<S::Word, S::Cond>, Option<S::Word>), miss: S::Miss) -> Option<S::Word> {
    let [IcOp::GuardShape(shape), rest @ ..] = ops else { return ic::run(stage, ops, at, records, miss) };
    guard_receiver_shape(stage, frame, (owner, direct), &at, *shape, miss)?;
    match rest {
        [IcOp::LoadSlot { slot, form, storage_len }] =>
            kept_slot(stage, frame, (owner, *shape), &at, (*slot, *form, *storage_len), miss),
        _ => ic::run(stage, rest, at, records, miss),
    }
}

/// The value at `slot` of `owner`'s object of exact `shape`, through the layout the frame kept
/// where it covers the slot, else the one this read proves and keeps.
fn kept_slot<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, (owner, shape): (Owner, u32),
    at: &Reached<S::Word, S::Cond>, (slot, form, storage_len): (u16, Form, usize), miss: S::Miss) -> Option<S::Word> {
    let layout = match frame.kept_layout(owner, shape).filter(|layout| usize::from(slot) < layout.storage_len) {
        Some(layout) => layout,
        None => {
            let layout = ic::slot_layout(stage, at, form, storage_len, miss)?;
            frame.keep_layout(owner, shape, layout);
            layout
        }
    };
    Some(read_slot(stage, layout, slot))
}

/// The first of `plans` whose shape set holds the record's shape reads it; the last is
/// the guard of the whole.
fn arms<S: Stage>(stage: &mut S, plans: &[Arm<'_>], at: Reached<S::Word, S::Cond>, records: Option<S::Word>,
    miss: S::Miss) -> Option<S::Word> {
    let (arm, rest) = plans.split_first()?;
    let exact = ic::shape_word(stage, &at);
    let member = stage.shape_in_site(exact, arm.shapes);
    if rest.is_empty() {
        stage.guard(member, miss)?;
        return ic::run(stage, arm.ops, at, records, miss);
    }
    stage.choose_leaving(member, |stage| ic::run(stage, arm.ops, at, records, miss),
        |stage| arms(stage, rest, at, records, miss))
}

/// A nested lookup: each link reads from the previous link's value, the first from `owner`'s
/// object and the rest from objects no register names.
fn chain<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, first: (Owner, bool), base: S::Word,
    links: &[FieldPlan<'_>], miss: S::Miss) -> Option<S::Word> {
    let mut object = base;
    let mut from = first;
    for link in links {
        object = read_with(stage, frame, from, object, *link, miss)?;
        from = (Owner::Temporary, true);
    }
    Some(object)
}

/// `GetField`: `base`'s value, as the loop's entry or the record of an OSR region already
/// holds it, else read through `lookup`'s plan.
#[inline(always)]
fn get_field<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, base: FieldBase, lookup: FieldLookup,
    miss: S::Miss) -> Option<S::Word> {
    if let Some(word) = frame.hoisted(stage, lookup) { return Some(word); }
    if let Some(word) = frame.entry_field(stage, lookup) { return Some(word); }
    let owner = Owner::of(base)?;
    let plans = frame.plans();
    let plan = plans.field_plan(lookup)?;
    let object = base_word(stage, frame, base, miss)?;
    read_with(stage, frame, (owner, false), object, plan, miss)
}

/// The property operations of `step`: the word each produces, or `Unresolved` where the
/// frame holds no plan or callout for it.
#[inline(always)]
pub(super) fn property_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let word = match operation {
        Operation::GetField { base, lookup } => get_field(stage, frame, base, lookup, miss),
        Operation::SetField { value, base, atom, site } => super::property_write::set_field(stage, frame, (value, base), (atom, site), miss),
        Operation::StoreField { base, value, fact, atom, site } =>
            super::property_write::store_word(stage, frame, base, (value, fact), (atom, site), miss),
        Operation::FieldGetHost { object, atom, site } => frame.field_probe(stage, Owner::Temporary, object, atom, site, miss),
        Operation::FieldAdd { object, atom, site, value } => frame.field_add(stage, object, atom, site, value, miss).map(|()| value),
        Operation::Delete { base, key, strict } => {
            let (base, key) = (frame.tagged(stage, base), frame.tagged(stage, key));
            frame.delete_property(stage, base, key, strict, miss)
        }
        Operation::IndexGetHost { base, key } => frame.index_get_host(stage, base, key, miss),
        _ => None,
    };
    word.map_or(Step::Unresolved, Step::Value)
}
