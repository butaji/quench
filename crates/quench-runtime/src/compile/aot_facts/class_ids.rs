//! The class ids of the program's constructors. A class id is a constructor's function id (its body index) unless the body is
//! constructed under two or more binding keys: a `Class.create()` helper returns one body for every class it makes, so every
//! class of such a body would share its prototype and instances. Each (body, key) pair a construction names is then a class of
//! its own, numbered above the function count. The table is the one home of that mapping: a class id is read back to its body
//! through `ClassIds::body`, and a construction, a prototype or a store resolves its key through `ClassIds::of`.

use super::flow::FunctionFlow;
use super::value::{Key, Origin, Word};
use std::collections::{BTreeMap, BTreeSet};

/// The class ids of a program: a function id for each body no split touches, and a synthetic id above `functions` for each
/// (body, key) pair a construction names where the body is stored under two or more binding keys.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ClassIds {
    /// The number of functions of the program: the first synthetic id.
    functions: u32,
    /// The synthetic classes in id order: class `functions + index` is the body and key at `index`.
    synthetic: Vec<(u32, Key)>,
    /// The synthetic class of each (body, key) pair of `synthetic`.
    ids: BTreeMap<(u32, Key), u32>,
}

impl ClassIds {
    /// The class ids of the flows of a program of `functions` functions, given the settled flows of its solve: every
    /// construction (`Call::construct`) whose callee is a closure of body `B` read from a binding key `K`, where `B` is
    /// stored under at least two binding keys and `K` is one of them, is a class of its own.
    pub(super) fn build(flows: &[FunctionFlow], functions: u32) -> Self {
        let mut stored: BTreeMap<u32, BTreeSet<Key>> = BTreeMap::new();
        for &(_, key, word) in flows.iter().flat_map(|flow| &flow.writes) {
            if let (Some(body), true) = (word.cell.ty.body_index(), is_binding(key)) {
                stored.entry(body).or_default().insert(key);
            }
        }
        let mut classes = Self { functions, synthetic: Vec::new(), ids: BTreeMap::new() };
        for call in flows.iter().flat_map(|flow| &flow.calls).filter(|call| call.construct) {
            let (Some(body), Some(key)) = (call.callee.cell.ty.body_index(), call.callee.origin.and_then(Origin::key)) else { continue };
            if !is_binding(key) || stored.get(&body).is_none_or(|keys| keys.len() < 2 || !keys.contains(&key)) { continue; }
            if !classes.ids.contains_key(&(body, key)) {
                classes.ids.insert((body, key), functions + classes.synthetic.len() as u32);
                classes.synthetic.push((body, key));
            }
        }
        classes
    }

    /// The class of a value `word` holds when it is a closure constructed or read as its own class: its body, or the synthetic
    /// class of its body and the binding key it was read from (`Origin::key`). None where `word` is no closure.
    pub(super) fn of_word(&self, word: Word) -> Option<u32> {
        let body = word.cell.ty.body_index()?;
        Some(word.origin.and_then(Origin::key).map_or(body, |key| self.of(body, key)))
    }

    /// The class of body `body` read from binding key `key`: a synthetic class where the pair has one, else the body itself.
    pub(super) fn of(&self, body: u32, key: Key) -> u32 {
        self.ids.get(&(body, key)).copied().unwrap_or(body)
    }

    /// The number of class ids: the functions and the synthetic classes, so every class id is below it. A runtime sizes its
    /// per-class tables (`ProgramStore::class_header`) by this count.
    pub(crate) fn count(&self) -> u32 {
        self.functions + self.synthetic.len() as u32
    }

    /// The function body class `class` runs: `class` itself for an ordinary class, the body of its pair for a synthetic one.
    pub(super) fn body(&self, class: u32) -> u32 {
        match class.checked_sub(self.functions).and_then(|index| self.synthetic.get(index as usize)) {
            Some(&(body, _)) => body,
            None => class,
        }
    }

    /// Whether the program has any synthetic class.
    pub(super) fn is_empty(&self) -> bool {
        self.synthetic.is_empty()
    }
}

/// Whether `key` names a binding a closure may be stored under and read back from (a global, a slot, a named property or a
/// class's own method), not a return, an element or a name of no one class.
fn is_binding(key: Key) -> bool {
    matches!(key, Key::Global(_) | Key::Slot { .. } | Key::Member(_) | Key::MemberOn { .. })
}
