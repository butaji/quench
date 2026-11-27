//! Which bindings are written exactly once, with a closure: the callee a binding-loaded register
//! names.

use super::flow::FunctionFlow;
use super::gate::World;
use super::value::{Word, Key, Origin, PrototypeBase};
use crate::typed_ir::FactKey;
use std::collections::{BTreeMap, BTreeSet};

/// The most closures a method read's closed set may hold (`candidates`): a test per arm.
pub(crate) const MAX_CANDIDATES: usize = 8;

/// The fact key a binding's value is settled under (`AotFacts::cell`), so the world says whether new code can move it.
/// A named property read off any object is the name-wide join, which the solve keeps as `AnyField`.
fn fact_key(key: Key) -> FactKey {
    match key {
        Key::Global(atom) => FactKey::Global(atom),
        Key::Slot { owner, slot } => FactKey::Slot { owner, slot },
        Key::Member(atom) | Key::MemberAny(atom) => FactKey::AnyField(atom),
        Key::MemberOn { class, instance, atom } => FactKey::Field { class, instance, atom },
        Key::Return(function) => FactKey::Return(function),
        Key::Element(atom) => FactKey::Elements(atom),
    }
}

/// Every binding write of the program, counted.
pub(super) struct Census {
    /// The world the program is solved in: a live key's closure is a guess, never a proof.
    world: World,
    /// Per binding: its number of writes, and the fact of its last write.
    writes: BTreeMap<Key, (u32, Word)>,
    /// Whether a write the analysis cannot follow reaches the globals.
    global_wild: bool,
    /// Per function, whether a write it cannot follow reaches its own slots.
    wild_owner: Vec<bool>,
    /// Whether a store through a key the analysis cannot name may write a named property.
    member_wild: bool,
    /// Per named property, the closures its stores of closures write: a method read's closed set of callees.
    members: BTreeMap<u32, BTreeSet<u32>>,
    /// Per class and property (static or prototype), the one closure every store of it writes.
    members_on: BTreeMap<(u32, bool, u32), Option<u32>>,
}

impl Census {
    pub(super) fn of(flows: &[FunctionFlow], world: World) -> Self {
        let mut writes: BTreeMap<Key, (u32, Word)> = BTreeMap::new();
        for &(key, fact) in flows.iter().flat_map(|flow| &flow.writes) {
            let entry = writes.entry(key).or_insert((0, fact));
            entry.0 += 1;
            entry.1 = fact;
        }
        let mut members: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
        let mut members_on: BTreeMap<(u32, bool, u32), Option<u32>> = BTreeMap::new();
        for &(key, fact) in flows.iter().flat_map(|flow| &flow.writes) {
            let stored = fact.cell.ty.body_index();
            match key {
                // A method read's guess is the one closure stored under its name: a store of anything else (an
                // instance's data field of the same name, `this.queue = q`) is never the callee a call reaches, and the
                // call's record guards the callee it does reach (`target`).
                Key::Member(atom) => if let Some(closure) = stored { members.entry(atom).or_default().insert(closure); }
                Key::MemberOn { class, instance, atom } => {
                    let entry = members_on.entry((class, instance, atom)).or_insert(stored);
                    if *entry != stored { *entry = None; }
                }
                _ => {}
            }
        }
        Self {
            world,
            member_wild: flows.iter().any(|flow| flow.member_wild),
            members,
            members_on,
            writes,
            global_wild: flows.iter().any(|flow| flow.global_wild),
            wild_owner: flows.iter().map(|flow| flow.wild_owner).collect(),
        }
    }

    /// The closures stored under name `atom` (`members`): the closed set a method call of that name may
    /// reach, where it holds at least one and at most `MAX_CANDIDATES`.
    pub(super) fn method_closures(&self, atom: u32) -> Option<&BTreeSet<u32>> {
        self.members.get(&atom).filter(|set| (1..=MAX_CANDIDATES).contains(&set.len()))
    }

    /// Whether `target(callee)` is a guess rather than a proof: the value was read from a key new code can
    /// move in this world (`World::live`), or it is a method read's guess the solve or the census made. A
    /// method read of a known closure is still a proof in the closed world (`guess` only where unknown).
    pub(super) fn guessed(&self, callee: Word) -> bool {
        let live = match callee.origin {
            Some(Origin::Binding(key)) => self.world.live(fact_key(key)),
            Some(Origin::Member(atom)) => self.world.live(FactKey::AnyField(atom)),
            Some(Origin::MemberOn { class, instance, atom, .. }) => self.world.live(FactKey::Field { class, instance, atom }),
            _ => false,
        };
        let method_guess = callee.cell.ty.body_index().is_none() && matches!(callee.origin, Some(Origin::Member(_) | Origin::MemberOn { guess: true, .. }));
        live || method_guess
    }

    /// The closure `key` holds, when it is written once, with a closure, and no write the
    /// analysis cannot follow reaches it.
    fn single(&self, key: Key) -> Option<u32> {
        let wild = match key {
            Key::Global(_) => self.global_wild,
            Key::Slot { owner, .. } => self.wild_owner.get(owner as usize).copied().unwrap_or(true),
            Key::Member(_) => self.member_wild,
            Key::MemberOn { .. } | Key::MemberAny(_) | Key::Return(_) | Key::Element(_) => return None,
        };
        match self.writes.get(&key) {
            Some(&(1, fact)) if !wild => fact.cell.ty.body_index(),
            _ => None,
        }
    }

    /// Every binding the census proves holds one closure, which the second flow pass resolves.
    pub(super) fn singles(&self) -> BTreeMap<Key, u32> {
        self.writes.iter().filter_map(|(&key, &(count, fact))| {
            let Some(function) = fact.cell.ty.body_index() else { return None };
            (count == 1 && self.single(key) == Some(function)).then_some((key, function))
        }).collect()
    }

    /// The function a callee fact names: a closure, or the single closure of a binding.
    pub(super) fn target(&self, callee: Word) -> Option<u32> {
        if let Some(function) = callee.cell.ty.body_index() {
            return Some(function);
        }
        match callee.origin? {
            Origin::Binding(key) => self.single(key),
            // A method read: the one closure every store of its name writes. The call's record still guards the
            // callee it reaches (a builtin's property of that name, a closure made again, a store through a computed
            // key, `member_wild`), so the proof only picks the callee its direct row tries first.
            Origin::Member(atom) => self.members.get(&atom).filter(|set| set.len() == 1).and_then(|set| set.first().copied()),
            Origin::MemberOn { class, instance, atom, .. } => self.members_on.get(&(class, instance, atom)).copied().flatten(),
            _ => None,
        }
    }

    /// The closed set of functions a method read's callee may be, where it is more than one: every closure stored
    /// under its name (`members`), as `target`'s one closure is. The call's records still guard the callee each
    /// arm reaches, and a callee none names takes the VM's call.
    pub(super) fn candidates(&self, callee: Word) -> Option<&BTreeSet<u32>> {
        if callee.cell.ty.body_index().is_some() { return None; }
        let Some(Origin::Member(atom)) = callee.origin else { return None };
        self.members.get(&atom).filter(|set| (2..=MAX_CANDIDATES).contains(&set.len()))
    }

    /// The function whose `prototype` a `prototype` read off `base` takes.
    pub(super) fn prototype_owner(&self, base: PrototypeBase) -> Option<u32> {
        match base {
            PrototypeBase::Closure(function) => Some(function),
            PrototypeBase::Binding(key) => self.single(key),
        }
    }
}
