//! A method call site's entries (`Vm::method_caches`): the miss path that records them and
//! the retirement that prunes them. A site's lookup reads them in `method_cache`.

use super::*;

/// One method call site's entries: `SITE_POLICIES.method.inline_ways` inline, most recent first, and a set
/// that holds up to `POLYMORPHISM` entries once a third receiver shape reaches the site. The
/// site is a slot of `Vm::method_caches`, so a lookup reaches its set without a search.
#[derive(Clone)]
pub(super) struct MethodSite {
    pub(super) inline: [MethodCache; SITE_POLICIES.method.inline_ways],
    pub(super) set: Option<Box<MethodCacheSet>>,
}

/// The entries of a site past its inline ways, oldest first: the inline entries as they
/// were when the set began, then each later one. A full set evicts its oldest entry.
#[derive(Clone)]
pub(super) struct MethodCacheSet {
    len: u8,
    entries: [MethodCache; super::compiled::tier_policy::POLYMORPHISM],
}

impl std::ops::Deref for MethodSite {
    type Target = [MethodCache; SITE_POLICIES.method.inline_ways];
    fn deref(&self) -> &Self::Target {
        &self.inline
    }
}

impl MethodSite {
    pub(super) const EMPTY: Self = Self { inline: [EMPTY_METHOD_CACHE; SITE_POLICIES.method.inline_ways], set: None };

    /// Every entry the site holds: the inline ways, then the set's.
    #[cfg(any(test, feature = "profile-aggregate"))]
    pub(super) fn entries(&self) -> impl Iterator<Item = &MethodCache> {
        self.inline.iter().chain(self.recorded_set())
    }

    /// The set's entries, none while the site keeps to its inline ways.
    pub(super) fn recorded_set(&self) -> &[MethodCache] {
        self.set.as_ref().map_or(&[], |set| &set.entries[..usize::from(set.len)])
    }

    /// Records `cache` as the site's newest entry: inline while the ways hold, then in the
    /// set. An inline entry the set also holds is refreshed with it, so a lookup that finds
    /// the inline copy first never meets a stale one.
    pub(super) fn record(&mut self, cache: MethodCache) {
        if let Some(set) = &mut self.set {
            for entry in self.inline.iter_mut().filter(|entry| entry.records_same(&cache)) {
                *entry = cache;
            }
            set.record(cache);
        } else if self.inline.iter().any(|entry| entry.records_same(&cache)) || self.inline[1].target.is_none() {
            self.inline[1] = self.inline[0];
            self.inline[0] = cache;
        } else {
            let mut set = MethodCacheSet { len: 0, entries: [EMPTY_METHOD_CACHE; super::compiled::tier_policy::POLYMORPHISM] };
            for entry in self.inline.into_iter().rev().chain([cache]) {
                set.record(entry);
            }
            self.set = Some(Box::new(set));
        }
    }

    /// Keeps the entries `keep` accepts, in order; a set left empty is dropped.
    pub(super) fn retain(&mut self, mut keep: impl FnMut(&MethodCache) -> bool) {
        retain_compact(&mut self.inline, &mut keep);
        if let Some(set) = &mut self.set {
            set.len = retain_compact(&mut set.entries[..usize::from(set.len)], &mut keep) as u8;
            if set.len == 0 {
                self.set = None;
            }
        }
    }
}

impl MethodCacheSet {
    /// Replaces the entry that records the same lookup and callee, else appends, evicting
    /// the oldest entry when the set is full.
    pub(super) fn record(&mut self, cache: MethodCache) {
        let len = usize::from(self.len);
        if let Some(entry) = self.entries[..len].iter_mut().find(|entry| entry.records_same(&cache)) {
            *entry = cache;
            return;
        }
        match SiteState::after_miss(len, &SITE_POLICIES.method) {
            Admission::Append => {
                self.entries[len] = cache;
                self.len += 1;
            }
            Admission::EvictOldest => {
                self.entries.copy_within(1.., 0);
                self.entries[len - 1] = cache;
            }
            Admission::Spill | Admission::Reject => {}
        }
    }
}

/// Moves the entries `keep` accepts to the front of `entries`, in order, empties the rest and
/// counts the kept ones.
fn retain_compact(entries: &mut [MethodCache], keep: &mut impl FnMut(&MethodCache) -> bool) -> usize {
    let mut retained = 0;
    for index in 0..entries.len() {
        let entry = entries[index];
        if keep(&entry) {
            entries[retained] = entry;
            retained += 1;
        }
    }
    entries[retained..].fill(EMPTY_METHOD_CACHE);
    retained
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_method_set_evicts_its_oldest_receiver_shape() {
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        vm.method_caches.push(MethodSite::EMPTY);
        for shape in 1..=6 {
            vm.record_method_cache(
                0,
                MethodCache {
                    shape,
                    atom: 0,
                    proto: crate::Value::NULL,
                    callee: crate::Value::NULL,
                    guard: EMPTY_METHOD_GUARD,
                    target: Some(CallTarget::User(program_store::ProgramId::MAIN, shape, crate::Value::NULL)),
                },
            );
        }
        let set: Vec<u32> = vm.method_caches[0].recorded_set().iter().map(|entry| entry.shape).collect();
        assert_eq!(set, [3, 4, 5, 6], "the set keeps the newest shapes, oldest first");
    }
}
