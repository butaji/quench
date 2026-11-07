//! A function's environment clone plans (`CloneEnv`) as its compiler makes them.
use super::FunctionCompiler;
use crate::bytecode::Op;
use rustc_hash::FxHashSet;
use std::rc::Rc;

#[derive(Default)]
pub(in crate::compile) struct ClonePlans {
    /// Each plan's fresh slots.
    pub(in crate::compile) slots: Vec<Vec<u16>>,
    /// For each plan, the slots of each lexical scope active at its site (`lexical_slots`),
    /// shared with every plan sited in the same scope; see `per_site_environment_clones`.
    scopes: Vec<Vec<Rc<[u16]>>>,
    /// Plans that open a class evaluation; their slots are never reset.
    pub(in crate::compile) class: FxHashSet<usize>,
    /// The plans `clone_environment_slots` made, by the hash of their slots and scopes' identity, so a
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
        let plan_hash = |slots: &[u16], scopes: &[Rc<[u16]>]| {
            use std::hash::{Hash, Hasher};
            let mut hasher = rustc_hash::FxHasher::default();
            slots.hash(&mut hasher);
            scopes.iter().for_each(|scope| scope.as_ptr().hash(&mut hasher));
            hasher.finish()
        };
        let same_scopes = |left: &[Rc<[u16]>], right: &[Rc<[u16]>]| {
            left.len() == right.len() && left.iter().zip(right).all(|(left, right)| Rc::ptr_eq(left, right))
        };
        let hash = plan_hash(&slots, &scopes);
        let (clones, clone_scopes) = (&self.clone_plans.slots, &self.clone_plans.scopes);
        let found = self.clone_plans.lookup.find(hash, |&index| {
            clones[index as usize] == slots && same_scopes(&clone_scopes[index as usize], &scopes)
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
    pub(in crate::compile) fn push_environment_clone(&mut self, slots: Vec<u16>, scopes: Vec<Rc<[u16]>>) -> usize {
        self.clone_plans.slots.push(slots);
        self.clone_plans.scopes.push(scopes);
        self.clone_plans.slots.len() - 1
    }

    /// The slots of each lexical scope active at the current position, outermost first.
    pub(in crate::compile) fn active_lexical_slots(&mut self) -> Vec<Rc<[u16]>> {
        (0..self.lexical_scopes.len()).map(|index| self.lexical_slots(index)).collect()
    }

    /// The local slots of lexical scope `index`'s bindings, sorted: made once per scope and
    /// shared by every plan sited in it, until a target it found unslotted gains a slot.
    pub(in crate::compile) fn lexical_slots(&mut self, index: usize) -> Rc<[u16]> {
        if let Some((epoch, slots)) = &self.lexical_scopes[index].slots
            && *epoch == self.capture.unslotted_epoch
        {
            return Rc::clone(slots);
        }
        let mut slots = Vec::new();
        for target in self.lexical_scopes[index].bindings.values() {
            match self.local_slots.get(target) {
                Some(slot) => slots.push(*slot),
                None => {
                    self.capture.unslotted_targets.insert(*target);
                }
            }
        }
        slots.sort_unstable();
        slots.dedup();
        let slots: Rc<[u16]> = slots.into();
        self.lexical_scopes[index].slots = Some((self.capture.unslotted_epoch, Rc::clone(&slots)));
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
        let mut resets = Resets { owned, kept_by: Vec::new() };
        std::mem::take(&mut self.clone_plans.slots)
            .into_iter()
            .zip(scopes)
            .enumerate()
            .map(|(plan, (fresh, active))| {
                let reset = resets.of(plan, &fresh, &active, self.locals.len());
                crate::bytecode::EnvironmentClone { fresh, reset }
            })
            .collect()
    }
}

/// The slots each clone plan resets: those some plan owns that the plan neither makes fresh
/// nor finds active, marked per plan so each costs its own slots and the owned ones.
struct Resets {
    /// Ascending, so each plan's resets come out sorted.
    owned: Vec<u16>,
    /// The plan that last marked each slot as one it keeps.
    kept_by: Vec<usize>,
}

impl Resets {
    fn of(&mut self, plan: usize, fresh: &[u16], active: &[Rc<[u16]>], slot_count: usize) -> Vec<u16> {
        if self.owned.is_empty() {
            return Vec::new();
        }
        self.kept_by.resize(slot_count, usize::MAX);
        for slot in active.iter().flat_map(|scope| scope.iter()).chain(fresh) {
            self.kept_by[usize::from(*slot)] = plan;
        }
        self.owned.iter().copied().filter(|slot| self.kept_by[usize::from(*slot)] != plan).collect()
    }
}
