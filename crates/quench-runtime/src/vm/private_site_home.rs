//! The class home a lane private-name access found last, per access site. A private name's
//! home depends on the running function and the environment its frame captures from, never
//! on the accessed object (`Vm::private_name_home`); a lane record belongs to one function of
//! one program, so its site fixes all but the environment. An entry names the environment,
//! which names the same cell, and so the same home, for as long as it lives: each collection
//! drops the entries a value of which it freed (`prune_dead`), as `Vm::private_homes` drops
//! its own, and keeps the rest. Only functions without binding sites record, whose homes no
//! eval-introduced private binding can change.
//!
//! An entry also keeps the private method or accessor the site's name resolved to under that
//! home, once found (`PrivateElement`): a class evaluation makes its private methods and
//! accessors once, under its own home, and no program can redefine them. The element follows
//! the home alone, so it outlives a new environment that finds the same home again (an arrow
//! field's closure per instance).
//!
//! A site whose private name belongs to a class evaluated once (`PrivateClasses`) meets one
//! home for good: a home is made by a class evaluation, which is a closure of the class's
//! constructor (`Vm::note_class_evaluation`). A site's shape and slot recorded under that home
//! then hold for any environment, so an access compares the receiver's shape alone
//! (`PrivateSiteHomes::unique_field_slot`): the shape names the private key with its home, so
//! it proves the brand, and no environment is found. A class evaluated again, or a name eval
//! code can see, disables only the sites of its own names.

use super::{Atom, Host, ProgramId, ResidualProgram, Value, Vm};
use rustc_hash::FxHashMap;
use crate::heap::PrivateBrand;

/// Direct-mapped entries: a power of two that holds the private-name sites a request runs
/// (a web `Request`'s and `Headers`' `#fields`) with few conflicts. Hono's query route meets
/// about a hundred such reads a request; 2^7 entries missed nearly half of them, 2^10 misses
/// under a tenth.
const PRIVATE_SITE_HOME_ENTRIES: usize = 1 << 10;

#[derive(Clone, Copy)]
struct SiteHome {
    site: usize,
    environment: Value,
    home: Value,
    element: Option<PrivateElement>,
    /// The shape of the object the site last accessed the private field of under `home`
    /// (`NO_SHAPE` for none), and the field's slot in it: a private field is a shape slot
    /// (`private_slots`), so an object of that shape holds it there. A define site holds the
    /// shape its add left the object in as `slot`. A compaction of the shape table drops
    /// both (`forget_shapes`).
    shape: u32,
    slot: u32,
    /// Whether `home` was its class's when `shape` was recorded, and the site's private name
    /// that class's own (`PrivateClasses::register`): while the name stays unique
    /// (`PrivateClasses::unique_name`), `home` is the site's for good, and so is `shape`'s
    /// answer under any environment.
    unique: bool,
}

/// `SiteHome::shape` of an entry that records no shape.
const NO_SHAPE: u32 = u32::MAX;

/// `SiteHome::slot` of a read site's shape that carries the brand of a private method or
/// accessor, not a field (`record_brand_shape`): the read answers the entry's element.
const BRAND_SLOT: u32 = u32::MAX;

/// A private name's method or accessor, which an object carrying its brand reads.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PrivateElement {
    /// A private method: the read's value.
    Method(Value),
    /// A private accessor's getter, which the read calls with the object.
    Getter(Value),
}

#[derive(Default)]
pub(super) struct PrivateSiteHomes {
    entries: super::shape_memo::DirectMappedTable<SiteHome, PRIVATE_SITE_HOME_ENTRIES>,
}


impl PrivateSiteHomes {
    #[inline(always)]
    fn index(site: usize) -> usize {
        super::shape_memo::direct_mapped_index(site, PRIVATE_SITE_HOME_ENTRIES)
    }

