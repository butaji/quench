//! The plans a named field access runs under (`staged::property`): each link of a lookup's
//! sites, from what their caches proved (`NativeFieldLoad`), as the data the rows read. A
//! plan borrows the op lists, shape sets and constants its link was built from, so a
//! lowering reads them through `with_read_plan`, which owns them for the step.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use super::super::object_slots::{field_load_groups, NativeFieldLoad};
use quench_runtime::staged::ic::IcOp;
use quench_runtime::staged::property::{Arm, FieldPlan, PlanSource, PrototypeProof, StorePlan};
use quench_runtime::staged::Form;
use quench_runtime::typed_ir::Representation;

/// What one access's frame proved: the plan of its read and of its store.
pub(crate) struct LanePlans<'p> {
    pub(crate) read: Option<FieldPlan<'p>>,
    pub(crate) store: Option<StorePlan<'p>>,
}

impl PlanSource for LanePlans<'_> {
    fn field_plan(&self, _lookup: FieldLookup) -> Option<FieldPlan<'_>> { self.read }
    fn store_plan(&self, _site: u16) -> Option<StorePlan<'_>> { self.store }
}

impl LanePlans<'static> {
    /// The plans of a step that reads or stores no named field of its own.
    pub(crate) const NONE: Self = Self { read: None, store: None };
}

/// One link of a lookup: the site it reads through and what its cache proved.
pub(crate) struct LinkInput {
    pub(crate) atom: Atom,
    pub(crate) site: u16,
    pub(crate) loads: Vec<NativeFieldLoad>,
    /// Whether a miss of the link's fast read asks the VM's caches before it leaves.
    pub(crate) probed: bool,
}

type Group = (NativeFieldLoad, Vec<u32>);

/// How a link's loads read natively.
enum Fast {
    /// The VM's own read: no loads, or one the layout does not admit.
    Host,
    /// One shape, which the owner's proofs keep.
    Single(NativeFieldLoad),
    /// Several shapes as layout families (`field_load_groups`).
    Family(Vec<Group>),
}

impl Fast {
    fn of(loads: &[NativeFieldLoad]) -> Self {
        match loads {
            [] => Self::Host,
            [load] if load.holder.is_none() && !load.valid() => Self::Host,
            [load] => Self::Single(*load),
            loads => match field_load_groups(loads) {
                Some(groups) if groups.iter().all(|(load, _)| load.holder.is_some() || load.valid()) => Self::Family(groups),
                _ => Self::Host,
            },
        }
    }

    /// The loads the plan reads, one per op list.
    fn loads(&self) -> Vec<NativeFieldLoad> {
        match self {
            Self::Host => Vec::new(),
            Self::Single(load) => vec![*load],
            Self::Family(groups) => groups.iter().map(|(load, _)| *load).collect(),
        }
    }

    /// Whether the receiver may be a function's record: several shapes, or a shape a function had.
    fn readable(&self) -> bool {
        match self {
            Self::Host => false,
            Self::Single(load) => load.function_shape,
            Self::Family(_) => true,
        }
    }
}

/// Whether a link's value comes from its loads' native reads, as the VM's own read does not.
pub(crate) fn reads_natively(loads: &[NativeFieldLoad]) -> bool {
    !matches!(Fast::of(loads), Fast::Host)
}

/// The values a holder load folds as constants (its prototypes' links and bits), where the
/// division folds Stable facts and the cache proved them.
fn inherited_pairs(load: &NativeFieldLoad, folds_stable: bool) -> Vec<(u32, u64)> {
    load.inherited.filter(|_| folds_stable).map(|constant|
        constant.values().iter().map(|(link, value)| (*link, value.to_bits())).collect()).unwrap_or_default()
}

/// Registers the heap values a folding division's inherited loads name with the lowering's
/// roots, which the publication keeps alive (`FoldedRoots::embed_heap_identity`), and the chains
/// they were read through with its chains, whose prototypes' layouts it holds
/// (`StableFacts::chains`); `inherited_pairs` then names the values by their bits.
pub(crate) fn embed_inherited(links: &[LinkInput], stable: &mut super::stable::StableUses) {
    for constant in links.iter().flat_map(|link| Fast::of(&link.loads).loads()).filter_map(|load| load.inherited) {
        for (_, value) in constant.values() {
            stable.roots.embed_heap_identity(*value);
        }
        stable.chains.extend(constant.chains());
    }
}

/// The layout id a single-shape own load was formed to (`NativeFieldLoad::formed`), where it names
/// the load's shape: one compare of the record's word then proves the shape and the storage form
/// (`IcOp::GuardLayout`), and the slot's form is the id's (`ic::form_of_layout`).
fn formed_id(load: &NativeFieldLoad) -> Option<u32> {
    load.formed.filter(|&id| load.holder.is_none() && (id as usize) < quench_runtime::heap::layouts::LAYOUT_CAPACITY
        && quench_runtime::staged::ic::layout_shape(id) == load.shape)
}

/// The layout id a holder read guards its prototype by (`NativeFieldLoad::holder_id`), where it is
/// a real id of the holder's exact shape: one compare of the prototype's own word then proves that
/// shape and the storage form, as `formed_id` does for a receiver. Id 0 names no holder.
fn holder_id(load: &NativeFieldLoad) -> Option<u32> {
    let holder = load.holder?;
    load.holder_id.filter(|&id| id != 0 && (id as usize) < quench_runtime::heap::layouts::LAYOUT_CAPACITY
        && quench_runtime::staged::ic::layout_shape(id) == holder)
}

