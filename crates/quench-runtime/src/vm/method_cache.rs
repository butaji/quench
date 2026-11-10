#![allow(dead_code)]
use super::*;
mod atoms;
pub(super) use atoms::{AtomBits, MethodCacheAtoms};

/// Entries of a call site's inline method cache, most recent first; a site that sees more
/// moves to a megamorphic set.
pub(super) const METHOD_CACHE_WAYS: usize = 2;
/// The shape a method lookup on a receiver without object data compares: it matches no
/// recorded entry, the empty entry included.
const NON_OBJECT_RECEIVER_SHAPE: u32 = u32::MAX - 1;

impl<H: Host> Vm<H> {
    pub(super) fn invalidate_method_caches(&mut self) {
        self.clear_method_caches(1);
    }

    pub(super) fn clear_method_caches(&mut self, _reason: usize) {
        #[cfg(feature = "profile-aggregate")]
        self.snapshot_method_caches(_reason);
        self.method_caches.fill(MethodSite::EMPTY);
        self.method_cache_atoms.clear();
    }

    /// Whether any method cache may name `atom`; `false` proves that none does.
    #[inline(always)]
    pub(super) fn method_caches_may_name(&self, atom: Atom) -> bool {
        self.method_cache_atoms.contains(atom)
    }

    pub(super) fn invalidate_method_caches_for_atom(&mut self, atom: Atom) {
        #[cfg(feature = "profile-aggregate")]
        self.snapshot_method_caches_for_atom(atom, 1);
        for site in &mut self.method_caches {
            site.retain(|entry| entry.atom != atom);
        }
    }

    pub(super) fn invalidate_method_caches_for_prototype_add(&mut self, object: Value, atom: Atom) {
        #[cfg(feature = "profile-aggregate")]
        self.snapshot_shadowed_method_caches(object, atom);
        let heap = &self.heap;
        for site in &mut self.method_caches {
            site.retain(|entry| !method_cache_shadowed_by(heap, *entry, object, atom));
        }
    }

    pub(super) fn is_function(&self, value: Value) -> bool {
        self.heap.is_function_value(value)
            || matches!(
                self.heap.get(value),
                Some(Cell::Proxy {
                    kind: ProxyKind::Callable | ProxyKind::Constructor,
                    ..
                })
            )
    }

    #[cfg(feature = "profile-aggregate")]
    pub(super) fn snapshot_method_caches(&mut self, reason: usize) {
        self.snapshot_method_cache_entries(self.all_method_cache_entries(), reason);
    }

    #[cfg(feature = "profile-aggregate")]
    fn snapshot_method_caches_for_atom(&mut self, atom: Atom, reason: usize) {
        let entries = self
            .all_method_cache_entries()
            .into_iter()
            .filter(|(_, entry)| entry.atom == atom)
            .collect();
        self.snapshot_method_cache_entries(entries, reason);
    }

    #[cfg(feature = "profile-aggregate")]
    fn snapshot_shadowed_method_caches(&mut self, object: Value, atom: Atom) {
        let heap = &self.heap;
        let entries = self
            .all_method_cache_entries()
            .into_iter()
            .filter(|(_, entry)| method_cache_shadowed_by(heap, *entry, object, atom))
            .collect();
        self.snapshot_method_cache_entries(entries, 1);
    }

    #[cfg(feature = "profile-aggregate")]
    fn all_method_cache_entries(&self) -> Vec<(usize, MethodCache)> {
        let mut entries = Vec::new();
        for (site, cache) in self.method_caches.iter().enumerate() {
            entries.extend(cache.entries().map(|entry| (site, *entry)));
        }
        entries
    }

    #[cfg(feature = "profile-aggregate")]
    fn snapshot_method_cache_entries(&mut self, entries: Vec<(usize, MethodCache)>, reason: usize) {
        let records: Vec<_> = entries
            .into_iter()
            .filter_map(|(site, entry)| {
                entry
                    .target
                    .map(|target| (site, entry.shape, entry.proto, target))
            })
            .collect();
        self.profile.method_cache_clear(reason, records.len());
        for (site, shape, proto, target) in records {
            self.invalidated_methods.insert(
                MethodCacheKey { site, shape, proto },
                InvalidatedMethod {
                    target,
                    reason: reason as u8,
                },
            );
        }
    }

    #[cfg(feature = "profile-aggregate")]
    pub(super) fn retain_live_gc_method_snapshots(&mut self) {
        let heap = &self.heap;
        let before = self.invalidated_methods.len();
        self.invalidated_methods.retain(|key, entry| {
            let proto_live = !key.proto.is_heap() || heap.is_live(key.proto);
            let env = match entry.target {
                CallTarget::User(_, _, env) => Some(env),
                CallTarget::Native(_) => None,
            };
            proto_live && env.is_none_or(|value| !value.is_heap() || heap.is_live(value))
        });
        self.profile.method_cache_dead_after_gc += (before - self.invalidated_methods.len()) as u64;
    }