    /// The home site `site` found for `environment`.
    #[inline(always)]
    pub(super) fn home(&self, site: usize, environment: Value) -> Option<Value> {
        let entry = self.entries.get(Self::index(site))?;
        (entry.site == site && entry.environment == environment).then_some(entry.home)
    }

    pub(super) fn record(&self, site: usize, environment: Value, home: Value) {
        let index = Self::index(site);
        // The element found under the same home stays (`element`).
        let kept = self.entries.get(index).filter(|entry| entry.site == site && entry.home == home);
        let element = kept.and_then(|entry| entry.element);
        let (shape, slot, unique) = kept.map_or((NO_SHAPE, 0, false), |entry| (entry.shape, entry.slot, entry.unique));
        self.entries.set(index, SiteHome { site, environment, home, element, shape, slot, unique });
    }

    /// The private method or accessor site `site` found for its name under `home`
    /// (`record_element`).
    #[inline(always)]
    pub(super) fn element(&self, site: usize, home: Value) -> Option<PrivateElement> {
        let entry = self.entries.get(Self::index(site))?;
        (entry.site == site && entry.home == home).then_some(entry.element).flatten()
    }

    /// Records the element site `site`'s name resolved to, for its entry under `home`.
    pub(super) fn record_element(&self, site: usize, home: Value, element: PrivateElement) {
        let index = Self::index(site);
        if let Some(entry) = self.entries.get(index)
            && entry.site == site
            && entry.home == home
        {
            self.entries.set(index, SiteHome { element: Some(element), ..entry });
        }
    }

    /// The slot of the private field site `site` reads, under the home it found for
    /// `environment`, in an object of `shape` (`record_field_slot`).
    #[inline(always)]
    pub(super) fn field_slot(&self, site: usize, environment: Value, shape: u32) -> Option<usize> {
        let entry = self.entries.get(Self::index(site))?;
        (entry.site == site && entry.environment == environment && entry.shape == shape && entry.slot != BRAND_SLOT)
            .then_some(entry.slot as usize)
    }

    /// `field_slot` under any environment, for an entry recorded under its class's home
    /// (`SiteHome::unique`), which only a site whose private name stays unique may take
    /// (`PrivateClasses::unique_name`): the shape names the field's key with that home.
    #[inline(always)]
    pub(super) fn unique_field_slot(&self, site: usize, shape: u32) -> Option<usize> {
        let entry = self.entries.get(Self::index(site))?;
        (entry.site == site && entry.shape == shape && entry.unique && entry.slot != BRAND_SLOT).then_some(entry.slot as usize)
    }

    /// The private method or accessor site `site` reads of an object of `shape`, which
    /// carries its brand (`record_brand_shape`), under any environment: as `unique_field_slot`,
    /// only for a site whose home is one value for good.
    #[inline(always)]
    pub(super) fn unique_element(&self, site: usize, shape: u32) -> Option<PrivateElement> {
        let entry = self.entries.get(Self::index(site))?;
        (entry.site == site && entry.shape == shape && entry.unique && entry.slot == BRAND_SLOT).then_some(entry.element).flatten()
    }

    /// Whether objects of `shape` carry the private name site `site` names, a field or a
    /// method's or accessor's brand (`record_field_slot`, `record_brand_shape`), under any
    /// environment: as `unique_field_slot`, only for a site whose home is one value for good.
    #[inline(always)]
    pub(super) fn unique_carrier(&self, site: usize, shape: u32) -> bool {
        self.entries.get(Self::index(site)).is_some_and(|entry| entry.site == site && entry.shape == shape && entry.unique)
    }

    /// Records that objects of `shape`, a shape no dictionary edit changes, carry the brand
    /// of the private method or accessor site `site` names under `home`, its class's for a
    /// name that class owns (`PrivateClasses::register`): a read's answers the entry's element
    /// (`unique_element`), an `in`'s is true (`unique_carrier`).
    pub(super) fn record_brand_shape(&self, site: usize, home: Value, shape: u32) {
        let index = Self::index(site);
        if let Some(entry) = self.entries.get(index)
            && entry.site == site
            && entry.home == home
        {
            self.entries.set(index, SiteHome { shape, slot: BRAND_SLOT, unique: true, ..entry });
        }
    }

