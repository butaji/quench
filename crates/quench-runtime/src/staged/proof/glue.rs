#![allow(unused_imports)]
use super::*;

/// The exact shapes a compiled unit holds under code dependencies (`StableFacts::shapes`), which
/// its publication subscribes: holding one is the only way a dependency makes a `ShapeFact`, so
/// a read under a held shape always retires with the code the moment an object leaves it. The
/// guard became the invalidation hook: the code tests nothing, the departure retires it.
#[cfg(not(stencil_build))]
#[derive(Default)]
pub struct HeldShapes(crate::bitset::BitSet);

#[cfg(not(stencil_build))]
impl HeldShapes {
    /// Holds `shape`: the code depends on it from now on, and the fact is the proof.
    #[inline]
    pub fn hold(&mut self, shape: u32) -> ShapeFact { self.0.insert_growing(shape as usize); ShapeFact(()) }
    /// The held shapes, leaving none.
    pub fn take(&mut self) -> Box<[u32]> { std::mem::take(&mut self.0).iter().map(|shape| shape as u32).collect() }
}

/// The global objects' layouts a compiled unit holds under code dependencies
/// (`StableFacts::layouts`, `subscribe_global_layout`): holding one is the only way a global
/// object's slot read gets its `ShapeFact`, as `HeldShapes::hold` is for an object's shape.
#[cfg(not(stencil_build))]
#[derive(Default)]
pub struct HeldLayouts(std::collections::BTreeSet<crate::heap::GlobalLayoutFact>);

#[cfg(not(stencil_build))]
impl HeldLayouts {
    /// Holds `layout`: the code depends on it from now on, and the fact is the proof.
    #[inline]
    pub fn hold(&mut self, layout: crate::heap::GlobalLayoutFact) -> ShapeFact { self.0.insert(layout); ShapeFact(()) }
    /// The held layouts, leaving none.
    pub fn take(&mut self) -> Box<[crate::heap::GlobalLayoutFact]> { std::mem::take(&mut self.0).into_iter().collect() }
}
