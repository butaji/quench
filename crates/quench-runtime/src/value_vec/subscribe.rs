//! Slot observations a native reader relies on: the shape its receiver must hold, and the
//! value class its slot held when the code was compiled.

use super::ValueArena;
use crate::code_dependencies::{DependentCode, SlotDependency};
use std::rc::Rc;

impl ValueArena {
    /// The slot's observation and its shape's departures together (tests of both tables).
    #[cfg(test)]
    pub(crate) fn subscribe_slot(&mut self, dependency: SlotDependency, code: &Rc<dyn DependentCode>) -> bool {
        let Some(owner) = self.admits_slot_dependency(dependency) else { return false };
        self.subscribe_shape(dependency.shape, code) && self.slot_readers.subscribe(owner, code)
    }

    /// Retire `code` at the next write that widens `dependency`'s slot type, before that write
    /// lands, not at departures of its shape: native code guards the shape it embeds at each
    /// use (or depends on it, `subscribe_shape`), and a compaction, which renumbers shape IDs,
    /// retires the code. The caller keeps the shape IDs it embeds for that retirement.
    pub(crate) fn subscribe_slot_observation(&mut self, dependency: SlotDependency, code: &Rc<dyn DependentCode>) -> bool {
        let Some(owner) = self.admits_slot_dependency(dependency) else { return false };
        self.slot_readers.subscribe(owner, code)
    }

    /// The slot owner `dependency` now describes, or `None` where its observation changed.
    fn admits_slot_dependency(&self, dependency: SlotDependency) -> Option<u32> {
        let SlotDependency { shape, slot, ty } = dependency;
        let slot = usize::from(slot);
        (self.tracking && slot < self.shape_length(shape) && self.slot_observation(shape, slot).ty() == ty)
            .then(|| self.slot_owner(shape, slot))
    }
}