    /// The shape and slot site `site` recorded under the home it found for `environment`.
    pub(super) fn field_entry(&self, site: usize, environment: Value) -> Option<(u32, usize)> {
        let entry = self.entries.get(Self::index(site))?;
        (entry.site == site && entry.environment == environment && entry.shape != NO_SHAPE && entry.slot != BRAND_SLOT)
            .then_some((entry.shape, entry.slot as usize))
    }

    /// Records that private field reads at `site` under `home` find the field at `slot` of
    /// an object of `shape`, a shape no dictionary edit changes; `unique` where `home` is its
    /// class's for a name that class owns (`PrivateClasses::register`, `SiteHome::unique`).
    pub(super) fn record_field_slot(&self, site: usize, home: Value, shape: u32, slot: usize, unique: bool) {
        let index = Self::index(site);
        if let Some(entry) = self.entries.get(index)
            && entry.site == site
            && entry.home == home
            && let Ok(slot) = u32::try_from(slot)
        {
            self.entries.set(index, SiteHome { shape, slot, unique, ..entry });
        }
    }

    /// The shape a private field define at `site` under `home` leaves an object of `shape`
    /// in (`record_define`).
    #[inline(always)]
    pub(super) fn define_shape(&self, site: usize, home: Value, shape: u32) -> Option<u32> {
        let entry = self.entries.get(Self::index(site))?;
        (entry.site == site && entry.home == home && entry.shape == shape).then_some(entry.slot)
    }

    /// Records that a private field define at `site` under `home` takes an object of
    /// `shape` to `next`, both shapes no dictionary edit changes.
    pub(super) fn record_define(&self, site: usize, home: Value, shape: u32, next: u32) {
        // A define site reads no environment: the home stands in for it, a live value.
        self.entries.set(Self::index(site), SiteHome { site, environment: home, home, element: None, shape, slot: next, unique: false });
    }

    pub(super) fn clear(&self) {
        self.entries.clear();
    }

    /// Drops every entry naming a value that `live` reports freed: its slot may name another
    /// cell from now on. Entries whose values all live stay valid across the collection.
    /// Every value the entries name (rooted by a minor collection under counts).
    pub(super) fn for_each_value(&self, mut visit: impl FnMut(Value)) {
        for index in 0..PRIVATE_SITE_HOME_ENTRIES {
            let Some(entry) = self.entries.get(index) else { continue };
            visit(entry.environment);
            visit(entry.home);
            if let Some(PrivateElement::Method(value) | PrivateElement::Getter(value)) = entry.element {
                visit(value);
            }
        }
    }

    /// Drops every recorded shape: the shape table was compacted, and its ids now name
    /// other shapes.
    pub(super) fn forget_shapes(&self) {
        for index in 0..PRIVATE_SITE_HOME_ENTRIES {
            if let Some(entry) = self.entries.get(index) {
                self.entries.set(index, SiteHome { shape: NO_SHAPE, ..entry });
            }
        }
    }

    pub(super) fn prune_dead(&self, live: impl Fn(Value) -> bool) {
        self.entries.retain(|entry| {
            let element = match entry.element {
                Some(PrivateElement::Method(value) | PrivateElement::Getter(value)) => Some(value),
                None => None,
            };
            live(entry.environment) && live(entry.home) && element.is_none_or(&live)
        });
    }
}

/// `PrivateClasses::ids` of a class constructor no class id could be given: evaluated
/// again for all the table knows.
const UNTRACKED_CLASS: u32 = u32::MAX;

/// `PrivateClasses::owners` of a private name no class owns for good: two classes' homes
/// registered it (the same identity in two programs), its class was evaluated again, or eval
/// code can name it (`PrivateClasses::share`). No name leaves it before a heap reset.
const SHARED_NAME: u32 = u32::MAX;

