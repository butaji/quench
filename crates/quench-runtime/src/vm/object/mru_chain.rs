//! The most-recently-recorded chains of a site's inherited and accessor field entries.

use super::*;
use crate::vm::compiled::tier_policy::SitePolicy;

/// The `next` of a chain's oldest entry, and the head of a site with none.
pub(in crate::vm) const MRU_CHAIN_END: u32 = u32::MAX;

/// An entry of a site's most-recently-recorded chain: entries link newest to oldest through
/// `next`, ending at `MRU_CHAIN_END`, and a site keeps one entry per receiver shape.
pub(super) trait MruEntry {
    fn receiver(&self) -> u32;
    fn next(&self) -> u32;
    fn set_next(&mut self, next: u32);
}

/// Storage that holds the entries of every site's chain, addressed by index.
pub(super) trait MruPool {
    type Entry: MruEntry;
    fn entry(&self, index: u32) -> &Self::Entry;
    fn entry_mut(&mut self, index: u32) -> &mut Self::Entry;
    fn push(&mut self, entry: Self::Entry) -> u32;
}

/// Records `entry` in the chain that starts at `head`: an entry for its receiver shape is
/// replaced in place, a chain the policy's `after_miss` finds full reuses its oldest one's
/// storage, and any other entry is added. Returns the chain's new head, or `None` when the head is unchanged.
/// The policy has more than one way, so the oldest entry is never the head the new entry
/// links to.
pub(super) fn record_mru_entry<P: MruPool>(pool: &mut P, head: u32, policy: &SitePolicy, mut entry: P::Entry) -> Option<u32> {
    let (mut index, mut previous, mut length) = (head, MRU_CHAIN_END, 0);
    while index != MRU_CHAIN_END {
        let cached = pool.entry(index);
        let next = cached.next();
        if cached.receiver() == entry.receiver() {
            entry.set_next(next);
            *pool.entry_mut(index) = entry;
            return None;
        }
        length += 1;
        if next == MRU_CHAIN_END {
            break;
        }
        (previous, index) = (index, next);
    }
    entry.set_next(head);
    let slot = match SiteState::after_miss(length, policy) {
        Admission::Append => pool.push(entry),
        Admission::EvictOldest => {
            // The oldest entry, `index`, follows `previous`.
            pool.entry_mut(previous).set_next(MRU_CHAIN_END);
            *pool.entry_mut(index) = entry;
            index
        }
        Admission::Spill | Admission::Reject => return None,
    };
    Some(slot)
}

impl MruEntry for AccessorFieldCache {
    fn receiver(&self) -> u32 {
        self.receiver
    }
    fn next(&self) -> u32 {
        self.next
    }
    fn set_next(&mut self, next: u32) {
        self.next = next;
    }
}

impl MruPool for Vec<AccessorFieldCache> {
    type Entry = AccessorFieldCache;
    fn entry(&self, index: u32) -> &AccessorFieldCache {
        &self[index as usize]
    }
    fn entry_mut(&mut self, index: u32) -> &mut AccessorFieldCache {
        &mut self[index as usize]
    }
    fn push(&mut self, entry: AccessorFieldCache) -> u32 {
        Vec::push(self, entry);
        (self.len() - 1) as u32
    }
}

impl MruEntry for InheritedFieldCache {
    fn receiver(&self) -> u32 {
        self.receiver
    }
    fn next(&self) -> u32 {
        self.next
    }
    fn set_next(&mut self, next: u32) {
        self.next = next;
    }
}

impl MruPool for crate::heap::Dependencies {
    type Entry = InheritedFieldCache;
    fn entry(&self, index: u32) -> &InheritedFieldCache {
        self.inherited(index)
    }
    fn entry_mut(&mut self, index: u32) -> &mut InheritedFieldCache {
        self.inherited_mut(index)
    }
    fn push(&mut self, entry: InheritedFieldCache) -> u32 {
        self.push_inherited(entry)
    }
}
