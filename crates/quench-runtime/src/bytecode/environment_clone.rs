//! The plans of a function's CloneEnv instructions.

use crate::heap::ThinBoxSlice;

/// A CloneEnv plan. The clone owns the `fresh` slots, copied from the source environment,
/// and resets the function's owned slots (`EnvironmentClones::owned`) it neither makes fresh
/// nor `kept`: block bindings out of scope at the clone's site, whose next scope entry
/// initializes them again. Every other slot is shared with its owner. `kept` holds the owned
/// slots of the scopes active at the site. Both lists are sorted local slots.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EnvironmentClone {
    pub fresh: Vec<u16>,
    pub kept: Vec<u16>,
}

/// A function's CloneEnv plans and, once for all of them, the sorted slots some plan of a
/// loop owns, so each plan costs the slots of its own site rather than every owned slot.
/// A function without plans allocates nothing.
#[derive(Clone, Debug, Default)]
pub struct EnvironmentClones(Option<Box<ClonePlans>>);

#[derive(Clone, Debug)]
struct ClonePlans {
    owned: ThinBoxSlice<u16>,
    plans: ThinBoxSlice<EnvironmentClone>,
}

impl EnvironmentClones {
    pub fn new(owned: Vec<u16>, plans: Vec<EnvironmentClone>) -> Self {
        Self((!plans.is_empty()).then(|| Box::new(ClonePlans { owned: owned.into(), plans: plans.into() })))
    }

    /// The sorted slots some loop plan owns, which a plan resets unless it makes them fresh or
    /// keeps them.
    pub fn owned(&self) -> &[u16] {
        self.0.as_deref().map_or(&[], |clones| &clones.owned)
    }

    /// Whether some plan makes `slot` fresh or may reset it.
    pub(crate) fn mentions(&self, slot: u16) -> bool {
        self.owned().binary_search(&slot).is_ok() || self.iter().any(|plan| plan.fresh.contains(&slot))
    }

    /// Every slot the plans at `indices` make fresh or reset: an owned slot is reset by a plan
    /// unless that plan spares it (makes it fresh or keeps it), so it is named when fewer
    /// plans spare it than there are plans. Linear in the plans' own lists and `owned`.
    pub(crate) fn named_slots(&self, indices: impl Iterator<Item = usize>) -> Vec<u16> {
        let mut referenced = crate::bitset::BitSet::new(self.len());
        indices.for_each(|index| {
            referenced.checked_insert(index);
        });
        let owned = self.owned();
        let mut spared = vec![0usize; owned.len()];
        let mut plans = 0;
        let mut named = Vec::new();
        for plan in referenced.iter().map(|index| &self[index]) {
            plans += 1;
            named.extend_from_slice(&plan.fresh);
            for slot in plan.fresh.iter().chain(&plan.kept) {
                if let Ok(position) = owned.binary_search(slot) {
                    spared[position] += 1;
                }
            }
        }
        named.extend(owned.iter().zip(spared).filter(|(_, spared)| *spared < plans).map(|(slot, _)| *slot));
        named
    }

    /// Whether every list is ascending local slots below `locals`, and each plan keeps only
    /// owned slots it does not make fresh.
    pub(super) fn valid(&self, locals: u16) -> bool {
        let ascending_locals = |slots: &[u16]| {
            slots.iter().all(|slot| *slot < locals) && slots.is_sorted_by(|left, right| left < right)
        };
        ascending_locals(self.owned())
            && self.iter().all(|plan| {
                ascending_locals(&plan.fresh)
                    && ascending_locals(&plan.kept)
                    && plan.kept.iter().all(|slot| {
                        plan.fresh.binary_search(slot).is_err() && self.owned().binary_search(slot).is_ok()
                    })
            })
    }

    pub(super) fn write(&self, out: &mut super::binary::BinaryWriter) {
        let slots = |out: &mut super::binary::BinaryWriter, slots: &[u16]| {
            out.u32(slots.len() as u32);
            slots.iter().for_each(|slot| out.u16(*slot));
        };
        slots(out, self.owned());
        out.u32(self.len() as u32);
        for plan in self.iter() {
            slots(out, &plan.fresh);
            slots(out, &plan.kept);
        }
    }

    pub(super) fn read(input: &mut super::binary::BinaryReader<'_>) -> Result<Self, String> {
        let owned = input.list(|input| input.u16())?;
        let plans = input.list(|input| {
            Ok(EnvironmentClone { fresh: input.list(|input| input.u16())?, kept: input.list(|input| input.u16())? })
        })?;
        Ok(Self::new(owned, plans))
    }

    /// The slots `plan`, one of these plans, resets.
    pub(crate) fn resets<'a>(&'a self, plan: &'a EnvironmentClone) -> Resets<'a> {
        Resets { owned: self.owned(), fresh: &plan.fresh, kept: &plan.kept }
    }
}

impl From<Vec<EnvironmentClone>> for EnvironmentClones {
    fn from(plans: Vec<EnvironmentClone>) -> Self {
        Self::new(Vec::new(), plans)
    }
}

impl std::ops::Deref for EnvironmentClones {
    type Target = [EnvironmentClone];

    fn deref(&self) -> &[EnvironmentClone] {
        self.0.as_deref().map_or(&[], |clones| &clones.plans)
    }
}

impl std::ops::DerefMut for EnvironmentClones {
    fn deref_mut(&mut self) -> &mut [EnvironmentClone] {
        self.0.as_deref_mut().map_or(&mut [], |clones| &mut clones.plans)
    }
}

/// The slots one clone resets: of `owned`, those neither `fresh` nor `kept`; all sorted.
#[derive(Clone, Copy)]
pub(crate) struct Resets<'a> {
    owned: &'a [u16],
    fresh: &'a [u16],
    kept: &'a [u16],
}

impl<'a> Resets<'a> {
    /// Exactly the sorted `slots`.
    pub(crate) fn listed(slots: &'a [u16]) -> Self {
        Self { owned: slots, fresh: &[], kept: &[] }
    }

    pub(crate) fn contains(self, slot: u16) -> bool {
        self.owned.binary_search(&slot).is_ok() && !self.spared(slot)
    }

    /// Ascending.
    pub(crate) fn iter(self) -> impl Iterator<Item = u16> + Clone + 'a {
        self.owned.iter().copied().filter(move |slot| !self.spared(*slot))
    }

    /// Whether the plan makes `slot` fresh or keeps it.
    fn spared(self, slot: u16) -> bool {
        self.fresh.binary_search(&slot).is_ok() || self.kept.binary_search(&slot).is_ok()
    }
}
