//! Which bindings are written exactly once, with a closure: the callee a binding-loaded register
//! names.

use super::flow::FunctionFlow;
use super::value::{Fact, Key, Origin, PrototypeBase};
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
    /// Per named property, the one closure its stores of closures write, or `None` where they write two.
    members: BTreeMap<u32, Option<u32>>,
    /// Per class and property (static or prototype), the one closure every store of it writes.
    members_on: BTreeMap<(u32, bool, u32), Option<u32>>,
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
        let mut members_on: BTreeMap<(u32, bool, u32), Option<u32>> = BTreeMap::new();
        for &(key, fact) in flows.iter().flat_map(|flow| &flow.writes) {
            let stored = fact.ty.body_index();
            match key {
                // A method read's guess is the one closure stored under its name: a store of anything else (an
                // instance's data field of the same name, `this.queue = q`) is never the callee a call reaches, and the
                // call's record guards the callee it does reach (`target`).
                Key::Member(atom) => if let Some(closure) = stored {
                    let entry = members.entry(atom).or_insert(Some(closure));
                    if *entry != Some(closure) { *entry = None; }
                }
                Key::MemberOn { class, instance, atom } => {
                    let entry = members_on.entry((class, instance, atom)).or_insert(stored);
                    if *entry != stored { *entry = None; }
                }
                _ => {}
            }
        }
        Self {
            member_wild: flows.iter().any(|flow| flow.member_wild),
            members,
            members_on,
            writes,
            global_wild: flows.iter().any(|flow| flow.global_wild),
            wild_owner: flows.iter().map(|flow| flow.wild_owner).collect(),
        }
    }

    /// Code added at run time as global code may write any global and any object's named
    /// properties (`gate::World::Locals`): no binding of either names one closure. Its method reads'
    /// guesses stay: a guess is no proof in any world, and the call's record guards the callee it
    /// reaches (`target`), so global code that stores another closure under the name misses the guard.
    pub(super) fn open_to_global_code(&mut self) {
        self.global_wild = true;
        self.member_wild = true;
    }

    /// Whether `target(callee)` is a method read's guess rather than a proof.
    pub(super) fn guessed(&self, callee: Fact) -> bool {
        callee.ty.body_index().is_none() && matches!(callee.origin, Some(Origin::Member(_)))
    }

    /// The closure `key` holds, when it is written once, with a closure, and no write the
    /// analysis cannot follow reaches it.
    fn single(&self, key: Key) -> Option<u32> {
        let wild = match key {
            Key::Global(_) => self.global_wild,
            Key::Slot { owner, .. } => self.wild_owner.get(owner as usize).copied().unwrap_or(true),
            Key::Member(_) => self.member_wild,
            Key::MemberOn { .. } => return None,
        };
        match self.writes.get(&key) {
            Some(&(1, fact)) if !wild => fact.ty.body_index(),
            _ => None,
        }
    }

    /// Every binding the census proves holds one closure, which the second flow pass resolves.
    pub(super) fn singles(&self) -> BTreeMap<Key, u32> {
        self.writes.iter().filter_map(|(&key, &(count, fact))| {
            let Some(function) = fact.ty.body_index() else { return None };
            (count == 1 && self.single(key) == Some(function)).then_some((key, function))
        }).collect()
    }

    /// The function a callee fact names: a closure, or the single closure of a binding.
    pub(super) fn target(&self, callee: Fact) -> Option<u32> {
        if let Some(function) = callee.ty.body_index() {
            return Some(function);
        }
        match callee.origin? {
            Origin::Binding(key) => self.single(key),
            // A method read: the one closure every store of its name writes. The call's record still guards the
            // callee it reaches (a builtin's property of that name, a closure made again, a store through a computed
            // key, `member_wild`), so the proof only picks the callee its direct row tries first.
            Origin::Member(atom) => self.members.get(&atom).copied().flatten(),
            Origin::MemberOn { class, instance, atom } => self.members_on.get(&(class, instance, atom)).copied().flatten(),
            _ => None,
        }
    }

    /// The function whose `prototype` a `prototype` read off `base` takes.
    pub(super) fn prototype_owner(&self, base: PrototypeBase) -> Option<u32> {
        match base {
            PrototypeBase::Closure(function) => Some(function),
            PrototypeBase::Binding(key) => self.single(key),
        }
    }
}