    pub(super) fn retain_live_method_caches(&mut self) {
        #[cfg(feature = "profile-aggregate")]
        let before = self.method_cache_entry_count();
        let heap = &self.heap;
        for site in &mut self.method_caches {
            site.retain(|entry| method_cache_live(heap, *entry));
        }
        #[cfg(feature = "profile-aggregate")]
        {
            let after = self.method_cache_entry_count();
            self.profile.method_cache_clear(0, before - after);
        }
    }

    #[cfg(feature = "profile-aggregate")]
    fn method_cache_entry_count(&self) -> usize {
        self.method_caches
            .iter()
            .map(|site| {
                site.entries().filter(|entry| entry.target.is_some()).count()
            })
            .sum()
    }

    #[cfg(feature = "profile-aggregate")]
    fn profile_method_refill(&mut self, site: usize, shape: u32, proto: Value, target: CallTarget) {
        let invalidated = self
            .invalidated_methods
            .remove(&MethodCacheKey { site, shape, proto });
        self.profile.method_cache_refill(
            invalidated.map(|entry| usize::from(entry.reason)),
            invalidated.is_some_and(|entry| entry.target == target),
        );
    }

    pub(super) fn record_method_cache(&mut self, site: usize, cache: MethodCache) {
        self.method_cache_atoms.name(cache.atom);
        self.method_caches[site].record(cache);
    }
}

/// One method call site's entries: `METHOD_CACHE_WAYS` inline, most recent first, and a set
/// that holds up to `POLYMORPHISM` entries once a third receiver shape reaches the site. The
/// site is a slot of `Vm::method_caches`, so a lookup reaches its set without a search.
#[derive(Clone)]
pub(super) struct MethodSite {
    pub(super) inline: [MethodCache; METHOD_CACHE_WAYS],
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
    type Target = [MethodCache; METHOD_CACHE_WAYS];
    fn deref(&self) -> &Self::Target {
        &self.inline
    }
}

impl MethodSite {
    pub(super) const EMPTY: Self = Self { inline: [EMPTY_METHOD_CACHE; METHOD_CACHE_WAYS], set: None };