/// Which private names belong to a class evaluated once, so that each site of one meets one
/// home for good (`Vm::note_class_evaluation`).
///
/// A class evaluation is one closure of the class's constructor, keyed by program and
/// function; it makes the homes of the private names the class declares, the constructor
/// (static names) and its prototype (instance names), both new objects. A private name's atom
/// is its declaration's identity (`Compiler::private_name_atom`), which eval code that names
/// it shares. A site's home is the binding of its name in the scope of an evaluation of the
/// class that declares the name, the innermost one around the site: a nested class's names
/// have its homes, and a site of an inner class naming an enclosing class's name has that
/// class's home. While that class has one evaluation, the site has one home.
///
/// A name becomes a class's own when a site records a shape under a home of that class
/// (`register`), and unique (`unique_name`) until the class is evaluated again, another
/// class's home registers it, or eval code can name it: then it is shared for good, and only
/// the sites of that name stop answering from shapes alone.
#[derive(Default)]
pub(super) struct PrivateClasses {
    /// The dense id of each class constructor a closure was made of (`UNTRACKED_CLASS` past
    /// `u32` ids), by program and function. A program id that a freed program's reuses finds
    /// its classes evaluated already: a false second evaluation, which shares their names.
    ids: FxHashMap<(ProgramId, u32), u32>,
    /// Per class id: the names registered as the class's own while it was evaluated once,
    /// `None` once it was evaluated again.
    classes: Vec<Option<Vec<Atom>>>,
    /// The homes a class's one evaluation made, by value: a home of a class evaluated again
    /// is removed, so an address a freed home's took again never names a class wrongly.
    homes: FxHashMap<Value, u32>,
    /// The class id each registered name belongs to, or `SHARED_NAME`.
    owners: FxHashMap<Atom, u32>,
    /// Per atom: whether its name belongs to a class evaluated once (`owners`), the one word
    /// a site's shape-alone answer reads.
    unique: Vec<bool>,
}

crate::snapshot::snap_struct!(PrivateClasses { ids, classes, homes, owners, unique });

impl PrivateClasses {
    /// Records an evaluation of the class whose constructor is `class`, which made the homes
    /// `constructor` and `prototype`. A second one shares the names the class owns.
    fn note_evaluation(&mut self, class: (ProgramId, u32), constructor: Value, prototype: Option<Value>) {
        let homes = std::iter::once(constructor).chain(prototype);
        match self.ids.entry(class) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                let id = u32::try_from(self.classes.len()).ok().filter(|id| *id != UNTRACKED_CLASS);
                entry.insert(id.unwrap_or(UNTRACKED_CLASS));
                if let Some(id) = id {
                    self.classes.push(Some(Vec::new()));
                    homes.for_each(|home| {
                        self.homes.insert(home, id);
                    });
                }
            }
            std::collections::hash_map::Entry::Occupied(entry) => {
                let id = *entry.get() as usize;
                for name in self.classes.get_mut(id).and_then(Option::take).into_iter().flatten() {
                    self.owners.insert(name, SHARED_NAME);
                    self.unique[name as usize] = false;
                }
                homes.for_each(|home| {
                    self.homes.remove(&home);
                });
            }
        }
    }

    /// Registers private name `name` as the own name of the class whose evaluation made
    /// `home`, where a site recorded a shape under `home`: whether the name is that class's,
    /// the class evaluated once. A name another class registered is shared from now on.
    pub(super) fn register(&mut self, name: Atom, home: Value) -> bool {
        let Some(&class) = self.homes.get(&home) else { return false };
        let Some(Some(names)) = self.classes.get_mut(class as usize) else { return false };
        match self.owners.entry(name) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(class);
                names.push(name);
                let index = name as usize;
                if self.unique.len() <= index {
                    self.unique.resize(index + 1, false);
                }
                self.unique[index] = true;
                true
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                let owner = *entry.get();
                if owner != class && owner != SHARED_NAME {
                    entry.insert(SHARED_NAME);
                    self.unique[name as usize] = false;
                }
                owner == class
            }
        }
    }

    /// Shares private name `name` for good: eval code that can name it is shared by every
    /// caller of its direct-eval key, whatever home each one's evaluation found.
    fn share(&mut self, name: Atom) {
        if self.owners.insert(name, SHARED_NAME).is_some_and(|owner| owner != SHARED_NAME) {
            self.unique[name as usize] = false;
        }
    }

    /// Whether private name `name` belongs to a class evaluated once (`register`): a site's
    /// entry recorded under its class's home (`SiteHome::unique`), or a record's hint made of
    /// one, then answers from the receiver's shape alone.
    #[inline(always)]
    pub(super) fn unique_name(&self, name: Atom) -> bool {
        self.unique.get(name as usize).is_some_and(|unique| *unique)
    }

    /// Drops the homes `live` reports freed: no site meets them again, and a cell that takes
    /// one's address is a new home only through `note_evaluation`, which names it anew.
    pub(super) fn prune_dead(&mut self, live: impl Fn(Value) -> bool) {
        self.homes.retain(|home, _| live(*home));
    }

    /// Forgets every class: the heap reset discards every class closure and home.
    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }
}

