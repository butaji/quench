//! Bounded disposable memoization for published stencil entries.

use crate::stencil_fact::RegionKey;
use std::sync::atomic::{AtomicUsize, Ordering};

pub const MAX_RENDERED_REGIONS: usize = 16;
const MAX_GLOBAL_RENDERED_CACHE_BYTES: usize = 4 << 20;
static GLOBAL_RENDERED_CACHE_BYTES: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RenderedRegion {
    pub key: RegionKey,
    pub signature: u64,
    pub address: usize,
    pub owner: u64,
}

/// Disposable memo table with a process-wide metadata budget.
#[derive(Debug)]
pub struct RenderedRegionCache {
    entries: Vec<RenderedRegion>,
    next: usize,
    reserved_bytes: usize,
}

impl Default for RenderedRegionCache {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderedRegionCache {
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
            next: 0,
            reserved_bytes: 0,
        }
    }

    pub fn get(&self, key: RegionKey, signature: u64) -> Option<usize> {
        self.find(key, signature, None).map(|entry| entry.address)
    }

    pub fn get_owned(&self, key: RegionKey, signature: u64, owner: u64) -> Option<usize> {
        self.find(key, signature, Some(owner))
            .map(|entry| entry.address)
    }

    pub fn insert(&mut self, key: RegionKey, signature: u64, address: usize) -> usize {
        self.insert_owned(key, signature, address, 0)
    }

    pub fn insert_owned(
        &mut self,
        key: RegionKey,
        signature: u64,
        address: usize,
        owner: u64,
    ) -> usize {
        if self.update(key, signature, address, owner) {
            return address;
        }
        if !self.ensure_storage() {
            return address;
        }
        self.insert_new(RenderedRegion {
            key,
            signature,
            address,
            owner,
        });
        address
    }

    pub fn remove(&mut self, key: RegionKey, signature: u64, address: usize) -> bool {
        let Some(index) = self.entries.iter().position(|entry| {
            entry.key == key && entry.signature == signature && entry.address == address
        }) else {
            return false;
        };
        self.entries.remove(index);
        self.release_if_empty();
        true
    }

    pub(crate) fn remove_owner(&mut self, owner: u64) -> usize {
        let before = self.entries.len();
        self.entries.retain(|entry| entry.owner != owner);
        self.release_if_empty();
        before - self.entries.len()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub const fn capacity(&self) -> usize {
        MAX_RENDERED_REGIONS
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.release_storage();
    }

    fn find(&self, key: RegionKey, signature: u64, owner: Option<u64>) -> Option<&RenderedRegion> {
        self.entries.iter().find(|entry| {
            entry.key == key
                && entry.signature == signature
                && owner.is_none_or(|expected| entry.owner == expected)
        })
    }

    fn update(&mut self, key: RegionKey, signature: u64, address: usize, owner: u64) -> bool {
        let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.key == key && entry.signature == signature && entry.owner == owner)
        else {
            return false;
        };
        entry.address = address;
        true
    }

    fn insert_new(&mut self, entry: RenderedRegion) {
        if self.entries.len() < MAX_RENDERED_REGIONS {
            self.entries.push(entry);
            return;
        }
        self.entries[self.next] = entry;
        self.next = (self.next + 1) % MAX_RENDERED_REGIONS;
    }

    fn ensure_storage(&mut self) -> bool {
        if self.reserved_bytes != 0 {
            return true;
        }
        let requested = cache_bytes(MAX_RENDERED_REGIONS);
        if !reserve_global(requested) {
            return false;
        }
        if self
            .entries
            .try_reserve_exact(MAX_RENDERED_REGIONS)
            .is_err()
        {
            release_global(requested);
            return false;
        }
        let allocated = cache_bytes(self.entries.capacity());
        if allocated > requested && !reserve_global(allocated - requested) {
            self.entries = Vec::new();
            release_global(requested);
            return false;
        }
        if allocated < requested {
            release_global(requested - allocated);
        }
        self.reserved_bytes = allocated;
        true
    }

    fn release_if_empty(&mut self) {
        if self.entries.is_empty() {
            self.release_storage();
        }
    }

    fn release_storage(&mut self) {
        self.entries = Vec::new();
        self.next = 0;
        release_global(std::mem::take(&mut self.reserved_bytes));
    }

    #[cfg(test)]
    pub(crate) fn allocated_entries(&self) -> usize {
        self.entries.capacity()
    }

    #[cfg(test)]
    pub(crate) fn allocated_bytes(&self) -> usize {
        self.reserved_bytes
    }
}

impl Drop for RenderedRegionCache {
    fn drop(&mut self) {
        release_global(self.reserved_bytes);
    }
}

fn cache_bytes(entries: usize) -> usize {
    entries.saturating_mul(std::mem::size_of::<RenderedRegion>())
}

fn reserve_global(bytes: usize) -> bool {
    GLOBAL_RENDERED_CACHE_BYTES
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
            used.checked_add(bytes)
                .filter(|next| *next <= MAX_GLOBAL_RENDERED_CACHE_BYTES)
        })
        .is_ok()
}

fn release_global(bytes: usize) {
    if bytes != 0 {
        GLOBAL_RENDERED_CACHE_BYTES.fetch_sub(bytes, Ordering::AcqRel);
    }
}

#[cfg(test)]
pub(crate) fn global_rendered_cache_bytes() -> usize {
    GLOBAL_RENDERED_CACHE_BYTES.load(Ordering::Acquire)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cold_cache_is_free_and_clear_releases_reservation() {
        let mut cache = RenderedRegionCache::new();
        assert_eq!(cache.allocated_bytes(), 0);
        cache.insert(RegionKey(1), 0, 7);
        assert_eq!(cache.allocated_bytes(), cache_bytes(MAX_RENDERED_REGIONS));
        assert!(global_rendered_cache_bytes() <= MAX_GLOBAL_RENDERED_CACHE_BYTES);
        cache.clear();
        assert_eq!(cache.allocated_bytes(), 0);
    }

    #[test]
    fn final_owner_removal_releases_reservation() {
        let mut cache = RenderedRegionCache::new();
        cache.insert_owned(RegionKey(1), 0, 7, 11);
        cache.insert_owned(RegionKey(2), 0, 9, 12);
        assert_eq!(cache.remove_owner(11), 1);
        assert_ne!(cache.allocated_bytes(), 0);
        assert_eq!(cache.remove_owner(12), 1);
        assert_eq!(cache.allocated_bytes(), 0);
    }

    #[test]
    fn replacement_stays_inside_one_reservation() {
        let mut cache = RenderedRegionCache::new();
        for index in 0..(MAX_RENDERED_REGIONS * 4) {
            cache.insert(RegionKey(index as u64), 0, index);
        }
        assert_eq!(cache.len(), MAX_RENDERED_REGIONS);
        assert_eq!(cache.allocated_bytes(), cache_bytes(MAX_RENDERED_REGIONS));
        assert!(global_rendered_cache_bytes() <= MAX_GLOBAL_RENDERED_CACHE_BYTES);
    }
}
