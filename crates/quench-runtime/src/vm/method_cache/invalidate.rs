//! Dropping the method cache entries an atom's change makes stale.

use super::super::*;

impl<H: Host> Vm<H> {
    /// Drops every entry naming `atom`, which only the sites listed for it hold.
    pub(in crate::vm) fn invalidate_method_caches_for_atom(&mut self, atom: Atom) {
        if !self.method_caches_may_name(atom) {
            return;
        }
        #[cfg(feature = "profile-aggregate")]
        self.snapshot_method_caches_for_atom(atom, 1);
        let named = self.method_cache_atoms.take_sites(atom);
        for site in named.sites {
            if let Some(entries) = self.method_caches.get_mut(site as usize) {
                let len = entries.len();
                retain_other_atom(entries, len, atom);
            }
        }
        if named.megamorphic {
            for set in &mut self.megamorphic_methods {
                set.len = retain_other_atom(&mut set.entries, usize::from(set.len), atom) as u8;
            }
            self.megamorphic_methods.retain(|set| set.len != 0);
        }
    }

    /// Drops the entries for `atom` that a new `atom` property of prototype `object` shadows.
    pub(in crate::vm) fn invalidate_method_caches_for_prototype_add(&mut self, object: Value, atom: Atom) {
        #[cfg(feature = "profile-aggregate")]
        self.snapshot_shadowed_method_caches(object, atom);
        let Some(named) = self.method_cache_atoms.sites_of(atom) else {
            return;
        };
        let heap = &self.heap;
        for &site in &named.sites {
            if let Some(entries) = self.method_caches.get_mut(site as usize) {
                let len = entries.len();
                retain_unshadowed_entries(heap, entries, len, object, atom);
            }
        }
        if named.megamorphic {
            for set in &mut self.megamorphic_methods {
                set.len = retain_unshadowed_entries(
                    heap,
                    &mut set.entries,
                    usize::from(set.len),
                    object,
                    atom,
                ) as u8;
            }
            self.megamorphic_methods.retain(|set| set.len != 0);
        }
    }
}

fn retain_other_atom(entries: &mut [MethodCache], len: usize, atom: Atom) -> usize {
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

fn retain_unshadowed_entries(
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

pub(super) fn method_cache_shadowed_by(
    heap: &crate::heap::Heap,
    entry: MethodCache,
    object: Value,
    atom: Atom,
) -> bool {
    if entry.atom != atom || entry.guard.depth <= 1 {
        return false;
    }
    let mut prototype = entry.proto;
    for _ in 1..entry.guard.depth {
        if prototype == object {
            return true;
        }
        let Some(data) = heap.object_record(prototype) else {
            return false;
        };
        prototype = data.proto();
    }
    false
}
