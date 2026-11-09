//! The host's cleanup boundary inside a collection.
use super::HostWeakRoot;
use crate::Host;

/// A collection's host cleanup boundary. It can inspect and release private weak roots,
/// but cannot allocate guest values, invoke JavaScript or initiate another collection.
pub struct CollectionContext<'a, H: Host> {
    host: &'a mut H,
    heap: &'a mut crate::heap::Heap,
}

impl<'a, H: Host> CollectionContext<'a, H> {
    pub(crate) fn new(host: &'a mut H, heap: &'a mut crate::heap::Heap) -> Self {
        Self { host, heap }
    }

    pub fn host_mut(&mut self) -> &mut H {
        self.host
    }

    pub fn host_weak_root_is_live(&self, reference: HostWeakRoot) -> bool {
        self.heap.weak_reference_value(reference.0).is_some()
    }

    pub fn release_host_weak_root(&mut self, reference: HostWeakRoot) -> bool {
        self.heap.release_root(reference.0)
    }
}
