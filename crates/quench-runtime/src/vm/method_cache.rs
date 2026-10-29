#![allow(dead_code)]
use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn invalidate_method_caches(&mut self) {
        self.clear_method_caches(1);
    }

    pub(super) fn clear_method_caches(&mut self, _reason: usize) {
        #[cfg(feature = "profile-aggregate")]
        self.snapshot_method_caches(_reason);
        self.method_caches.fill([EMPTY_METHOD_CACHE; 2]);
        self.megamorphic_methods.clear();
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
        for entries in &mut self.method_caches {
            let len = entries.len();
            retain_other_atom(entries, len, atom);
        }
        for set in &mut self.megamorphic_methods {
            set.len = retain_other_atom(&mut set.entries, usize::from(set.len), atom) as u8;
        }
        self.megamorphic_methods.retain(|set| set.len != 0);
    }

    pub(super) fn invalidate_method_caches_for_prototype_add(&mut self, object: Value, atom: Atom) {
        #[cfg(feature = "profile-aggregate")]
        self.snapshot_shadowed_method_caches(object, atom);
        let heap = &self.heap;
        for entries in &mut self.method_caches {
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
        self.megamorphic_methods.retain(|set| set.len != 0);
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
        for entries in &mut self.method_caches {
            let len = entries.len();
            retain_entries(heap, entries, len);
        }
        for set in &mut self.megamorphic_methods {
            set.len = retain_entries(heap, &mut set.entries, usize::from(set.len)) as u8;
        }
        self.megamorphic_methods.retain(|set| set.len != 0);
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
        self.method_cache_atoms.insert(cache.atom);
        if let Some(set) = self
            .megamorphic_methods
            .iter_mut()
            .find(|set| set.site == site)
        {
            if let Some(entry) = set.entries[..usize::from(set.len)]
                .iter_mut()
                .find(|entry| {
                    entry.shape == cache.shape
                        && entry.atom == cache.atom
                        && entry.proto == cache.proto
                        && entry.callee == cache.callee
                })
            {
                *entry = cache;
            } else if usize::from(set.len) < METHOD_MEGAMORPHIC_LIMIT {
                set.entries[usize::from(set.len)] = cache;
                set.len += 1;
            }
            return;
        }
        let entries = &mut self.method_caches[site];
        if entries.iter().any(|entry| {
            entry.shape == cache.shape
                && entry.atom == cache.atom
                && entry.proto == cache.proto
                && entry.callee == cache.callee
        }) || entries[1].target.is_none()
        {
            entries[1] = entries[0];
            entries[0] = cache;
            return;
        }
        let mut set = MethodCacheSet {
            site,
            len: 3,
            entries: [EMPTY_METHOD_CACHE; METHOD_MEGAMORPHIC_LIMIT],
        };
        set.entries[..2].copy_from_slice(entries);
        set.entries[2] = cache;
        self.megamorphic_methods.push(set);
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
            if self.heap.object_record(owner).is_none() {
                return None;
            }
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

        let (shape, proto) = self
            .object_data(this)
            .map(|object| (object.shape(), object.proto()))
            .unwrap_or((u32::MAX - 1, Value::UNDEFINED));

        let specialized = self.specialized && p.specialized;
        let mut candidate = if specialized {
            self.method_caches[cache_site]
                .iter()
                .enumerate()
                .find(|(_, entry)| {
                    entry.shape == shape
                        && entry.atom == metadata.atom
                        && (entry.proto == proto || entry.proto == this)
                })
                .map(|(tier, entry)| (*entry, tier))
        } else {
            None
        };
        if specialized && candidate.is_none() {
            candidate = self
                .megamorphic_methods
                .iter()
                .find(|set| set.site == cache_site)
                .and_then(|set| {
                    set.entries[..usize::from(set.len)]
                        .iter()
                        .find(|entry| {
                            entry.shape == shape
                                && entry.atom == metadata.atom
                                && (entry.proto == proto || entry.proto == this)
                        })
                        .copied()
                })
                .map(|entry| (entry, 2));
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

/// The atoms method caches may name: a bit per atom, set when an entry for the atom is
/// recorded and cleared only when every method cache is, so a clear bit proves that no method
/// cache names the atom. Exact per atom, so a program with many cached method names does not
/// make every property add on every object scan the caches.
#[derive(Clone, Debug, Default)]
pub(super) struct MethodCacheAtoms {
    words: Vec<u64>,
}

impl MethodCacheAtoms {
    #[inline(always)]
    fn position(atom: Atom) -> (usize, u64) {
        ((atom / u64::BITS) as usize, 1 << (atom % u64::BITS))
    }

    #[inline(always)]
    pub(super) fn contains(&self, atom: Atom) -> bool {
        let (word, bit) = Self::position(atom);
        self.words.get(word).is_some_and(|bits| bits & bit != 0)
    }

    pub(super) fn insert(&mut self, atom: Atom) {
        let (word, bit) = Self::position(atom);
        if word >= self.words.len() {
            self.words.resize(word + 1, 0);
        }
        self.words[word] |= bit;
    }

    pub(super) fn clear(&mut self) {
        self.words.fill(0);
    }

    /// The bit words, for generated code that tests a name (`emit_js_own_field_set`).
    pub(super) fn words(&self) -> &[u64] {
        &self.words
    }
}
