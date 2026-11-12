//! The named-property family as staged rows: `GetField` in every form (own slot, prototype
//! slot, prototype constant, hinted holder, string method, polymorphic arms, nested chains),
//! `SetField` (own store, adding store, VM store), `Delete` and `in`. A site's proved plan is
//! static data (`FieldPlan`, `StorePlan`); each row runs it once over a stage, so the
//! interpreter (`Concrete`) and every specializer (`Emit`) read one definition of each read and
//! store. Whatever the plan cannot prove is an explicit `PropertyFrame` call-out to the VM's
//! helper, never a second copy of the lookup.

// The plans are built by the tiers' site tables as they move onto these rows (the lane
// specializer's `NativeFieldLoad` families, the interpreter's field caches); until then only
// the host callouts are constructed in production code.
#![allow(dead_code)]

use super::ic::{self, IcOp, Reached};
use super::object::object_record_at;
use super::{holder, value, Frame, NumericFrame, NumericStage, Operation, Stage, Step};
use crate::bytecode::{FieldBase, FieldLookup};

/// One shape family of a polymorphic read: the sorted exact shapes that take `ops`, which
/// read from the record the shapes already matched.
#[derive(Clone, Copy)]
pub(crate) struct Arm<'a> {
    pub(crate) shapes: &'a [u32],
    pub(crate) ops: &'a [IcOp<'a>],
}

/// How a `GetField` site reads, as the proof its tier holds of the site.
#[derive(Clone, Copy)]
pub(crate) enum FieldPlan<'a> {
    /// The ops of `staged::ic` over the receiver's record: own slot, prototype slot, or a
    /// prototype constant.
    Ops(&'a [IcOp<'a>]),
    /// The first arm whose shape set holds the receiver's exact shape reads; none leaves.
    Arms(&'a [Arm<'a>]),
    /// A method read through a prototype hint (`holder::holder_read`).
    Holder { shape: u32, link: u32, watch: u64, value: u64 },
    /// A method read of a string receiver (`holder::string_holder_read`).
    StringHolder { watch: u64, value: u64 },
    /// A nested lookup `a.b.c`: each plan reads from the value the previous one produced.
    Chain(&'a [FieldPlan<'a>]),
    /// The VM's own cached read of `atom` through cache `site`: a dictionary object, a
    /// deeper holder, an accessor, a proxy.
    Host { atom: u32, site: u16 },
}

/// How a named store site writes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum StorePlan {
    /// An own data field of an object whose exact shape is `shape`, at `slot` (`store::own_store`).
    Own { shape: u32, slot: u16 },
    /// A store that may add the property or change the shape: the VM's adding entry.
    Add,
    /// A setter, a read-only or dictionary property, a proxy: the VM's store.
    Host,
}

/// The plans a frame's tier proved, by site. A frame with none answers `None`, and the
/// operation is left to the host path.
pub(crate) trait PlanSource {
    /// The read plan of `lookup`.
    fn field_plan(&self, lookup: FieldLookup) -> Option<FieldPlan<'_>>;
    /// The store plan of cache site `site`.
    fn store_plan(&self, site: u16) -> Option<StorePlan>;
}

impl<P: PlanSource> PlanSource for &P {
    fn field_plan(&self, lookup: FieldLookup) -> Option<FieldPlan<'_>> { (**self).field_plan(lookup) }
    fn store_plan(&self, site: u16) -> Option<StorePlan> { (**self).store_plan(site) }
}

/// The plans of a frame that proved none.
#[derive(Clone, Copy, Default)]
pub(crate) struct NoPlans;

impl PlanSource for NoPlans {
    fn field_plan(&self, _lookup: FieldLookup) -> Option<FieldPlan<'_>> { None }
    fn store_plan(&self, _site: u16) -> Option<StorePlan> { None }
}

/// A frame's property callouts. Every default declines, leaving the operation to the host
/// path; a miss edge of the stage is each callout's failure.
pub(crate) trait PropertyFrame<S: Stage>: Frame<S> {
    /// The handle to the plans, copied out so a row holds a plan while it calls the frame.
    type Plans: PlanSource + Copy;

    /// The plans this frame's tier proved.
    fn plans(&self) -> Self::Plans;

    /// The base of the object record table, which prototype reads and stores address.
    #[inline(always)]
    fn object_records(&mut self, _stage: &mut S) -> Option<S::Word> { None }

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

