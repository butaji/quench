//! The slot and shape facts a native reader subscribes to, forwarded to the value arena that
//! owns their reader tables, and the prototype chains it read through, held in the dependency
//! table by their prototypes' subjects.

use crate::code_dependencies::{DependentCode, PrototypeChain, SlotDependency};
use crate::heap::{Heap, ProtoLink, INHERITED_FIELD_CACHE_MAX_DEPTH};
use crate::value::Value;
use std::rc::Rc;

/// The subjects of one chain's prototypes, nearest first (`Heap::chain_subjects`).
pub(crate) type ChainSubjects = smallvec::SmallVec<[u32; INHERITED_FIELD_CACHE_MAX_DEPTH]>;

impl Heap {
    pub fn subscribe_shape(&mut self, shape: u32, code: &Rc<dyn DependentCode>) -> bool {
        self.properties.subscribe_shape(shape, code)
    }

    /// The bytes of the facts held in this heap: the slot and shape readers, and the layouts code holds.
    pub(crate) fn dependency_bytes(&self) -> usize {
        self.properties.dependency_bytes() + self.dependencies.layout_hold_bytes()
    }

    /// The slot's observation and its shape's departures together (tests of both tables).
    #[cfg(test)]
    pub(crate) fn subscribe_slot(&mut self, dependency: SlotDependency, code: &Rc<dyn DependentCode>) -> bool {
        self.properties.subscribe_slot(dependency, code)
    }

    /// Subscribe to a slot's observation without its shape's departures: the code guards the
    /// shape it embeds at each use.
    pub fn subscribe_slot_observation(&mut self, dependency: SlotDependency, code: &Rc<dyn DependentCode>) -> bool {
        self.properties.subscribe_slot_observation(dependency, code)
    }

    /// The subjects of the `depth` prototypes from `prototype`, each marked a prototype first;
    /// `None` where one is no object or has no subject, so no subject names that chain.
    pub(crate) fn chain_subjects(&mut self, prototype: Value, depth: u8) -> Option<ChainSubjects> {
        let mut subjects = ChainSubjects::new();
        let mut link = prototype;
        for _ in 0..depth {
            self.mark_prototype(link);
            let object = self.object_ref(link)?;
            subjects.push(object.watched_subject()?);
            link = object.proto();
        }
        Some(subjects)
    }

    /// Makes `code` retire when a prototype of `chain` changes its layout or dies, or at an event
    /// no prototype names (`Dependencies::hold_layouts`). A chain no subject names (a link that is
    /// no watched object) falls back to every prototype's layout
    /// (`heap::subscribe_prototype_layouts`). `false` where a reader table is full.
    pub fn subscribe_prototype_chain(&mut self, chain: PrototypeChain, code: &Rc<dyn DependentCode>) -> bool {
        match self.chain_subjects(ProtoLink::from_bits(chain.link).prototype(), chain.depth) {
            Some(subjects) => self.dependencies.hold_layouts(&subjects, code),
            None => crate::heap::subscribe_prototype_layouts(code),
        }
    }
}
