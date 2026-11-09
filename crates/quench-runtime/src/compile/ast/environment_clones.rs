//! A function's environment clone plans (`CloneEnv`) as its compiler makes them.
use super::FunctionCompiler;
use crate::bytecode::Op;
use rustc_hash::FxHashSet;

#[derive(Default)]
pub(in crate::compile) struct ClonePlans {
    /// Each plan's fresh slots.
    pub(in crate::compile) slots: Vec<Vec<u16>>,
    /// For each plan, the lexical scopes active at its site, outermost first, as indices of
    /// `scope_slots`; see `per_site_environment_clones`.
    scopes: Vec<Vec<u32>>,
    /// The sorted local slots of a lexical scope's bindings, once per scope (`lexical_slots`).
    scope_slots: Vec<Box<[u16]>>,
    /// Plans that open a class evaluation; their slots are never reset.
    pub(in crate::compile) class: FxHashSet<usize>,
    /// The plans `clone_environment_slots` made, by the hash of their slots and scopes, so a
    /// block entered again with the same scopes reuses its plan without comparing every plan.
    lookup: hashbrown::HashTable<u32>,
}

impl FunctionCompiler<'_, '_> {
    pub(in crate::compile) fn clone_environment_slots(&mut self, mut slots: Vec<u16>) {
        slots.sort_unstable();
        slots.dedup();
        if slots.is_empty() {
            return;
        }
        let scopes = self.active_lexical_slots();
        let plan_hash = |slots: &[u16], scopes: &[u32]| {
            use std::hash::{Hash, Hasher};
            let mut hasher = rustc_hash::FxHasher::default();
            (slots, scopes).hash(&mut hasher);
            hasher.finish()
        };
        let hash = plan_hash(&slots, &scopes);
        let (clones, clone_scopes) = (&self.clone_plans.slots, &self.clone_plans.scopes);
        let found = self.clone_plans.lookup.find(hash, |&index| {
            clones[index as usize] == slots && clone_scopes[index as usize] == scopes
        });
        let index = match found {
            Some(&index) => index as usize,
            None => {
                let index = self.push_environment_clone(slots, scopes);
                let (clones, clone_scopes) = (&self.clone_plans.slots, &self.clone_plans.scopes);
                self.clone_plans.lookup.insert_unique(hash, index as u32, |&index| {
                    plan_hash(&clones[index as usize], &clone_scopes[index as usize])
                });
                index
            }
        };
        self.emit(Op::CloneEnv, 0, 0, 0, index as u32);
    }

    /// Adds a clone plan for `slots`, sited where the lexical scopes `scopes` are active,
    /// and returns its index.
    pub(in crate::compile) fn push_environment_clone(&mut self, slots: Vec<u16>, scopes: Vec<u32>) -> usize {
        self.clone_plans.slots.push(slots);
        self.clone_plans.scopes.push(scopes);
        self.clone_plans.slots.len() - 1
    }

    /// The lexical scopes active at the current position, outermost first (`lexical_slots`).
    pub(in crate::compile) fn active_lexical_slots(&mut self) -> Vec<u32> {
        (0..self.lexical_scopes.len()).map(|index| self.lexical_slots(index)).collect()
    }

    /// The sorted local slots of lexical scope `index`'s bindings.
    pub(in crate::compile) fn lexical_scope_slots_of(&mut self, index: usize) -> &[u16] {
        let id = self.lexical_slots(index);
        &self.clone_plans.scope_slots[id as usize]
    }

    /// The index in `ClonePlans::scope_slots` of lexical scope `index`'s slots: made once per
    /// scope and shared by every plan sited in it, until a target it found unslotted gains a
    /// slot.
    fn lexical_slots(&mut self, index: usize) -> u32 {
        if let Some((epoch, id)) = self.lexical_scopes[index].slots
            && epoch == self.capture_memo.unslotted_epoch
        {
            return id;
        }
        let mut slots = Vec::new();
        for target in self.lexical_scopes[index].bindings.values() {
            match self.local_slots.get(target) {
                Some(slot) => slots.push(*slot),
                None => {
                    self.capture_memo.unslotted_targets.insert_growing(*target as usize);
                }
            }
        }
        slots.sort_unstable();
        slots.dedup();
        let id = self.clone_plans.scope_slots.len() as u32;
        self.clone_plans.scope_slots.push(slots.into());
        self.lexical_scopes[index].slots = Some((self.capture_memo.unslotted_epoch, id));
        id
    }

    /// The clone plans the function runs. A clone owns its fresh slots; every
    /// other slot is shared with the slot's owner. A slot some clone owns but
    /// whose scope is not active at another clone's site (an inner loop's or
    /// a loop body's binding, seen from the loop's next iteration) is out of
    /// reach there, so that clone resets it: sharing or copying it would keep
    /// the previous iteration's environment, and through it every earlier
    /// one, alive for as long as the activation runs. A class's slots (its
    /// private names, homes and field initializers) are no lexical scope's
    /// bindings, yet remain reachable from nested classes and closures created
    /// while the class evaluates, so class clones' slots are never reset.
    /// The owned slots are stored once; each plan lists only the owned slots of
    /// the scopes active at its site, which it keeps.
    pub(in crate::compile) fn per_site_environment_clones(&mut self) -> crate::bytecode::EnvironmentClones {
        let plans = std::mem::take(&mut self.clone_plans);
        let mut owned = plans
            .slots
            .iter()
            .enumerate()
            .filter(|(plan, _)| !plans.class.contains(plan))
            .flat_map(|(_, slots)| slots)
            .copied()
            .collect::<Vec<_>>();
        owned.sort_unstable();
        owned.dedup();
        let owned_in = owned_slots_of_scopes(&plans.scope_slots, &owned, self.locals.len());
        let plans = plans
            .slots
            .into_iter()
            .zip(plans.scopes)
            .map(|(fresh, active)| {
                let kept = kept_slots(&active, &fresh, &owned_in);
                crate::bytecode::EnvironmentClone { fresh, kept }
            })
            .collect::<Vec<_>>();
        crate::bytecode::EnvironmentClones::new(owned, plans)
    }
}

/// Of each lexical scope's slots, those in the sorted `owned`: once per scope, not per plan.
fn owned_slots_of_scopes(scope_slots: &[Box<[u16]>], owned: &[u16], slot_count: usize) -> Vec<Vec<u16>> {
    let mut is_owned = crate::bitset::BitSet::new(slot_count);
    owned.iter().for_each(|slot| {
        is_owned.insert(usize::from(*slot));
    });
    scope_slots
        .iter()
        .map(|slots| slots.iter().copied().filter(|slot| is_owned.contains(usize::from(*slot))).collect())
        .collect()
}

/// The owned slots of the scopes `active` at a plan's site that it does not make `fresh`.
fn kept_slots(active: &[u32], fresh: &[u16], owned_in: &[Vec<u16>]) -> Vec<u16> {
    let mut kept: Vec<u16> = active
        .iter()
        .flat_map(|scope| &owned_in[*scope as usize])
        .copied()
        .filter(|slot| fresh.binary_search(slot).is_err())
        .collect();
    kept.sort_unstable();
    kept.dedup();
    kept
}
