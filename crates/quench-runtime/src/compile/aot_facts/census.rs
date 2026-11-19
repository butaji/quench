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
}

impl Census {
    pub(super) fn of(flows: &[FunctionFlow]) -> Self {
        let mut writes: BTreeMap<Key, (u32, Fact)> = BTreeMap::new();
        for &(key, fact) in flows.iter().flat_map(|flow| &flow.writes) {
            let entry = writes.entry(key).or_insert((0, fact));
            entry.0 += 1;
            entry.1 = fact;
        }
        Self {
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
            _ => None,
        }
    }
}
