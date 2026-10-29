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

use super::Value;

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
}

/// A private name's method or accessor, which an object carrying its brand reads.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PrivateElement {
    /// A private method: the read's value.
    Method(Value),
    /// A private accessor's getter, which the read calls with the object.
    Getter(Value),
}

pub(super) struct PrivateSiteHomes {
    entries: super::shape_memo::DirectMappedTable<SiteHome, PRIVATE_SITE_HOME_ENTRIES>,
}

impl Default for PrivateSiteHomes {
    fn default() -> Self {
        Self { entries: Default::default() }
    }
}

impl PrivateSiteHomes {
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
        let element = self
            .entries
            .get(index)
            .filter(|entry| entry.site == site && entry.home == home)
            .and_then(|entry| entry.element);
        self.entries.set(index, SiteHome { site, environment, home, element });
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
            live(entry.environment) && live(entry.home) && element.is_none_or(&live)
        });
    }
}
