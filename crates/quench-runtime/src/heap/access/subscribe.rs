//! The slot and shape facts a native reader subscribes to, forwarded to the value arena that
//! owns their reader tables.

use crate::code_dependencies::{DependentCode, SlotDependency};
use crate::heap::Heap;
use std::rc::Rc;

impl Heap {
    pub(crate) fn subscribe_shape(&mut self, shape: u32, code: &Rc<dyn DependentCode>) -> bool {
        self.properties.subscribe_shape(shape, code)
    }

    /// The slot's observation and its shape's departures together (tests of both tables).
    #[cfg(test)]
    pub(crate) fn subscribe_slot(&mut self, dependency: SlotDependency, code: &Rc<dyn DependentCode>) -> bool {
        self.properties.subscribe_slot(dependency, code)
    }

    /// Subscribe to a slot's observation without its shape's departures: the code guards the
    /// shape it embeds at each use.
    pub(crate) fn subscribe_slot_observation(&mut self, dependency: SlotDependency, code: &Rc<dyn DependentCode>) -> bool {
        self.properties.subscribe_slot_observation(dependency, code)
    }
}