impl Vm {
    /// Records a closure `constructor` of class constructor `function` of the active program,
    /// whose prototype object is `prototype`: a class evaluation, which makes the homes of the
    /// private names the class declares (`PrivateClasses`). A constructor of another program
    /// is another class's: a site's code belongs to the program whose evaluation made its
    /// function (a first-call chunk to its stub's).
    pub(super) fn note_class_evaluation(&mut self, function: u32, constructor: Value, prototype: Option<Value>) {
        self.private_classes.note_evaluation((self.active_program, function), constructor, prototype);
    }

    /// Shares the private names eval code being compiled can see, by their identities: each
    /// caller of its direct-eval key runs the same code, whatever home its class's evaluation
    /// made (`PrivateClasses::share`). Names it does not see keep their sites' answers.
    pub(super) fn share_eval_private_names(&mut self, identities: &[(String, String)]) {
        for (_, identity) in identities {
            let name = self.intern_atom(identity);
            self.private_classes.share(name);
        }
    }

    /// The private brand of `atom` at lane site `site` of `function`, whose frame captures
    /// from `environment`: the home the site found last for that environment, inline, else
    /// `site_private_brand_miss`. `None` leaves the access to the general path.
    #[inline(always)]
    pub(super) fn site_private_brand(
        &mut self,
        p: &ResidualProgram,
        site: usize,
        environment: Value,
        function: u32,
        frame: bool,
        atom: Atom,
    ) -> Option<PrivateBrand> {
        match self.private_site_homes.home(site, environment) {
            Some(home) => Some(PrivateBrand { home, name: atom }),
            None => self.site_private_brand_miss(p, site, environment, function, frame, atom),
        }
    }

    /// `site_private_brand` past the site's entry: the running frame's home
    /// (`private_name_home`) when `frame` is set, or the memoized one
    /// (`memoized_private_brand`) for a frameless activation, recorded for the site where no
    /// eval-introduced private binding can change it.
    #[inline(never)]
    fn site_private_brand_miss(
        &mut self,
        p: &ResidualProgram,
        site: usize,
        environment: Value,
        function: u32,
        frame: bool,
        atom: Atom,
    ) -> Option<PrivateBrand> {
        let brand = if frame {
            PrivateBrand { home: self.private_name_home(p, atom)?, name: atom }
        } else {
            self.memoized_private_brand(p, environment, function, atom)?
        };
        if p.function_get(function as usize).is_some_and(|body| body.binding_sites.is_empty()) {
            self.private_site_homes.record(site, environment, brand.home);
        }
        Some(brand)
    }
}
