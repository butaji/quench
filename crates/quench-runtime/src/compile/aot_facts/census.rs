//! Which bindings are written exactly once, with a closure: the callee a binding-loaded register
//! names.

use super::accessors::Accessors;
use super::flow::FunctionFlow;
use super::gate::World;
use super::value::{Word, Key, Origin, PrototypeBase};
use crate::typed_ir::{Classes, ElementClass, FactKey};
use std::collections::{BTreeMap, BTreeSet};

/// The most closures a method read's closed set may hold (`candidates`): a test per arm.
pub(crate) const MAX_CANDIDATES: usize = 8;

/// The fact key a binding's value is settled under (`AotFacts::cell`), so the world says whether new code can move it.
/// A named property read off any object is the name-wide join, which the solve keeps as `AnyField`. The elements of a
/// key's element class are `Elements` of that class (`ElementClass`).
pub(super) fn fact_key(key: Key) -> FactKey {
    match key {
        Key::Global(atom) => FactKey::Global(atom),
        Key::Slot { owner, slot } => FactKey::Slot { owner, slot },
        Key::Member(atom) | Key::MemberAny(atom) => FactKey::AnyField(atom),
        Key::MemberOn { class, instance, atom } => FactKey::Field { class, instance, atom },
        Key::Return(function) => FactKey::Return(function),
        Key::Element(class) => FactKey::Elements(class),
    }
}

/// The element classes of the program's arrays: a Steensgaard union-find over `ElementClass`. Each link a flow makes
/// (`FunctionFlow::links`), and each link the types add (a call's callees' returns, the classes escapes reach), joins
/// the classes of its two keys. An array's elements are whatever is stored into any array of its class, because an
/// array reached through another register, a binding or a field is the same array, and a store through any of them
/// must be seen through all of them. A class no link joins is its own, so two arrays that never meet stay apart.
#[derive(Clone, Default, PartialEq, Eq)]
pub(super) struct ElementClasses {
    /// Per class a link joined into another, the class it joined. A class's root has no entry.
    parent: BTreeMap<ElementClass, ElementClass>,
    /// Per class root, the classes of its class, the root among them: the classes a read of the class's elements joins.
    class_keys: BTreeMap<ElementClass, Vec<ElementClass>>,
}

impl ElementClasses {
    /// The classes `links` make. Each class's members are listed once the links are all joined, so a read of a class is
    /// a walk over its members, never a search over every class.
    pub(super) fn of<'a>(links: impl Iterator<Item = &'a (ElementClass, ElementClass)>) -> Self {
        let mut classes = Self::default();
        for &(from, to) in links { classes.join(from, to); }
        let keys: BTreeSet<ElementClass> = classes.parent.iter().flat_map(|(&key, &parent)| [key, parent]).collect();
        for key in keys {
            let root = classes.root(key);
            classes.class_keys.entry(root).or_default().push(key);
        }
        classes
    }

    /// The root of `key`'s class: the class every class of it follows to.
    fn root(&self, mut key: ElementClass) -> ElementClass {
        while let Some(&parent) = self.parent.get(&key) { key = parent; }
        key
    }

    /// Joins the classes of `from` and `to`. Roots only are linked, so the chains stay as short as the links make them.
    fn join(&mut self, from: ElementClass, to: ElementClass) {
        let (from, to) = (self.root(from), self.root(to));
        if from != to { self.parent.insert(from, to); }
    }

    /// Every class of `key`'s class, `key` among them: the classes a store into any array of it writes, so the classes
    /// whose elements a read of its elements joins. A class no link names is its own class.
    pub(super) fn members(&self, key: ElementClass) -> impl Iterator<Item = ElementClass> + '_ {
        let root = self.root(key);
        let listed: &[ElementClass] = self.class_keys.get(&root).map_or(&[][..], Vec::as_slice);
        listed.iter().copied().chain(std::iter::once(root).filter(move |_| listed.is_empty()))
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
    /// The accessors the program defines and what may shadow them (`accessors::Accessors`).
    pub(super) accessors: Accessors,
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
        let mut census = Self {
            world,
            member_wild: flows.iter().any(|flow| flow.member_wild),
            members,
            members_on,
            writes,
            global_wild: flows.iter().any(|flow| flow.global_wild || flow.computed_global),
            wild_owner: flows.iter().map(|flow| flow.wild_owner).collect(),
            accessors: Accessors::default(),
        };
        census.accessors = Accessors::of(flows, &census);
        census
    }

    /// The closures stored under name `atom` (`members`): the closed set a method call of that name may
    /// reach, where it holds at least one and at most `MAX_CANDIDATES`.
    pub(super) fn method_closures(&self, atom: u32) -> Option<&BTreeSet<u32>> {
        self.members.get(&atom).filter(|set| (1..=MAX_CANDIDATES).contains(&set.len()))
    }

    /// Whether new code (`World`) can move `key`: a store under a live key may put any closure where the solve reads it,
    /// so the closure stored escapes (`types::escapes`).
    pub(super) fn live(&self, key: FactKey) -> bool {
        self.world.live(key)
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

    /// Whether a write the analysis cannot follow may reach `key`: a global written by code it cannot name
    /// (`global_wild`), a slot of a function whose frame it cannot follow (`wild_owner`), or a named property
    /// written through a computed key (`member_wild`). The keys with no such write are never wild.
    pub(super) fn wild(&self, key: Key) -> bool {
        match key {
            Key::Global(_) => self.global_wild,
            Key::Slot { owner, .. } => self.wild_owner.get(owner as usize).copied().unwrap_or(true),
            Key::Member(_) => self.member_wild,
            Key::MemberOn { .. } | Key::MemberAny(_) | Key::Return(_) | Key::Element(_) => false,
        }
    }

    /// Whether a write the analysis cannot follow may reach the value a read off `origin` takes (`wild`).
    pub(super) fn wild_read(&self, origin: Origin) -> bool {
        match origin {
            Origin::Binding(key) => self.wild(key),
            Origin::Member(atom) | Origin::MemberOn { atom, .. } => self.wild(Key::Member(atom)),
            _ => false,
        }
    }

    /// Whether every class `classes` lists holds its own method `atom` (`members_on`), and every one of those is one of
    /// `callees`. A method call whose receiver is such a closed set of program classes runs the program's own method, so
    /// no builtin of the same name is the callee (`types::resolved`).
    pub(super) fn own_methods(&self, classes: Classes, atom: u32, callees: &[u32]) -> bool {
        let ids = classes.ids();
        !ids.is_empty() && ids.iter().all(|&class| matches!(self.members_on.get(&(class, true, atom)), Some(Some(function)) if callees.contains(function)))
    }

    /// The closure `key` holds, when it is written once, with a closure, and no write the
    /// analysis cannot follow reaches it.
    fn single(&self, key: Key) -> Option<u32> {
        if matches!(key, Key::MemberOn { .. } | Key::MemberAny(_) | Key::Return(_) | Key::Element(_)) { return None; }
        match self.writes.get(&key) {
            Some(&(1, fact)) if !self.wild(key) => fact.cell.ty.body_index(),
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
