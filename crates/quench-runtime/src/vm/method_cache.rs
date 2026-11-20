use super::*;
mod atoms;
mod table;
pub(super) use table::MethodCacheTable;
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
        self.method_caches.clear_all();
        self.megamorphic_methods.clear();
        self.method_cache_atoms.clear();
        self.method_cache_sites.clear();
    }

    /// Whether any method cache may name `atom`; `false` proves that none does.
    #[inline(always)]
    pub(super) fn method_caches_may_name(&self, atom: Atom) -> bool {
        self.method_cache_atoms.contains(atom)
    }

    pub(super) fn invalidate_method_caches_for_atom(&mut self, atom: Atom) {
        #[cfg(feature = "profile-aggregate")]
        self.snapshot_method_caches_for_atom(atom, 1);
        // Only the sites that recorded the atom can hold it: no scan of every site.
        if let Some(sites) = self.method_cache_sites.remove(&atom) {
            for site in sites {
                if let Some(entries) = self.method_caches.recorded_ways_mut(site as usize) {
                    let len = entries.len();
                    retain_other_atom(entries, len, atom);
                }
            }
        }
        for set in &mut self.megamorphic_methods {
            set.len = retain_other_atom(&mut set.entries, usize::from(set.len), atom) as u8;
        }
        self.retain_megamorphic_sets();
    }

    /// Drops the megamorphic sets that are empty or that no site reaches, and renumbers the kept
    /// ones: a set is reached through its site's index (`MethodCacheTable::megamorphic_index`),
    /// so the index of every kept set is rewritten here, since a drop shifts the sets after it.
    /// Sound because each site has at most one set and each set has one index, so the rewrite
    /// never moves another site's set.
    fn retain_megamorphic_sets(&mut self) {
        let sets = std::mem::take(&mut self.megamorphic_methods);
        for (index, set) in sets.into_iter().enumerate() {
            if self.method_caches.megamorphic_index(set.site) != Some(index) {
                continue;
            }
            if set.len == 0 {
                self.method_caches.set_megamorphic_index(set.site, None);
                continue;
            }
            self.method_caches
                .set_megamorphic_index(set.site, Some(self.megamorphic_methods.len()));
            self.megamorphic_methods.push(set);
        }
    }

    pub(super) fn invalidate_method_caches_for_prototype_add(&mut self, object: Value, atom: Atom) {
        #[cfg(feature = "profile-aggregate")]
        self.snapshot_shadowed_method_caches(object, atom);
        let heap = &self.heap;
        for entries in self.method_caches.recorded_mut() {
            let len = entries.len();
            retain_unshadowed_entries(heap, entries, len, object, atom);
        }
        for set in &mut self.megamorphic_methods {
            set.len = retain_unshadowed_entries(
                heap,
                &mut set.entries,
                usize::from(set.len),
                object,
                atom,
            ) as u8;
        }
        self.retain_megamorphic_sets();
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
        for (site, cache) in self.method_caches.sites() {
            entries.extend(cache.iter().map(|entry| (site, *entry)));
        }
        entries.extend(self.megamorphic_methods.iter().flat_map(|set| {
            set.entries[..usize::from(set.len)]
                .iter()
                .map(|entry| (set.site, *entry))
        }));
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
        // Rebuilt with the entries, so dropped entries leave no stale sites behind.
        self.method_cache_sites.clear();
        for (site, entries) in self.method_caches.recorded_sites_mut() {
            let len = entries.len();
            retain_entries(heap, entries, len);
            for (index, entry) in entries.iter().enumerate() {
                if entry.atom != EMPTY_METHOD_CACHE.atom && !entries[..index].iter().any(|other| other.atom == entry.atom) {
                    self.method_cache_sites.entry(entry.atom).or_default().push(site as u32);
                }
            }
        }
        for set in &mut self.megamorphic_methods {
            set.len = retain_entries(heap, &mut set.entries, usize::from(set.len)) as u8;
        }
        self.retain_megamorphic_sets();
        #[cfg(feature = "profile-aggregate")]
        {
            let after = self.method_cache_entry_count();
            self.profile.method_cache_clear(0, before - after);
        }
    }

    #[cfg(feature = "profile-aggregate")]
    fn method_cache_entry_count(&self) -> usize {
        self.method_caches
            .recorded()
            .flat_map(|entries| entries.iter())
            .filter(|entry| entry.target.is_some())
            .count()
            + self
                .megamorphic_methods
                .iter()
                .map(|set| usize::from(set.len))
                .sum::<usize>()
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
        if let Some(index) = self.method_caches.megamorphic_index(site) {
            let set = &mut self.megamorphic_methods[index];
            if let Some(entry) = set.entries[..usize::from(set.len)]
                .iter_mut()
                .find(|entry| entry.records_same(&cache))
            {
                *entry = cache;
            } else if usize::from(set.len) < super::compiled::tier_policy::POLYMORPHISM {
                set.entries[usize::from(set.len)] = cache;
                set.len += 1;
            }
            return;
        }
        let entries = self.method_caches.ways_mut(site);
        if !entries.iter().any(|entry| entry.atom == cache.atom) {
            self.method_cache_sites.entry(cache.atom).or_default().push(site as u32);
        }
        if entries.iter().any(|entry| entry.records_same(&cache)) || entries[1].target.is_none()
        {
            entries[1] = entries[0];
            entries[0] = cache;
            return;
        }
        let mut set = MethodCacheSet {
            site,
            len: (METHOD_CACHE_WAYS + 1) as u8,
            entries: [EMPTY_METHOD_CACHE; super::compiled::tier_policy::POLYMORPHISM],
        };
        set.entries[..METHOD_CACHE_WAYS].copy_from_slice(entries);
        set.entries[METHOD_CACHE_WAYS] = cache;
        self.method_caches
            .set_megamorphic_index(site, Some(self.megamorphic_methods.len()));
        self.megamorphic_methods.push(set);
    }
}

