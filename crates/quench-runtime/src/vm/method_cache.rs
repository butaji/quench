#![allow(dead_code)]
use super::*;
mod atoms;
mod megamorphic;
pub(super) use atoms::{AtomBits, MethodCacheAtoms};
use megamorphic::{retain_entries, retain_other_atom, retain_unshadowed_entries};

/// Entries of a call site's inline method cache, one way per shape residue (`method_way`); a site
/// that sees a second receiver in a way moves to a megamorphic set.
pub(super) const METHOD_CACHE_WAYS: usize = 2;
const _: () = assert!(METHOD_CACHE_WAYS.is_power_of_two());

/// The way of a site's inline cache that holds receivers of `shape`: the ways are direct-mapped
/// by shape id, so a lookup reads one entry.
const fn method_way(shape: u32) -> usize {
    shape as usize & (METHOD_CACHE_WAYS - 1)
}
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
        self.method_caches.fill([EMPTY_METHOD_CACHE; _]);
        self.clear_megamorphic_methods();
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
        for ways in &mut self.method_caches {
            clear_ways(ways, |entry| entry.atom == atom);
        }
        for set in &mut self.megamorphic_methods {
            set.len = retain_other_atom(&mut set.entries, usize::from(set.len), atom) as u8;
        }
        self.retire_empty_method_sets();
    }

    pub(super) fn invalidate_method_caches_for_prototype_add(&mut self, object: Value, atom: Atom) {
        #[cfg(feature = "profile-aggregate")]
        self.snapshot_shadowed_method_caches(object, atom);
        let heap = &self.heap;
        for ways in &mut self.method_caches {
            clear_ways(ways, |entry| method_cache_shadowed_by(heap, *entry, object, atom));
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
        self.retire_empty_method_sets();
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
        for ways in &mut self.method_caches {
            clear_ways(ways, |entry| !method_cache_live(heap, *entry));
        }
        for set in &mut self.megamorphic_methods {
            set.len = retain_entries(heap, &mut set.entries, usize::from(set.len)) as u8;
        }
        self.retire_empty_method_sets();
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
        self.method_cache_atoms.name(cache.atom);
        if let Some(position) = self.megamorphic_set_position(site) {
            let set = &mut self.megamorphic_methods[position];
            let len = usize::from(set.len);
            if let Some(entry) = set.entries[..len].iter_mut().find(|entry| entry.records_same(&cache)) {
                *entry = cache;
            } else if len < super::compiled::tier_policy::POLYMORPHISM {
                set.entries[len] = cache;
                set.len += 1;
            }
            return;
        }
        let way = &mut self.method_caches[site][method_way(cache.shape)];
        if way.target.is_none() || way.records_same(&cache) {
            *way = cache;
            return;
        }
        self.promote_method_site(site, cache);
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

/// Empties the entries of `ways` that `dropped` names, each in its own way, which a lookup
/// indexes by shape.
fn clear_ways(ways: &mut [MethodCache], dropped: impl Fn(&MethodCache) -> bool) {
    for entry in ways.iter_mut().filter(|entry| dropped(entry)) {
        *entry = EMPTY_METHOD_CACHE;
    }
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
        let way = method_way(shape);
        let inline = self.method_caches[cache_site][way];
        // The megamorphic set is the tier after the inline ways.
        let (entry, tier) = if serves(&inline) { (inline, way) } else { (self.megamorphic_method(cache_site, serves)?, METHOD_CACHE_WAYS) };
        let owner = self.method_cache_owner(this, atom, entry.guard)?;
        let owner_data = self.object_data(owner)?;
        if owner_data.shape() != entry.guard.owner_shape {
            return None;
        }
        let callee = self.heap.property_get(owner_data, entry.guard.slot as usize)?;
        (entry.callee == callee).then_some(())?;
        Some((callee, entry.target?, tier))
    }

    /// The callees the inline ways of method site `site` (of the active program) serve, by way.
    pub(super) fn method_cache_callees(&self, site: usize) -> Vec<Value> {
        self.method_caches[self.method_cache_index(site)].iter().filter(|entry| entry.target.is_some()).map(|entry| entry.callee).collect()
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
