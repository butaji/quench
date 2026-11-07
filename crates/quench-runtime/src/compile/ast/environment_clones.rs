//! A function's environment clone plans (`CloneEnv`) as its compiler makes them.
use super::FunctionCompiler;
use crate::bytecode::Op;
use rustc_hash::FxHashSet;

#[derive(Default)]
pub(in crate::compile) struct ClonePlans {
    /// Each plan's fresh slots.
    pub(in crate::compile) slots: Vec<Vec<u16>>,
    /// For each plan, the slots of every lexical scope active at its site; see
    /// `per_site_environment_clones`.
    scopes: Vec<Vec<u16>>,
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
        let plan_hash = |slots: &[u16], scopes: &[u16]| {
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

    /// Adds a clone plan for `slots`, sited where the lexical scopes owning
    /// `scopes` are active, and returns its index.
    pub(in crate::compile) fn push_environment_clone(&mut self, slots: Vec<u16>, scopes: Vec<u16>) -> usize {
        self.clone_plans.slots.push(slots);
        self.clone_plans.scopes.push(scopes);
        self.clone_plans.slots.len() - 1
    }

    /// The slots of every lexical scope active at the current position.
    pub(in crate::compile) fn active_lexical_slots(&self) -> Vec<u16> {
        let mut slots = self
            .lexical_scopes
            .iter()
            .flat_map(|scope| scope.bindings.values())
            .filter_map(|binding| self.local_slots.get(binding).copied())
            .collect::<Vec<_>>();
        slots.sort_unstable();
        slots.dedup();
        slots
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
    pub(in crate::compile) fn per_site_environment_clones(&mut self) -> Vec<crate::bytecode::EnvironmentClone> {
        // Ascending, so each plan's resets filter out already sorted.
        let mut owned = self
            .clone_plans
            .slots
            .iter()
            .enumerate()
            .filter(|(plan, _)| !self.clone_plans.class.contains(plan))
            .flat_map(|(_, slots)| slots)
            .copied()
            .collect::<Vec<_>>();
        owned.sort_unstable();
        owned.dedup();
        self.clone_plans.lookup.clear();
        let scopes = std::mem::take(&mut self.clone_plans.scopes);
        std::mem::take(&mut self.clone_plans.slots)
            .into_iter()
            .zip(scopes)
            .map(|(fresh, active)| {
                let reset = owned
                    .iter()
                    .copied()
                    .filter(|slot| {
                        active.binary_search(slot).is_err() && fresh.binary_search(slot).is_err()
                    })
                    .collect::<Vec<_>>();
                crate::bytecode::EnvironmentClone { fresh, reset }
            })
            .collect()
    }
}
