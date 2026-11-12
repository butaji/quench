//! The heap identities compiled code names as constants, which its publication keeps alive.
//!
//! A record id is reused once its object is collected, so code whose guard compares a
//! callee's id, or whose fold reads a prototype's value, would pass for the wrong object if
//! that object died while the code stayed published. Each publication therefore owns the
//! set of identities its code embeds (`FoldedRoots`), the heap roots every set of a live
//! publication at each collection (`FoldedRegistry`), and retiring the publication drops it.

use crate::Value;
use std::rc::{Rc, Weak};

/// The heap values one body of code embeds, ordered by their bits and without repeats.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FoldedRoots(Vec<Value>);

impl FoldedRoots {
    /// The word code names `value` by. The only way to embed a value in generated code: a
    /// heap value is registered here before its word is returned, so the publication that
    /// takes this set keeps it alive for as long as the code can compare it.
    pub(crate) fn embed_heap_identity(&mut self, value: Value) -> u64 {
        if value.is_heap() && let Err(at) = self.0.binary_search_by_key(&value.to_bits(), |held| held.to_bits()) {
            self.0.insert(at, value);
        }
        debug_assert!(!value.is_heap() || self.0.contains(&value), "an embedded heap value is registered");
        value.to_bits()
    }

    pub(crate) fn is_empty(&self) -> bool { self.0.is_empty() }

    /// Releases the spare capacity the set grew with while code lowered.
    pub(crate) fn settled(mut self) -> Self {
        self.0.shrink_to_fit();
        self
    }

    pub(crate) fn extend_roots(&self, roots: &mut Vec<Value>) { roots.extend_from_slice(&self.0) }
}

/// A publication whose code embeds heap identities (`PublishedCode::hold_roots`).
pub(crate) trait RootedCode {
    fn is_published(&self) -> bool;
    fn extend_roots(&self, roots: &mut Vec<Value>);
}

/// The publications of one heap that hold folded roots, weakly: a collection roots each one
/// still published, and forgets the retired and the dropped.
#[derive(Default)]
pub(crate) struct FoldedRegistry(Vec<Weak<dyn RootedCode>>);

impl FoldedRegistry {
    pub(crate) fn register(&mut self, code: &Rc<dyn RootedCode>) { self.0.push(Rc::downgrade(code)) }

    pub(crate) fn extend_roots(&mut self, roots: &mut Vec<Value>) {
        self.0.retain(|weak| match weak.upgrade() {
            Some(code) if code.is_published() => { code.extend_roots(roots); true }
            _ => false,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct Code(Cell<bool>, FoldedRoots);
    impl RootedCode for Code {
        fn is_published(&self) -> bool { self.0.get() }
        fn extend_roots(&self, roots: &mut Vec<Value>) { self.1.extend_roots(roots) }
    }

    #[test]
    fn embedding_registers_heap_values_once_and_passes_immediates_through() {
        let (mut roots, object) = (FoldedRoots::default(), Value::heap(7));
        assert_eq!(roots.embed_heap_identity(Value::integer(3)), Value::integer(3).to_bits());
        assert!(roots.is_empty(), "an immediate names no heap record");
        assert_eq!(roots.embed_heap_identity(object), object.to_bits());
        roots.embed_heap_identity(object);
        let mut rooted = Vec::new();
        roots.extend_roots(&mut rooted);
        assert_eq!(rooted, [object]);
    }

    #[test]
    fn a_registry_roots_published_code_and_forgets_retired_and_dropped_code() {
        let mut roots = FoldedRoots::default();
        let object = Value::heap(9);
        roots.embed_heap_identity(object);
        let (live, retired, dropped) = (Rc::new(Code(Cell::new(true), roots.clone())),
            Rc::new(Code(Cell::new(false), roots.clone())), Rc::new(Code(Cell::new(true), roots)));
        let mut registry = FoldedRegistry::default();
        for code in [live.clone() as Rc<dyn RootedCode>, retired, dropped.clone()] { registry.register(&code); }
        drop(dropped);
        let mut rooted = Vec::new();
        registry.extend_roots(&mut rooted);
        assert_eq!(rooted, [object], "only the published, held code roots its identities");
        assert_eq!(registry.0.len(), 1);
        live.0.set(false);
        registry.extend_roots(&mut rooted);
        assert!(registry.0.is_empty());
    }
}
