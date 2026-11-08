//! What a collection's end returns under the process's memory policy.
use super::super::*;

impl<H: Host> Vm<H> {
    /// A collection's end: memory-first returns the warm parse arena
    /// (`compile::release_warm_arena`), and the host releases its own pages.
    pub(in crate::vm) fn collection_boundary(&mut self) {
        let policy = self.memory_policy();
        if policy.returns_pages_at_collections() {
            crate::compile::release_warm_arena();
        }
        self.host.at_collection_boundary(policy);
    }
}
