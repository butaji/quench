//! The bindings of one enclosing function that a closure created inside it can capture.

use crate::bytecode::Atom;
use rustc_hash::FxHashMap;
use std::rc::Rc;

/// A function's capturable bindings at one position: its base bindings (locals and compiler
/// bindings, which change only as locals are added) under the lexical bindings in effect
/// there. The base is shared by every position with the same locals, so a closure inside a
/// block of a large function costs the block's bindings, not the function's.
pub(super) struct CaptureScope {
    base: Rc<FxHashMap<Atom, u16>>,
    /// Lexical bindings over the base: `Some` binds or rebinds a name, `None` hides a base
    /// binding the lexical scopes shadow without rebinding.
    overlay: FxHashMap<Atom, Option<u16>>,
}

impl CaptureScope {
    pub(super) fn new(base: Rc<FxHashMap<Atom, u16>>) -> Self {
        Self {
            base,
            overlay: FxHashMap::default(),
        }
    }

    pub(super) fn get(&self, atom: &Atom) -> Option<&u16> {
        match self.overlay.get(atom) {
            Some(slot) => slot.as_ref(),
            None => self.base.get(atom),
        }
    }

    pub(super) fn contains_key(&self, atom: &Atom) -> bool {
        self.get(atom).is_some()
    }

    pub(super) fn insert(&mut self, atom: Atom, slot: u16) {
        self.overlay.insert(atom, Some(slot));
    }

    pub(super) fn remove(&mut self, atom: &Atom) {
        if self.contains_key(atom) {
            self.overlay.insert(*atom, None);
        }
    }

    /// Every binding once: the overlay's, then the base's it leaves visible.
    pub(super) fn iter(&self) -> impl Iterator<Item = (&Atom, &u16)> {
        self.overlay
            .iter()
            .filter_map(|(atom, slot)| slot.as_ref().map(|slot| (atom, slot)))
            .chain(
                self.base
                    .iter()
                    .filter(|(atom, _)| !self.overlay.contains_key(atom)),
            )
    }

    pub(super) fn keys(&self) -> impl Iterator<Item = &Atom> {
        self.iter().map(|(atom, _)| atom)
    }
}

impl FromIterator<(Atom, u16)> for CaptureScope {
    fn from_iter<I: IntoIterator<Item = (Atom, u16)>>(bindings: I) -> Self {
        Self::new(Rc::new(bindings.into_iter().collect()))
    }
}
