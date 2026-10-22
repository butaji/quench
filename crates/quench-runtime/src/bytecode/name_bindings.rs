//! The bindings a function's dynamic name lookups resolve statically.
//!
//! A function sees its own direct bindings first; the names it declares hide every captured
//! binding; captured bindings come from the scopes it closes over, innermost first. Captured
//! scopes are shared by every function closing over them, so a function costs its own
//! declarations rather than a copy of every outer name.

use super::{Atom, EvalBinding, EvalBindingLocation};
use std::rc::Rc;

#[derive(Clone, Debug, Default)]
pub(crate) struct NameBindings {
    /// Bindings resolved before anything else, sorted by atom.
    direct: Rc<[EvalBinding]>,
    /// Names declared by the function that hide captured bindings, sorted.
    hidden: Rc<[Atom]>,
    /// The innermost captured scope.
    captured: Option<Rc<CapturedNames>>,
}

/// One captured scope's bindings, with the scopes enclosing it.
#[derive(Debug)]
pub(crate) struct CapturedNames {
    /// Sorted by atom; capture depths are relative to this scope.
    bindings: Box<[EvalBinding]>,
    outer: Option<Rc<CapturedNames>>,
}

impl CapturedNames {
    /// `bindings` must be sorted by atom with one entry per atom and capture depth zero.
    pub(crate) fn new(bindings: Vec<EvalBinding>, outer: Option<Rc<CapturedNames>>) -> Self {
        debug_assert!(bindings.windows(2).all(|pair| pair[0].atom < pair[1].atom));
        Self {
            bindings: bindings.into(),
            outer,
        }
    }

    pub(crate) fn contains(&self, atom: Atom) -> bool {
        self.levels().any(|level| level.find(atom).is_some())
    }

    fn find(&self, atom: Atom) -> Option<EvalBinding> {
        self.bindings
            .binary_search_by_key(&atom, |binding| binding.atom)
            .ok()
            .map(|index| self.bindings[index])
    }

    fn levels(&self) -> impl Iterator<Item = &CapturedNames> {
        std::iter::successors(Some(self), |level| level.outer.as_deref())
    }
}

/// A binding found `depth` captured scopes out.
fn at_depth(mut binding: EvalBinding, depth: u16) -> EvalBinding {
    if let EvalBindingLocation::Capture { slot, .. } = binding.location {
        binding.location = EvalBindingLocation::Capture { depth, slot };
    }
    binding
}

impl NameBindings {
    pub(crate) fn new(
        direct: Vec<EvalBinding>,
        hidden: Vec<Atom>,
        captured: Option<Rc<CapturedNames>>,
    ) -> Self {
        Self {
            direct: direct.into(),
            hidden: hidden.into(),
            captured,
        }
    }

    /// A flat, already resolved list such as a decoded residual program's.
    pub(crate) fn flat(bindings: Vec<EvalBinding>) -> Self {
        Self::new(bindings, Vec::new(), None)
    }

    pub(crate) fn get(&self, atom: Atom) -> Option<EvalBinding> {
        if let Ok(index) = self
            .direct
            .binary_search_by_key(&atom, |binding| binding.atom)
        {
            return Some(self.direct[index]);
        }
        if self.hidden.binary_search(&atom).is_ok() {
            return None;
        }
        self.captured_levels()
            .find_map(|(depth, level)| level.find(atom).map(|binding| at_depth(binding, depth)))
    }

    pub(crate) fn direct(&self) -> &[EvalBinding] {
        &self.direct
    }

    pub(crate) fn hidden(&self) -> &[Atom] {
        &self.hidden
    }

    /// Each captured scope with its depth from this function.
    pub(crate) fn captured_levels(&self) -> impl Iterator<Item = (u16, &CapturedNames)> {
        self.captured
            .iter()
            .flat_map(|captured| captured.levels())
            .zip(0..=u16::MAX)
            .map(|(level, depth)| (depth, level))
    }

    /// Every visible binding, sorted by atom: the flat projection residual programs store.
    pub(crate) fn resolved(&self) -> Vec<EvalBinding> {
        let mut seen: rustc_hash::FxHashSet<Atom> = self.hidden.iter().copied().collect();
        let mut bindings: Vec<EvalBinding> = self.direct.to_vec();
        seen.extend(bindings.iter().map(|binding| binding.atom));
        for (depth, level) in self.captured_levels() {
            bindings.extend(
                level
                    .bindings_at(depth)
                    .filter(|binding| seen.insert(binding.atom)),
            );
        }
        bindings.sort_unstable_by_key(|binding| binding.atom);
        bindings
    }
}

impl CapturedNames {
    /// This scope's bindings as seen `depth` captured scopes out, sorted by atom.
    pub(crate) fn bindings_at(&self, depth: u16) -> impl Iterator<Item = EvalBinding> + '_ {
        self.bindings
            .iter()
            .map(move |binding| at_depth(*binding, depth))
    }
}
