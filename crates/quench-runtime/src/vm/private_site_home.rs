//! The private atom a lane private-name access found last, per access site. A private name's
//! atom is the one its class evaluation's home declared (`Vm::private_name_atom`), which depends
//! on the running function and the environment its frame captures from, never on the accessed
//! object; a lane record belongs to one function of one program, so its site fixes all but the
//! environment. An entry names the environment, which names the same cell, and so the same home
//! and atom, for as long as it lives: each collection drops the entries a value of which it freed
//! (`prune_dead`), as `Vm::private_homes` drops its own, and keeps the rest. Only functions
//! without binding sites record, whose homes no eval-introduced private binding can change.
//!
//! An entry also keeps the private method or accessor the site's name resolved to under that
//! atom, once found (`PrivateElement`): a class evaluation makes its private methods and
//! accessors once, under its own atoms, and no program can redefine them. The element follows
//! the atom alone, so it outlives a new environment that finds the same atom again (an arrow
//! field's closure per instance).

use super::{Atom, Host, ResidualProgram, Value, Vm};
use crate::heap::PrivateAtom;

/// Direct-mapped entries: a power of two that holds the private-name sites a request runs
/// (a web `Request`'s and `Headers`' `#fields`) with few conflicts. Hono's query route meets
/// about a hundred such reads a request; 2^7 entries missed nearly half of them, 2^10 misses
/// under a tenth.
const PRIVATE_SITE_HOME_ENTRIES: usize = 1 << 10;

#[derive(Clone, Copy)]
struct SiteHome {
    site: usize,
    environment: Value,
    atom: PrivateAtom,
    element: Option<PrivateElement>,
}

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
    fn index(site: usize) -> usize {
        super::shape_memo::direct_mapped_index(site, PRIVATE_SITE_HOME_ENTRIES)
    }

    /// The private atom site `site` found for `environment`.
    #[inline(always)]
    pub(super) fn atom(&self, site: usize, environment: Value) -> Option<PrivateAtom> {
        let entry = self.entries.get(Self::index(site))?;
        (entry.site == site && entry.environment == environment).then_some(entry.atom)
    }

    pub(super) fn record(&self, site: usize, environment: Value, atom: PrivateAtom) {
        let index = Self::index(site);
        // The element found under the same atom stays (`element`).
        let element = self
            .entries
            .get(index)
            .filter(|entry| entry.site == site && entry.atom == atom)
            .and_then(|entry| entry.element);
        self.entries.set(index, SiteHome { site, environment, atom, element });
    }

    /// The private method or accessor site `site` found for its name under `atom`
    /// (`record_element`).
    #[inline(always)]
    pub(super) fn element(&self, site: usize, atom: PrivateAtom) -> Option<PrivateElement> {
        let entry = self.entries.get(Self::index(site))?;
        (entry.site == site && entry.atom == atom).then_some(entry.element).flatten()
    }

    /// Records the element site `site`'s name resolved to, for its entry under `atom`.
    pub(super) fn record_element(&self, site: usize, atom: PrivateAtom, element: PrivateElement) {
        let index = Self::index(site);
        if let Some(entry) = self.entries.get(index)
            && entry.site == site
            && entry.atom == atom
        {
            self.entries.set(index, SiteHome { element: Some(element), ..entry });
        }
    }

    pub(super) fn clear(&self) {
        self.entries.clear();
    }

    /// Drops every entry naming a value that `live` reports freed: its slot may name another
    /// cell from now on. Entries whose values all live stay valid across the collection.
    pub(super) fn prune_dead(&self, live: impl Fn(Value) -> bool) {
        self.entries.retain(|entry| {
            let element = match entry.element {
                Some(PrivateElement::Method(value) | PrivateElement::Getter(value)) => Some(value),
                None => None,
            };
            live(entry.environment) && element.is_none_or(&live)
        });
    }
}

impl<H: Host> Vm<H> {
    /// The private atom of `name` at lane site `site` of `function`, whose frame captures
    /// from `environment`: the atom the site found last for that environment, inline, else
    /// `site_private_atom_miss`. `None` leaves the access to the general path.
    #[inline(always)]
    pub(super) fn site_private_atom(
        &mut self,
        p: &ResidualProgram,
        site: usize,
        environment: Value,
        function: u32,
        frame: bool,
        name: Atom,
    ) -> Option<PrivateAtom> {
        match self.private_site_homes.atom(site, environment) {
            Some(atom) => Some(atom),
            None => self.site_private_atom_miss(p, site, environment, function, frame, name),
        }
    }

    /// `site_private_atom` past the site's entry: the running frame's atom
    /// (`private_name_atom`) when `frame` is set, or the memoized one
    /// (`memoized_private_atom`) for a frameless activation, recorded for the site where no
    /// eval-introduced private binding can change it.
    #[inline(never)]
    fn site_private_atom_miss(
        &mut self,
        p: &ResidualProgram,
        site: usize,
        environment: Value,
        function: u32,
        frame: bool,
        name: Atom,
    ) -> Option<PrivateAtom> {
        let atom = if frame {
            self.private_name_atom(p, name)?
        } else {
            self.memoized_private_atom(p, environment, function, name)?
        };
        if p.function_get(function as usize).is_some_and(|body| body.binding_sites.is_empty()) {
            self.private_site_homes.record(site, environment, atom);
        }
        Some(atom)
    }
}
