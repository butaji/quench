//! The bindings of one enclosing function that a closure created inside it can capture.

use crate::bytecode::Atom;
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::rc::Rc;

/// A function's base bindings (locals and compiler bindings), shared by every position in
/// it. Locals are only appended, so each binding carries the local count from which it is
/// visible, and a position sees the bindings below its own count.
#[derive(Default)]
pub(super) struct BaseBindings(RefCell<FxHashMap<Atom, (u32, u16)>>);

impl BaseBindings {
    /// Binds `atom` from local count `from`, unless it is already bound: an earlier position
    /// keeps seeing what it saw.
    pub(super) fn bind(&self, atom: Atom, from: u32, slot: u16) -> bool {
        match self.0.borrow_mut().entry(atom) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.get().1 == slot,
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert((from, slot));
                true
            }
        }
    }
}

/// The base bindings visible at one position: those bound below local count `visible`, and
/// the parameter context's own `arguments`, which no local shadows.
#[derive(Clone)]
pub(super) struct CaptureBase {
    pub(super) bindings: Rc<BaseBindings>,
    pub(super) visible: u32,
    pub(super) arguments: Option<(Atom, u16)>,
}

impl CaptureBase {
    fn get(&self, atom: Atom) -> Option<u16> {
        if let Some((name, slot)) = self.arguments
            && name == atom
        {
            return Some(slot);
        }
        let bindings = self.bindings.0.borrow();
        bindings.get(&atom).filter(|(from, _)| *from < self.visible).map(|(_, slot)| *slot)
    }

    fn entries(&self) -> Vec<(Atom, u16)> {
        let bindings = self.bindings.0.borrow();
        bindings
            .iter()
            .filter(|(atom, (from, _))| *from < self.visible && self.arguments.is_none_or(|(name, _)| name != **atom))
            .map(|(atom, (_, slot))| (*atom, *slot))
            .chain(self.arguments)
            .collect()
    }
}

/// One lexical scope's bindings over those around it: `Some` binds or rebinds a name, `None`
/// hides one the scopes around it bind. A scope's layer depends on its own bindings alone, so
/// every position inside it shares one.
pub(super) type CaptureLayer = FxHashMap<Atom, Option<u16>>;

/// A function's capturable bindings at one position: its base bindings under the layers of
/// the lexical scopes in effect there, innermost last. The base and each layer are shared by
/// every position that sees them, so a closure costs the scopes it is nested in, not the
/// bindings they hold.
pub(super) struct CaptureScope {
    base: CaptureBase,
    layers: Vec<Rc<CaptureLayer>>,
}

impl CaptureScope {
    pub(super) fn new(base: CaptureBase) -> Self {
        Self { base, layers: Vec::new() }
    }

    pub(super) fn get(&self, atom: &Atom) -> Option<u16> {
        match self.layers.iter().rev().find_map(|layer| layer.get(atom)) {
            Some(slot) => *slot,
            None => self.base.get(*atom),
        }
    }

    pub(super) fn contains_key(&self, atom: &Atom) -> bool {
        self.get(atom).is_some()
    }

    pub(super) fn push_layer(&mut self, layer: Rc<CaptureLayer>) {
        self.layers.push(layer);
    }

    /// Every binding once: the innermost layer's first, then the base's they leave visible.
    pub(super) fn iter(&self) -> impl Iterator<Item = (Atom, u16)> + '_ {
        let mut seen = rustc_hash::FxHashSet::default();
        let layered: Vec<_> = self
            .layers
            .iter()
            .rev()
            .flat_map(|layer| layer.iter())
            .filter(|(atom, _)| seen.insert(**atom))
            .filter_map(|(atom, slot)| slot.map(|slot| (*atom, slot)))
            .collect();
        layered
            .into_iter()
            .chain(self.base.entries().into_iter().filter(move |(atom, _)| !seen.contains(atom)))
    }

    pub(super) fn keys(&self) -> impl Iterator<Item = Atom> + '_ {
        self.iter().map(|(atom, _)| atom)
    }
}

impl FromIterator<(Atom, u16)> for CaptureScope {
    fn from_iter<I: IntoIterator<Item = (Atom, u16)>>(bindings: I) -> Self {
        let base = BaseBindings::default();
        for (atom, slot) in bindings {
            base.0.borrow_mut().insert(atom, (0, slot));
        }
        Self::new(CaptureBase { bindings: Rc::new(base), visible: 1, arguments: None })
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
                    .find_map(|(depth, scope)| scope.get(&atom).map(|slot| (depth, slot)));
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
