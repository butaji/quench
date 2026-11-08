//! Slot observations a native reader relies on: the shape its receiver must hold, and the
//! value class its slot held when the code was compiled.

use super::ValueArena;
use crate::code_dependencies::{DependentCode, SlotDependency};
use std::rc::Rc;

impl ValueArena {
    pub(crate) fn subscribe_slot(&mut self, dependency: SlotDependency, code: &Rc<dyn DependentCode>) -> bool {
        let Some(owner) = self.admits_slot_dependency(dependency) else { return false };
        self.subscribe_shape(dependency.shape, code) && self.slot_readers.subscribe(owner, code)
    }

    /// Observe the next write that changes `dependency`'s slot observation, not departures of
    /// its shape: native code guards the shape it embeds at each use, so an object that leaves
    /// that shape falls out at the guard, and only a compaction, which renumbers shape IDs,
    /// retires the code. The caller keeps the shape IDs it embeds for that retirement.
    pub(crate) fn subscribe_slot_observation(&mut self, dependency: SlotDependency, code: &Rc<dyn DependentCode>) -> bool {
        let Some(owner) = self.admits_slot_dependency(dependency) else { return false };
        self.slot_readers.subscribe(owner, code)
    }

    /// The slot owner `dependency` now describes, or `None` where its observation changed.
    fn admits_slot_dependency(&self, dependency: SlotDependency) -> Option<u32> {
        let SlotDependency { shape, slot, ty, target_shape } = dependency;
        let slot = usize::from(slot);
        (self.tracking && slot < self.shape_length(shape)
            && self.slot_observation(shape, slot).ty() == ty
            && target_shape.is_none_or(|target| self.slot_target_shape(shape, slot) == Some(target)))
            .then(|| self.slot_owner(shape, slot))
    }
}
