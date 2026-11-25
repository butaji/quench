//! Method cache storage sized by the sites that recorded a cache, not by the sites a program has.
//!
//! Every installed program appends one site per method call site, and most sites of a large
//! bundle never run. A site holds only a dense index (`slots`, zero-page backed, so a page of
//! indices becomes resident once a site on it records), and the ways themselves live in
//! `entries`, which grows by recorded sites. Walks over every cache (collection, invalidation)
//! visit only recorded sites.

use super::super::site_table::CacheSiteTable;
use super::super::{EMPTY_METHOD_CACHE, MethodCache};
use super::METHOD_CACHE_WAYS;

#[quench_macros::test_visible]
pub(in crate::vm) type MethodWays = [MethodCache; METHOD_CACHE_WAYS];
const EMPTY_WAYS: MethodWays = [EMPTY_METHOD_CACHE; METHOD_CACHE_WAYS];
/// A site that recorded nothing.
const NO_SLOT: u32 = u32::MAX;
/// A slot whose site has no megamorphic set.
const NO_SET: u32 = u32::MAX;

#[quench_macros::test_visible]
#[derive(Default)]
pub(in crate::vm) struct MethodCacheTable {
    slots: CacheSiteTable<u32>,
    entries: Vec<MethodWays>,
    /// The site of each entry of `entries`; `NO_SLOT` for one in `free`.
    entry_sites: Vec<u32>,
    /// The index of the megamorphic set that took over each entry's site (`NO_SET` while the
    /// site is inline), so a call finds its set without scanning the sets of every site.
    entry_sets: Vec<u32>,
    /// Indices into `entries` a reclaimed program's sites left, empty, for the next record.
    free: Vec<u32>,
}

impl MethodCacheTable {
    #[quench_macros::test_visible]
    /// Sites the table covers.
    pub(in crate::vm) fn len(&self) -> usize {
        self.slots.len()
    }

    /// A table of `len` sites, none recorded.
    pub(in crate::vm) fn with_empty(len: usize) -> Self {
        Self { slots: CacheSiteTable::with_empty(len), ..Self::default() }
    }

    /// Appends `additional` sites, none recorded.
    pub(in crate::vm) fn extend_empty(&mut self, additional: usize) {
        self.slots.extend_empty(additional);
    }

    /// The ways of `site`, when it recorded a cache.
    #[inline(always)]
    pub(in crate::vm) fn ways(&self, site: usize) -> Option<&MethodWays> {
        let slot = self.slots.get(site);
        // SAFETY: a recorded slot names an entry; entries only grow until `clear_all`, which
        // also empties every slot.
        (slot != NO_SLOT).then(|| unsafe { self.entries.get_unchecked(slot as usize) })
    }

    /// The ways of `site`, made empty ways first when it recorded nothing.
    pub(in crate::vm) fn ways_mut(&mut self, site: usize) -> &mut MethodWays {
        let mut slot = self.slots.get(site);
        if slot == NO_SLOT {
            slot = match self.free.pop() {
                Some(slot) => slot,
                None => {
                    self.entries.push(EMPTY_WAYS);
                    self.entry_sites.push(NO_SLOT);
                    self.entry_sets.push(NO_SET);
                    (self.entries.len() - 1) as u32
                }
            };
            self.slots.set(site, slot);
            self.entry_sites[slot as usize] = site as u32;
        }
        &mut self.entries[slot as usize]
    }

    /// The index of the megamorphic set that took over `site`, when one did.
    ///
    /// Sound because a site's slot holds at most one set index, and the index is written only
    /// by `set_megamorphic_index`, which the VM keeps in step with its set list, so a reader
    /// sees the index of the set made for this site, or none.
    #[inline(always)]
    pub(in crate::vm) fn megamorphic_index(&self, site: usize) -> Option<usize> {
        let slot = if site < self.len() { self.slots.get(site) } else { NO_SLOT };
        (slot != NO_SLOT)
            .then(|| self.entry_sets[slot as usize])
            .filter(|index| *index != NO_SET)
            .map(|index| index as usize)
    }