impl<H: Host> Vm<H> {
    /// The holder a method guard recorded, for a lookup of `atom` on `receiver` whose entry
    /// already matched the receiver's shape and prototype. The receiver's own kind is tested
    /// by its shape (`shape_property_lookup`), a depth-0 guard names the receiver itself, and
    /// the holder is the recorded object, which still answers `atom` only while its shape is
    /// the recorded `owner_shape`. The intermediate prototypes are not walked: each one's
    /// shape and prototype are kept by invalidation, not by a compare on every hit. A property
    /// added to an intermediate prototype drops the entry (`invalidate_method_caches_for_prototype_add`),
    /// a removal drops it through its key (`invalidate_method_caches_for_key`), and a change of
    /// an intermediate's prototype flushes every cache (`set_prototype_of`). The holder's own
    /// layout is still compared here, so a changed holder misses as the walk did.
    #[inline(always)]
    fn method_cache_holder(&self, receiver: Value, atom: Atom, guard: MethodGuard) -> Option<&Object> {
        self.shape_property_lookup(receiver, atom)?;
        if guard.depth == 0 && receiver != guard.owner {
            return None;
        }
        let holder = self.object_data(guard.owner)?;
        (holder.shape() == guard.owner_shape).then_some(holder)
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

fn retain_entries(heap: &crate::heap::Heap, entries: &mut [MethodCache], len: usize) -> usize {
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

        let key = self.method_receiver_key(this);
        let (shape, proto) = key;
        let specialized = self.specialized && p.specialized;
        let cached_call = self.cached_method(cache_site, metadata.atom, key, this, specialized);
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

        // The guard only feeds a specialized refill, so an unspecialized site walks the chain once,
        // for the callee alone. It is taken before the callee, which may run a getter that changes it.
        let guard = if specialized { self.method_cache_guard(this, metadata.atom) } else { None };
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

impl<H: Host> Vm<H> {
    /// The shape and prototype method caches key a lookup on `this` by: an object's own, else
    /// the key no entry serves (`NON_OBJECT_RECEIVER_SHAPE`).
    fn method_receiver_key(&self, this: Value) -> (u32, Value) {
        self.object_data(this)
            .map(|object| (object.shape(), object.proto()))
            .unwrap_or((NON_OBJECT_RECEIVER_SHAPE, Value::UNDEFINED))
    }

    /// The method of `this` the caches of `cache_site` serve for `atom` under `key` (its shape and
    /// prototype), with its call target and the cache tier that served it (the inline ways, then
    /// the megamorphic set): the entry whose owner still holds the same callee. `specialized` is
    /// whether the program and the VM use the caches at all.
    fn cached_method(&self, cache_site: usize, atom: Atom, (shape, proto): (u32, Value), this: Value, specialized: bool)
        -> Option<(Value, CallTarget, usize)> {
        specialized.then_some(())?;
        let serves = |entry: &MethodCache| entry.serves(shape, atom, proto, this);
        let inline = self.method_caches[cache_site].iter().enumerate()
            .find(|(_, entry)| serves(entry)).map(|(tier, entry)| (*entry, tier));
        // The megamorphic set is the tier after the inline ways, read at the index its site
        // recorded when the set was made, so no scan over the sets.
        let megamorphic = || self.method_caches.megamorphic_index(cache_site)
            .and_then(|index| self.megamorphic_methods.get(index))
            .and_then(|set| set.entries[..usize::from(set.len)].iter().find(|entry| serves(entry)).copied())
            .map(|entry| (entry, METHOD_CACHE_WAYS));
        let (entry, tier) = inline.or_else(megamorphic)?;
        let holder = self.method_cache_holder(this, atom, entry.guard)?;
        let callee = self.heap.property_get(holder, entry.guard.slot as usize)?;
        (entry.callee == callee).then_some(())?;
        Some((callee, entry.target?, tier))
    }

    /// The method `atom` of `this` that method site `site` caches, loaded as `call_method_site_safe`
    /// loads it on a hit and without running guest code: `None` where the caches do not serve it.
    pub(super) fn method_cache_load(&self, p: &ResidualProgram, site: usize, atom: Atom, this: Value) -> Option<Value> {
        let key = self.method_receiver_key(this);
        self.cached_method(self.method_cache_index(site), atom, key, this, self.specialized && p.specialized)
            .map(|(callee, _, _)| callee)
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