    /// Every entry the site holds: the inline ways, then the set's.
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
    fn record(&mut self, cache: MethodCache) {
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
    fn retain(&mut self, mut keep: impl FnMut(&MethodCache) -> bool) {
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
    fn record(&mut self, cache: MethodCache) {
        let len = usize::from(self.len);
        if let Some(entry) = self.entries[..len].iter_mut().find(|entry| entry.records_same(&cache)) {
            *entry = cache;
        } else if len < self.entries.len() {
            self.entries[len] = cache;
            self.len += 1;
        } else {
            self.entries.copy_within(1.., 0);
            self.entries[len - 1] = cache;
        }
    }
}

impl<H: Host> Vm<H> {
    fn method_cache_owner(&self, object: Value, atom: Atom, guard: MethodGuard) -> Option<Value> {
        let mut owner = object;
        for depth in 0..=guard.depth {
            let current = self.shape_property_lookup(owner, atom)?;
            if depth == guard.depth {
                return (owner == guard.owner && current.shape() == guard.owner_shape)
                    .then_some(owner);
            }
            if self
                .shape_slot(current.shape(), atom)
                .is_some_and(|slot| self.heap.property_get(current, slot).is_some())
            {
                return None;
            }
            owner = current.proto();
            if owner.is_null() {
                return None;
            }
        }
        None
    }

    fn method_cache_guard(&self, receiver: Value, atom: Atom) -> Option<MethodGuard> {
        let receiver_shape = self.object_data(receiver)?.shape();
        if self.shape_is_dictionary(receiver_shape) {
            return None;
        }
        let key = super::property_key::PropertyKey::string(atom);
        let mut owner = receiver;
        let mut depth = 0_u16;
        loop {
            self.heap.object_record(owner)?;
            let data = self.shape_property_lookup(owner, atom)?;
            if let Some(slot) = self.shape_slot(data.shape(), atom) {
                if slot > u16::MAX as usize
                    || self
                        .property_attributes(owner, key)
                        .is_some_and(|attributes| attributes.accessor)
                    || self.heap.property_get(data, slot).is_none()
                {
                    return None;
                }
                return Some(MethodGuard {
                    owner,
                    owner_shape: data.shape(),
                    slot: slot as u16,
                    depth,
                });
            }
            if self.property_attributes(owner, key).is_some() {
                return None;
            }
            owner = data.proto();
            if owner.is_null() {
                return None;
            }
            depth = depth.checked_add(1)?;
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

fn method_cache_shadowed_by(
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

fn method_cache_live(heap: &crate::heap::Heap, entry: MethodCache) -> bool {
    let key_live = !entry.proto.is_heap() || heap.is_live(entry.proto);
    let owner_live = !entry.guard.owner.is_heap() || heap.is_live(entry.guard.owner);
    let callee_live = !entry.callee.is_heap() || heap.is_live(entry.callee);
    let env = match entry.target {
        Some(CallTarget::User(_, _, env)) => Some(env),
        _ => None,
    };
    key_live
        && owner_live
        && callee_live
        && env.is_none_or(|value| !value.is_heap() || heap.is_live(value))
}

impl<H: Host> Vm<H> {
    pub(super) fn call_method_site_safe(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        site: usize,
        this: Value,
    ) -> Result<Value, JsError> {
        let metadata = p.method_sites[site];
        let cache_site = self.method_cache_index(site);
        let start = metadata.argument_start as usize;
        let registers = &p.method_arguments[start..start + metadata.argument_count as usize];
        let arguments = CallArguments::from_values(
            registers.iter().map(|register| self.read(frame, *register)),
        );
        self.profile.method_args(registers.len());

        let (shape, proto) = self
            .object_data(this)
            .map(|object| (object.shape(), object.proto()))
            .unwrap_or((NON_OBJECT_RECEIVER_SHAPE, Value::UNDEFINED));

        let specialized = self.specialized && p.specialized;
        let site_entries = &self.method_caches[cache_site];
        let mut candidate = if specialized {
            site_entries
                .iter()
                .enumerate()
                .find(|(_, entry)| entry.serves(shape, metadata.atom, proto, this))
                .map(|(tier, entry)| (*entry, tier))
        } else {
            None
        };
        if specialized && candidate.is_none() {
            candidate = site_entries
                .recorded_set()
                .iter()
                .find(|entry| entry.serves(shape, metadata.atom, proto, this))
                // The set is the tier after the inline ways.
                .map(|entry| (*entry, METHOD_CACHE_WAYS));
        }

        let cached_call = candidate.and_then(|(entry, tier)| {
            let owner = self.method_cache_owner(this, metadata.atom, entry.guard)?;
            let owner_data = self.object_data(owner)?;
            if owner_data.shape() != entry.guard.owner_shape {
                return None;
            }
            let callee = self
                .heap
                .property_get(owner_data, entry.guard.slot as usize)?;
            if entry.callee != callee {
                return None;
            }
            let target = entry.target?;
            Some((callee, target, tier))
        });
        self.profile.method_cache(cached_call.is_some());
        #[cfg(feature = "profile-aggregate")]
        self.profile
            .method_cache_tier(cached_call.map(|(_, _, tier)| tier));
        if let Some((callee, target, _)) = cached_call {
            return self.call_value_with_target_from_frame(
                p,
                callee,
                this,
                arguments.as_slice(),
                Some(target),
            );
        }

        let guard = self.method_cache_guard(this, metadata.atom);
        let callee = self.get_field_cached(p, this, metadata.atom, metadata.cache)?;
        if specialized
            && let Some(guard) = guard
            && self.heap.is_function_value(callee)
            && let Ok(target) = self.call_target(callee)
        {
            let cache_proto = if guard.depth == 0 { this } else { proto };
            #[cfg(feature = "profile-aggregate")]
            self.profile_method_refill(cache_site, shape, cache_proto, target);
            self.record_method_cache(
                cache_site,
                MethodCache {
                    shape,
                    atom: metadata.atom,
                    proto: cache_proto,
                    callee,
                    guard,
                    target: Some(target),
                },
            );
        }
        self.call_value_with_target_from_frame(p, callee, this, arguments.as_slice(), None)
    }
}

impl MethodCache {
    /// Whether this entry records the same lookup and callee as `other`.
    fn records_same(&self, other: &Self) -> bool {
        self.shape == other.shape && self.atom == other.atom && self.proto == other.proto && self.callee == other.callee
    }

    /// Whether this entry answers a lookup of `atom` on a receiver of `shape` whose prototype
    /// is `proto` (or that is itself the recorded prototype).
    #[inline(always)]
    fn serves(&self, shape: u32, atom: Atom, proto: Value, this: Value) -> bool {
        self.shape == shape && self.atom == atom && (self.proto == proto || self.proto == this)
    }
}
