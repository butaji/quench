//! The class home a lane private-name access found last, per access site. A private name's
//! home depends on the running function and the environment its frame captures from, never
//! on the accessed object (`Vm::private_name_home`); a lane record belongs to one function of
//! one program, so its site fixes all but the environment. An entry names the environment
//! and the collection epoch it was found under: the environment value names the same cell,
//! and so the same home, while no collection runs. Only functions without binding sites
//! record, whose homes no eval-introduced private binding can change.

use super::Value;
use std::cell::Cell;

/// Direct-mapped entries: a power of two that holds the private-name sites a request runs
/// (a web `Request`'s and `Headers`' `#fields`).
const PRIVATE_SITE_HOME_ENTRIES: usize = 1 << 7;

#[derive(Clone, Copy)]
struct SiteHome {
    site: usize,
    environment: Value,
    epoch: u64,
    home: Value,
}

pub(super) struct PrivateSiteHomes {
    entries: Box<[Cell<Option<SiteHome>>]>,
}

impl Default for PrivateSiteHomes {
    fn default() -> Self {
        Self { entries: (0..PRIVATE_SITE_HOME_ENTRIES).map(|_| Cell::new(None)).collect() }
    }
}

impl PrivateSiteHomes {
    fn index(site: usize) -> usize {
        super::shape_memo::direct_mapped_index(site, PRIVATE_SITE_HOME_ENTRIES)
    }

    /// The home site `site` found for `environment` under collection `epoch`.
    #[inline(always)]
    pub(super) fn home(&self, site: usize, environment: Value, epoch: u64) -> Option<Value> {
        let entry = self.entries[Self::index(site)].get()?;
        (entry.site == site && entry.environment == environment && entry.epoch == epoch).then_some(entry.home)
    }

    pub(super) fn record(&self, site: usize, environment: Value, epoch: u64, home: Value) {
        self.entries[Self::index(site)].set(Some(SiteHome { site, environment, epoch, home }));
    }

    pub(super) fn clear(&self) {
        for entry in self.entries.iter() {
            entry.set(None);
        }
    }
}
