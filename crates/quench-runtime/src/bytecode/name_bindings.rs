//! The bindings a function's dynamic name lookups resolve statically.
//!
//! A function sees its own direct bindings first; the names it declares hide every captured
//! binding; captured bindings come from the scopes it closes over, innermost first. Captured
//! scopes are shared by every function closing over them, so a function costs its own
//! declarations rather than a copy of every outer name.

use super::{Atom, EvalBinding, EvalBindingLocation, Function};
#[cfg(any(test, feature = "test-support"))] use super::WideInstruction;
use super::{Op, Traits};
use std::rc::Rc;

/// One word: most functions bind no names a dynamic lookup can see (`None`), and every
/// compiled function record holds one of these.
#[derive(Clone, Debug, Default)]
pub(crate) struct NameBindings(Option<Rc<NameBindingLists>>);

#[derive(Debug)]
pub(crate) struct NameBindingLists {
    /// Bindings resolved before anything else, sorted by atom.
    direct: Box<[EvalBinding]>,
    /// Names declared by the function that hide captured bindings, sorted.
    hidden: Box<[Atom]>,
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
        if direct.is_empty() && hidden.is_empty() && captured.is_none() {
            return Self(None);
        }
        Self(Some(Rc::new(NameBindingLists { direct: direct.into(), hidden: hidden.into(), captured })))
    }

    fn lists(&self) -> (&[EvalBinding], &[Atom], Option<&Rc<CapturedNames>>) {
        match &self.0 {
            Some(lists) => (&lists.direct, &lists.hidden, lists.captured.as_ref()),
            None => (&[], &[], None),
        }
    }

    /// A flat, already resolved list such as a decoded residual program's.
    pub(crate) fn flat(bindings: Vec<EvalBinding>) -> Self {
        Self::new(bindings, Vec::new(), None)
    }

    pub(crate) fn get(&self, atom: Atom) -> Option<EvalBinding> {
        let (direct, hidden, _) = self.lists();
        if let Ok(index) = direct.binary_search_by_key(&atom, |binding| binding.atom) {
            return Some(direct[index]);
        }
        if hidden.binary_search(&atom).is_ok() {
            return None;
        }
        self.captured_levels()
            .find_map(|(depth, level)| level.find(atom).map(|binding| at_depth(binding, depth)))
    }

    pub(crate) fn direct(&self) -> &[EvalBinding] {
        self.lists().0
    }

    pub(crate) fn hidden(&self) -> &[Atom] {
        self.lists().1
    }

    /// Each captured scope with its depth from this function.
    pub(crate) fn captured_levels(&self) -> impl Iterator<Item = (u16, &CapturedNames)> {
        self.lists()
            .2
            .into_iter()
            .flat_map(|captured| captured.levels())
            .zip(0..=u16::MAX)
            .map(|(level, depth)| (depth, level))
    }

    /// Every visible binding, sorted by atom: the flat projection residual programs store.
    pub(crate) fn resolved(&self) -> Vec<EvalBinding> {
        let (direct, hidden, _) = self.lists();
        let mut seen: rustc_hash::FxHashSet<Atom> = hidden.iter().copied().collect();
        let mut bindings: Vec<EvalBinding> = direct.to_vec();
        seen.extend(bindings.iter().map(|binding| binding.atom));
        for (depth, level) in self.captured_levels() {
            bindings.extend(
                level
                    .bindings_at(depth)
                    .filter(|binding| seen.insert(binding.atom)),
            );
        }
        crate::bytecode::EvalBinding::sort_by_atom(&mut bindings);
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

impl Function {
    /// The local slot a store of this function writes: a local store's own, or the slot named
    /// as a by-name store's atom, which a script root keeps for the global var its functions
    /// capture.
    #[cfg(any(test, feature = "test-support"))]
    #[quench_macros::test_visible]
    pub(crate) fn stored_slot(&self, instruction: WideInstruction) -> Option<usize> {
        match instruction.op() {
            op if op.traits().has(Traits::LOCAL_SLOT_STORE) => Some(instruction.local_slot()),
            op if op.stores_name() => self.local_atoms.iter().position(|atom| *atom == instruction.atom_index()),
            _ => None,
        }
    }
}

/// Whether a load or store by name reads a hidden or unknown binding (a global store is the lane's).
pub(super) fn reads_hidden_name(instruction: super::WideInstruction, tables: &impl super::FactTables) -> bool {
    (instruction.op().loads_name() || instruction.op().stores_name()) && usize::try_from(instruction.atom_index())
        .is_ok_and(|atom| tables.atom_name(atom).is_none_or(|name| name.starts_with('\0') && !super::is_intrinsic_name(name)))
}

/// The ops that read the implicit bindings an environment holds, by their form.
pub(super) fn reads_binding_op(op: Op) -> bool {
    op.traits().has(Traits::BINDING_FORM)
}

/// The ops `body_binding_reads_beside_clones` examines: those that name a binding, read one by
/// form (`reads_binding_op`) or run a direct eval.
pub(super) fn may_read_binding(op: Op) -> bool {
    op.loads_name() || op.stores_name() || op.is_call_with_eval_flags() || reads_binding_op(op)
}
