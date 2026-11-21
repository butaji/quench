//! Which bindings are written exactly once, with a closure: the callee a binding-loaded register
//! names.

use super::flow::FunctionFlow;
use super::value::{Fact, Key};
use std::collections::BTreeMap;

/// Every binding write of the program, counted.
pub(super) struct Census {
    /// Per binding: its number of writes, and the fact of its last write.
    writes: BTreeMap<Key, (u32, Fact)>,
    /// Whether a write the analysis cannot follow reaches the globals.
    global_wild: bool,
    /// Per function, whether a write it cannot follow reaches its own slots.
    wild_owner: Vec<bool>,
    /// Whether a store through a key the analysis cannot name may write a named property.
    member_wild: bool,
    /// Per named property, the one closure every store of it writes, or `None` where a store writes anything else.
    members: BTreeMap<u32, Option<u32>>,
}

impl Census {
    pub(super) fn of(flows: &[FunctionFlow]) -> Self {
        let mut writes: BTreeMap<Key, (u32, Fact)> = BTreeMap::new();
        for &(key, fact) in flows.iter().flat_map(|flow| &flow.writes) {
            let entry = writes.entry(key).or_insert((0, fact));
            entry.0 += 1;
            entry.1 = fact;
        }
        let mut members: BTreeMap<u32, Option<u32>> = BTreeMap::new();
        for &(key, fact) in flows.iter().flat_map(|flow| &flow.writes) {
            let Key::Member(atom) = key else { continue };
            let stored = match fact { Fact::Closure(function) => Some(function), _ => None };
            let entry = members.entry(atom).or_insert(stored);
            if *entry != stored { *entry = None; }
        }
        Self {
            member_wild: flows.iter().any(|flow| flow.member_wild),
            members,
            writes,
            global_wild: flows.iter().any(|flow| flow.global_wild),
            wild_owner: flows.iter().map(|flow| flow.wild_owner).collect(),
        }
    }

    /// The closure `key` holds, when it is written once, with a closure, and no write the
    /// analysis cannot follow reaches it.
    fn single(&self, key: Key) -> Option<u32> {
        let wild = match key {
            Key::Global(_) => self.global_wild,
            Key::Slot { owner, .. } => self.wild_owner.get(owner as usize).copied().unwrap_or(true),
            Key::Member(_) => self.member_wild,
        };
        match self.writes.get(&key) {
            Some(&(1, Fact::Closure(function))) if !wild => Some(function),
            _ => None,
        }
    }

    /// The function a callee fact names: a closure, or the single closure of a binding.
    pub(super) fn target(&self, callee: Fact) -> Option<u32> {
        match callee {
            Fact::Closure(function) => Some(function),
            Fact::Binding(key) => self.single(key),
            // A method read: the one closure every store of its name writes. The call's record still guards the
            // callee it reaches (a builtin's property of that name, a closure made again, a store through a computed
            // key, `member_wild`), so the proof only picks the callee its direct row tries first.
            Fact::Member(atom) => self.members.get(&atom).copied().flatten(),
            _ => None,
        }
    }
}
