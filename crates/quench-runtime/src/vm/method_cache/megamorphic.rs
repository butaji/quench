//! The megamorphic sets of method sites whose inline ways collided: where a site's set lives
//! (`Vm::megamorphic_method_indices`), how it is probed and what its invalidations retain.
use super::*;

impl<H: Host> Vm<H> {
    /// Drops every megamorphic set and the site index that names them.
    pub(in crate::vm) fn clear_megamorphic_methods(&mut self) {
        self.megamorphic_methods.clear();
        self.megamorphic_method_indices.clear();
    }

    /// Drops the megamorphic sets an invalidation emptied and renumbers the site index of the
    /// rest: an event of invalidation, not of a lookup.
    pub(super) fn retire_empty_method_sets(&mut self) {
        self.megamorphic_methods.retain(|set| set.len != 0);
        self.megamorphic_method_indices.fill(NO_MEGAMORPHIC_METHOD);
        for (position, set) in self.megamorphic_methods.iter().enumerate() {
            self.megamorphic_method_indices[set.site] = position as u32;
        }
    }

    /// The position in `megamorphic_methods` of the set of method site `site`, if it has one.
    pub(super) fn megamorphic_set_position(&self, site: usize) -> Option<usize> {
        let position = *self.megamorphic_method_indices.get(site)? as usize;
        (position < self.megamorphic_methods.len()).then_some(position)
    }

    /// Promotes `site`, whose inline way for `cache` holds another entry, to a megamorphic set of
    /// its inline entries and `cache`.
    pub(super) fn promote_method_site(&mut self, site: usize, cache: MethodCache) {
        let mut set = MethodCacheSet { site, len: 0, entries: [EMPTY_METHOD_CACHE; crate::vm::compiled::tier_policy::POLYMORPHISM] };
        for entry in self.method_caches[site].iter().chain([&cache]).filter(|entry| entry.target.is_some()) {
            set.entries[usize::from(set.len)] = *entry;
            set.len += 1;
        }
        if self.megamorphic_method_indices.len() <= site {
            self.megamorphic_method_indices.resize(site + 1, NO_MEGAMORPHIC_METHOD);
        }
        self.megamorphic_method_indices[site] = self.megamorphic_methods.len() as u32;
        self.megamorphic_methods.push(set);
    }

    /// The entry of the megamorphic set of method site `site` that `serves`: a probe of at most
    /// `POLYMORPHISM` entries.
    pub(super) fn megamorphic_method(&self, site: usize, serves: impl Fn(&MethodCache) -> bool) -> Option<MethodCache> {
        let set = &self.megamorphic_methods[self.megamorphic_set_position(site)?];
        for entry in &set.entries[..usize::from(set.len)] {
            if serves(entry) {
                return Some(*entry);
            }
        }
        None
    }
}

pub(super) fn retain_other_atom(entries: &mut [MethodCache], len: usize, atom: Atom) -> usize {
    let mut retained = 0;
    for index in 0..len {
        let entry = entries[index];
        if entry.atom != atom {
            entries[retained] = entry;
            retained += 1;
        }
    }
    entries[retained..].fill(EMPTY_METHOD_CACHE);
    retained
}

pub(super) fn retain_unshadowed_entries(
    heap: &crate::heap::Heap,
    entries: &mut [MethodCache],
    len: usize,
    object: Value,
    atom: Atom,
) -> usize {
    let mut retained = 0;
    for index in 0..len {
        let entry = entries[index];
        if !method_cache_shadowed_by(heap, entry, object, atom) {
            entries[retained] = entry;
            retained += 1;
        }
    }
    entries[retained..].fill(EMPTY_METHOD_CACHE);
    retained
}

pub(super) fn retain_entries(heap: &crate::heap::Heap, entries: &mut [MethodCache], len: usize) -> usize {
    let mut retained = 0;
    for index in 0..len {
        let entry = entries[index];
        if method_cache_live(heap, entry) {
            entries[retained] = entry;
            retained += 1;
        }
    }
    entries[retained..].fill(EMPTY_METHOD_CACHE);
    retained
}