    /// `delete base[key]` where `strict` throws on a refusal: the boolean word, or a leave
    /// at `miss`.
    #[inline(always)]
    fn delete_property(&mut self, _stage: &mut S, _base: S::Word, _key: S::Word, _strict: bool, _miss: S::Miss) -> Option<S::Word> { None }

    /// `key in base`: the boolean word, or a leave at `miss`.
    #[inline(always)]
    fn has_property(&mut self, _stage: &mut S, _key: S::Word, _base: S::Word, _miss: S::Miss) -> Option<S::Word> { None }

    /// `base[key]` where no dense or typed element read answers it: a hole read through the
    /// prototype chain, an index past a typed array, a string's character, a keyed own read.
    #[inline(always)]
    fn index_get_host(&mut self, _stage: &mut S, _base: S::Word, _key: S::Word, _miss: S::Miss) -> Option<S::Word> { None }
}

/// The record and id of the direct object `object`; anything else leaves at `miss`.
#[inline(always)]
pub(super) fn receiver<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, object: S::Word,
    miss: S::Miss) -> Option<(Reached<S::Word, S::Cond>, S::Word)> {
    let records = frame.object_records(stage)?;
    let direct = value::is_direct_object(stage, object);
    stage.guard(direct, miss)?;
    let id = stage.and_imm(object, u64::from(u32::MAX));
    Some((Reached { record: object_record_at(stage, records, id), shape: None, inline: None }, id))
}

/// The base of a field access: the activation's bound `this`, or a register's tagged word;
/// a local or nested base is the tier's own.
#[inline(always)]
pub(super) fn base_word<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, base: FieldBase, miss: S::Miss) -> Option<S::Word> {
    if base == FieldBase::THIS {
        return super::steps::bound_this(stage, frame, miss);
    }
    Some(frame.tagged(stage, base.register_index()?))
}

/// The value `plan` reads from `object`, or a leave at `miss`.
#[inline]
pub(crate) fn read_plan<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, object: S::Word, plan: FieldPlan<'_>,
    miss: S::Miss) -> Option<S::Word> {
    match plan {
        FieldPlan::Ops(ops) => {
            let (at, _) = receiver(stage, frame, object, miss)?;
            let records = frame.object_records(stage);
            ic::run(stage, ops, at, records, miss)
        }
        FieldPlan::Arms(plans) => {
            let (at, _) = receiver(stage, frame, object, miss)?;
            let shape = ic::shape_word(stage, &at);
            let records = frame.object_records(stage);
            arms(stage, plans, Reached { shape: Some(shape), ..at }, records, miss)
        }
        FieldPlan::Holder { shape, link, watch, value } => {
            let (at, _) = receiver(stage, frame, object, miss)?;
            holder::holder_read(stage, at.record, shape, link, watch, value, miss)
        }
        FieldPlan::StringHolder { watch, value } => holder::string_holder_read(stage, object, watch, value, miss),
        FieldPlan::Chain(links) => chain(stage, frame, object, links, miss),
        FieldPlan::Host { atom, site } => frame.field_get_host(stage, object, atom, site, miss),
    }
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

/// A nested lookup: each link reads from the previous link's value.
fn chain<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, base: S::Word, links: &[FieldPlan<'_>],
    miss: S::Miss) -> Option<S::Word> {
    let mut object = base;
    for link in links {
        object = read_plan(stage, frame, object, *link, miss)?;
    }
    Some(object)
}

/// `GetField`: `base`'s value read through `lookup`'s plan.
#[inline(always)]
fn get_field<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, base: FieldBase, lookup: FieldLookup,
    miss: S::Miss) -> Option<S::Word> {
    let plans = frame.plans();
    let plan = plans.field_plan(lookup)?;
    let object = base_word(stage, frame, base, miss)?;
    read_plan(stage, frame, object, plan, miss)
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
        Operation::FieldGetHost { object, atom, site } => frame.field_get_host(stage, object, atom, site, miss),
        Operation::FieldAdd { object, atom, site, value } => frame.field_add(stage, object, atom, site, value, miss).map(|()| value),
        Operation::Delete { base, key, strict } => {
            let (base, key) = (frame.tagged(stage, base), frame.tagged(stage, key));
            frame.delete_property(stage, base, key, strict, miss)
        }
        Operation::HasProperty { key, base } => {
            let (key, base) = (frame.tagged(stage, key), frame.tagged(stage, base));
            frame.has_property(stage, key, base, miss)
        }
        Operation::IndexGetHost { base, key } => frame.index_get_host(stage, base, key, miss),
        _ => None,
    };
    word.map_or(Step::Unresolved, Step::Value)
}
