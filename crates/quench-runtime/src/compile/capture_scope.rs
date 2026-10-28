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

/// The nearest bindings a function's compilation found in the scopes it closes over: each
/// name it looked up that some scope binds, with that scope's depth and the slot.
type ScopeMemo = FxHashMap<Atom, (u32, u16)>;

/// The scopes a function closes over, innermost first, as its compilation reads them: by
/// the nearest binding of a name.
#[derive(Clone)]
pub(super) enum ScopeChain {
    /// The scopes themselves. A replayable function's compilation keeps `memo`, the record of
    /// every binding it finds, which is all a replay of it reads.
    Scopes {
        levels: Vec<Rc<CaptureScope>>,
        memo: Option<Rc<std::cell::RefCell<ScopeMemo>>>,
    },
    /// The bindings a load-time compilation found, by atom. A name it looked up and found
    /// in no scope has no entry, so a replay finds it in none.
    Memo(Rc<[(Atom, u32, u16)]>),
}

impl ScopeChain {
    pub(super) fn new(levels: Vec<Rc<CaptureScope>>) -> Self {
        Self::Scopes { levels, memo: None }
    }

    /// The same chain, recording the names its compilation looks up.
    pub(super) fn recording(self) -> Self {
        match self {
            Self::Scopes { levels, .. } => Self::Scopes {
                levels,
                memo: Some(Rc::default()),
            },
            memo => memo,
        }
    }

    /// The recorded lookups in place of the scopes, which a replay does not need.
    pub(super) fn into_memo(self) -> Self {
        match self {
            Self::Scopes {
                memo: Some(memo), ..
            } => {
                let mut found: Vec<(Atom, u32, u16)> = std::mem::take(&mut *memo.borrow_mut())
                    .into_iter()
                    .map(|(atom, (depth, slot))| (atom, depth, slot))
                    .collect();
                found.sort_unstable();
                Self::Memo(found.into())
            }
            chain => chain,
        }
    }

    /// The depth and slot of the nearest scope binding `atom`.
    pub(super) fn find(&self, atom: Atom) -> Option<(usize, u16)> {
        match self {
            Self::Scopes { levels, memo } => {
                let found = levels
                    .iter()
                    .enumerate()
                    .find_map(|(depth, scope)| scope.get(&atom).map(|slot| (depth, *slot)));
                if let (Some(memo), Some((depth, slot))) = (memo, found) {
                    memo.borrow_mut().insert(atom, (depth as u32, slot));
                }
                found
            }
            Self::Memo(found) => found
                .binary_search_by_key(&atom, |(atom, _, _)| *atom)
                .ok()
                .map(|index| (found[index].1 as usize, found[index].2)),
        }
    }

    /// The atoms a recording chain's compilation found bound in some scope.
    #[cfg(feature = "verify-replay")]
    pub(super) fn recorded_atoms(&self) -> Vec<Atom> {
        match self {
            Self::Scopes { memo: Some(memo), .. } => memo.borrow().keys().copied().collect(),
            Self::Scopes { memo: None, .. } => Vec::new(),
            Self::Memo(found) => found.iter().map(|(atom, _, _)| *atom).collect(),
        }
    }

    pub(super) fn contains(&self, atom: Atom) -> bool {
        self.find(atom).is_some()
    }

    /// The scopes, which closures made inside the function extend. A replay compiles none of
    /// them, and has none.
    pub(super) fn levels(&self) -> &[Rc<CaptureScope>] {
        match self {
            Self::Scopes { levels, .. } => levels,
            Self::Memo(_) => &[],
        }
    }
}
