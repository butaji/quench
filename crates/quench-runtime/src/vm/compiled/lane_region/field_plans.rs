//! The plans a named field access runs under (`staged::property`): each link of a lookup's
//! sites, from what their caches proved (`NativeFieldLoad`), as the data the rows read. A
//! plan borrows the op lists, shape sets and constants its link was built from, so a
//! lowering reads them through `with_read_plan`, which owns them for the step.

use super::*;
use super::super::object_slots::{field_load_groups, NativeFieldLoad};
use crate::staged::ic::IcOp;
use crate::staged::property::{Arm, FieldPlan, PlanSource, StorePlan};
use crate::staged::Form;
use crate::staged::object::Rep;

/// What one access's frame proved: the plan of its read and of its store.
pub(super) struct LanePlans<'p> {
    pub(super) read: Option<FieldPlan<'p>>,
    pub(super) store: Option<StorePlan<'p>>,
}

impl PlanSource for LanePlans<'_> {
    fn field_plan(&self, _lookup: FieldLookup) -> Option<FieldPlan<'_>> { self.read }
    fn store_plan(&self, _site: u16) -> Option<StorePlan<'_>> { self.store }
}

impl LanePlans<'static> {
    /// The plans of a step that reads or stores no named field of its own.
    pub(super) const NONE: Self = Self { read: None, store: None };
}

/// One link of a lookup: the site it reads through and what its cache proved.
pub(super) struct LinkInput {
    pub(super) atom: Atom,
    pub(super) site: u16,
    pub(super) loads: Vec<NativeFieldLoad>,
    /// Whether a miss of the link's fast read asks the VM's caches before it leaves.
    pub(super) probed: bool,
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
pub(super) fn reads_natively(loads: &[NativeFieldLoad]) -> bool {
    !matches!(Fast::of(loads), Fast::Host)
}

/// The values a holder load folds as constants (its prototypes' links and bits), where the
/// division folds Stable facts and the cache proved them.
fn inherited_pairs(load: &NativeFieldLoad, folds_stable: bool) -> Vec<(u32, u64)> {
    load.inherited.filter(|_| folds_stable).map(|constant|
        constant.values().iter().map(|(link, value)| (*link, value.to_bits())).collect()).unwrap_or_default()
}

/// Registers the heap values a folding division's inherited loads name with `roots`, which the
/// publication keeps alive (`FoldedRoots::embed_heap_identity`); `inherited_pairs` then names
/// them by their bits.
pub(super) fn embed_inherited(links: &[LinkInput], roots: &mut crate::folded_roots::FoldedRoots) {
    for load in links.iter().flat_map(|link| Fast::of(&link.loads).loads()) {
        for (_, value) in load.inherited.iter().flat_map(|constant| constant.values().iter()) {
            roots.embed_heap_identity(*value);
        }
    }
}

/// The ops that read `load` once the receiver's shape is proved: its own slot, a prototype's
/// constant, or the slot of the prototype whose exact shape the load names.
fn load_ops<'a>(load: &NativeFieldLoad, pairs: &'a [(u32, u64)]) -> Vec<IcOp<'a>> {
    match load.holder {
        None => vec![IcOp::LoadSlot { slot: load.slot, form: Form::of(load.storage, load.storage_len), storage_len: load.storage_len,
            rep: if load.raw_double { Rep::Float64 } else { Rep::Tagged } }],
        Some(_) if !pairs.is_empty() => vec![IcOp::PrototypeConstants(pairs)],
        Some(holder) => vec![IcOp::Prototype, IcOp::GuardShape(holder), IcOp::LoadPrototypeSlot(load.slot)],
    }
}

/// The plan of a single shape's load: the receiver's shape guard, then its ops; a folded
/// prototype constant is its own plan.
fn single_plan<'a>(load: &NativeFieldLoad, ops: &'a [IcOp<'a>], pairs: &'a [(u32, u64)]) -> FieldPlan<'a> {
    match load.holder {
        Some(holder) if !pairs.is_empty() => FieldPlan::Inherited { shape: load.shape, holder, constants: pairs },
        _ => FieldPlan::Ops(ops),
    }
}

/// The op lists of each link: a single shape's after its own guard (`GuardShape` first), a
/// family's per group.
fn link_ops<'a>(fast: &Fast, pairs: &'a [Vec<(u32, u64)>]) -> Vec<Vec<IcOp<'a>>> {
    let loads = fast.loads();
    let lists = loads.iter().zip(pairs).map(|(load, pairs)| load_ops(load, pairs));
    match fast {
        Fast::Single(load) => lists.map(|ops| std::iter::once(IcOp::GuardShape(load.shape)).chain(ops).collect()).collect(),
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
pub(super) fn with_read_plan<R>(links: &[LinkInput], (folds_stable, known): (bool, bool),
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