    /// Records (or, with `None`, forgets) the megamorphic set of `site`, which must have
    /// recorded a cache. Sound as `megamorphic_index`: the VM calls this whenever it creates a
    /// set or renumbers the kept sets, so the index always names the set at that position.
    pub(in crate::vm) fn set_megamorphic_index(&mut self, site: usize, index: Option<usize>) {
        let slot = self.slots.get(site);
        if slot != NO_SLOT {
            self.entry_sets[slot as usize] = index.map_or(NO_SET, |index| index as u32);
        }
    }

    /// The ways of `site`, when it recorded a cache, mutably.
    pub(in crate::vm) fn recorded_ways_mut(&mut self, site: usize) -> Option<&mut MethodWays> {
        let slot = if site < self.len() { self.slots.get(site) } else { NO_SLOT };
        (slot != NO_SLOT).then(|| &mut self.entries[slot as usize])
    }

    /// Every recorded site with its ways, mutably, in no particular order.
    pub(in crate::vm) fn recorded_sites_mut(&mut self) -> impl Iterator<Item = (usize, &mut MethodWays)> {
        self.entry_sites
            .iter()
            .zip(self.entries.iter_mut())
            .filter(|(site, _)| **site != NO_SLOT)
            .map(|(site, ways)| (*site as usize, ways))
    }

    /// Every recorded site's ways, in no particular order (sites emptied by a reclaimed program
    /// read as empty ways).
    #[cfg(any(test, feature = "profile-aggregate"))]
    pub(in crate::vm) fn recorded(&self) -> impl Iterator<Item = &MethodWays> {
        self.entries.iter()
    }

    /// `recorded`, mutably.
    pub(in crate::vm) fn recorded_mut(&mut self) -> impl Iterator<Item = &mut MethodWays> {
        self.entries.iter_mut()
    }

    /// Every recorded site with its ways.
    #[cfg(feature = "profile-aggregate")]
    pub(in crate::vm) fn sites(&self) -> impl Iterator<Item = (usize, &MethodWays)> {
        (0..self.len()).filter_map(|site| Some((site, self.ways(site)?)))
    }

    /// Empties every site, keeping the site count.
    pub(in crate::vm) fn clear_all(&mut self) {
        self.slots.reset();
        self.entries.clear();
        self.entry_sites.clear();
        self.entry_sets.clear();
        self.free.clear();
    }

    /// Empties the sites in `sites`, whose program was reclaimed.
    /// The site indices only read here; their pages are then handed back
    /// (`SiteTable::empty_range`), so a range's empty sites stay off the resident set.
    pub(in crate::vm) fn empty_sites(&mut self, sites: std::ops::Range<usize>) {
        for site in sites.clone() {
            let slot = self.slots.get(site);
            if slot != NO_SLOT {
                self.entries[slot as usize] = EMPTY_WAYS;
                self.entry_sites[slot as usize] = NO_SLOT;
                self.entry_sets[slot as usize] = NO_SET;
                self.free.push(slot);
            }
        }
        self.slots.empty_range(sites);
    }

    /// Drops every site.
    pub(in crate::vm) fn clear(&mut self) {
        self.slots.clear();
        self.entries.clear();
        self.entry_sites.clear();
        self.entry_sets.clear();
        self.free.clear();
    }

    /// Bytes held: the index mapping (resident or not) and the recorded ways.
    pub(in crate::vm) fn capacity_bytes(&self) -> usize {
        self.slots.capacity_bytes()
            + self.entries.capacity() * size_of::<MethodWays>()
            + (self.free.capacity() + self.entry_sites.capacity() + self.entry_sets.capacity())
                * size_of::<u32>()
    }

    /// Appends one site holding `ways`.
    #[cfg(any(test, feature = "test-support"))]
    #[quench_macros::test_visible]
    pub(in crate::vm) fn push(&mut self, ways: MethodWays) {
        self.extend_empty(1);
        *self.ways_mut(self.len() - 1) = ways;
    }

    /// Every recorded site's ways.
    #[cfg(test)]
    pub(in crate::vm) fn iter(&self) -> impl Iterator<Item = &MethodWays> {
        self.recorded()
    }
}

impl std::ops::Index<usize> for MethodCacheTable {
    type Output = MethodWays;
    fn index(&self, site: usize) -> &MethodWays {
        self.ways(site).unwrap_or(&EMPTY_WAYS)
    }
}