/// The ops that read `load` once the receiver's shape is proved: its own slot, a prototype's
/// constant, or the slot of the prototype whose exact shape the load names. `formed` is the
/// layout id a single-shape load was guarded by (`formed_id`), whose form the slot is read in;
/// a family's shapes are guarded one by one, so their slots keep the form the site observed.
fn load_ops<'a>(load: &NativeFieldLoad, pairs: &'a [(u32, u64)], formed: Option<u32>) -> Vec<IcOp<'a>> {
    match load.holder {
        None => vec![IcOp::LoadSlot { slot: load.slot,
            form: formed.map_or_else(|| Form::of(load.storage, load.storage_len), quench_runtime::staged::ic::form_of_layout),
            storage_len: load.storage_len, rep: if load.raw_double { Representation::Float64 } else { Representation::Tagged } }],
        Some(_) if !pairs.is_empty() => vec![IcOp::PrototypeConstants(pairs)],
        Some(holder) => {
            // The prototype's layout id proves its shape (and storage form) in one compare where
            // the entry kept it (`holder_id`); else its shape alone is compared.
            let guard = match holder_id(load) {
                Some(id) => IcOp::GuardLayout(id),
                None => IcOp::GuardShape(holder),
            };
            vec![IcOp::Prototype, guard, IcOp::LoadPrototypeSlot(load.slot)]
        }
    }
}

/// The plan of a single shape's load: the receiver's shape guard, then its ops; a folded
/// prototype constant is its own plan.
fn single_plan<'a>(load: &NativeFieldLoad, ops: &'a [IcOp<'a>], pairs: &'a [(u32, u64)]) -> FieldPlan<'a> {
    match load.holder {
        Some(holder) if !pairs.is_empty() => FieldPlan::Inherited { shape: load.shape, constants: pairs,
            proof: PrototypeProof::Watched { holder } },
        _ => FieldPlan::Ops(ops),
    }
}

/// The op lists of each link: a single shape's after its own guard (`GuardShape` first), a
/// family's per group.
fn link_ops<'a>(fast: &Fast, pairs: &'a [Vec<(u32, u64)>]) -> Vec<Vec<IcOp<'a>>> {
    let loads = fast.loads();
    let single = matches!(fast, Fast::Single(_));
    let lists = loads.iter().zip(pairs).map(|(load, pairs)| load_ops(load, pairs, if single { formed_id(load) } else { None }));
    match fast {
        Fast::Single(load) => {
            // An own read whose receivers' storage form the site observed guards the whole word.
            let guard = match formed_id(load) {
                Some(id) => IcOp::GuardLayout(id),
                None => IcOp::GuardShape(load.shape),
            };
            lists.map(|ops| std::iter::once(guard).chain(ops).collect()).collect()
        }
        _ => lists.collect(),
    }
}

/// The arms of a family: each group's shapes, ops and the prototype whose constants it folds.
fn family_arms<'a>(groups: &'a [Group], ops: &'a [Vec<IcOp<'a>>], pairs: &'a [Vec<(u32, u64)>]) -> Vec<Arm<'a>> {
    groups.iter().zip(ops).zip(pairs).map(|(((load, shapes), ops), pairs)| Arm { shapes, ops,
        holds: load.holder.filter(|_| !pairs.is_empty()) }).collect()
}

/// The plans of a lookup's `links`, one per site and chained where there are several, for
/// `then`. A division that folds Stable facts takes a holder's inherited constants; where the
/// first field's value is `known`, its link is that value.
pub(crate) fn with_read_plan<R>(links: &[LinkInput], (folds_stable, known): (bool, bool),
    then: impl FnOnce(&LanePlans<'_>) -> R) -> R {
    let fasts: Vec<Fast> = links.iter().map(|link| Fast::of(&link.loads)).collect();
    let pairs: Vec<Vec<Vec<(u32, u64)>>> = fasts.iter().map(|fast|
        fast.loads().iter().map(|load| inherited_pairs(load, folds_stable)).collect()).collect();
    let ops: Vec<Vec<Vec<IcOp<'_>>>> = fasts.iter().zip(&pairs).map(|(fast, pairs)| link_ops(fast, pairs)).collect();
    let arms: Vec<Vec<Arm<'_>>> = fasts.iter().zip(&ops).zip(&pairs).map(|((fast, ops), pairs)| match fast {
        Fast::Family(groups) => family_arms(groups, ops, pairs),
        _ => Vec::new(),
    }).collect();
    let fast_plans: Vec<FieldPlan<'_>> = links.iter().enumerate().map(|(index, link)| match &fasts[index] {
        Fast::Host => FieldPlan::Host { atom: link.atom, site: link.site },
        Fast::Single(load) => single_plan(load, &ops[index][0], &pairs[index][0]),
        Fast::Family(_) => FieldPlan::Arms(&arms[index]),
    }).collect();
    let readable: Vec<FieldPlan<'_>> = fasts.iter().zip(&fast_plans).map(|(fast, plan)|
        if fast.readable() { FieldPlan::Readable(plan) } else { *plan }).collect();
    let mut plans: Vec<FieldPlan<'_>> = links.iter().zip(&fasts).zip(&readable).map(|((link, fast), plan)| match fast {
        Fast::Host => *plan,
        _ if link.probed => FieldPlan::Probed { fast: plan, atom: link.atom, site: link.site },
        _ => *plan,
    }).collect();
    if let Some(first) = plans.first_mut().filter(|_| known) { *first = FieldPlan::Known; }
    let read = match plans.as_slice() {
        [] => None,
        [only] => Some(*only),
        links => Some(FieldPlan::Chain(links)),
    };
    then(&LanePlans { read, store: None })
}
