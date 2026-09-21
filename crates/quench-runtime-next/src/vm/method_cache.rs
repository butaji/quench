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
    }

    pub(super) fn is_function(&self, value: Value) -> bool {
        matches!(self.heap.get(value), Some(Cell::Function { .. }))
    }

    #[cfg(feature = "profile-aggregate")]
    pub(super) fn snapshot_method_caches(&mut self, reason: usize) {
        let mut records = Vec::new();
        for (site, entries) in self.method_caches.iter().enumerate() {
            records.extend(entries.iter().filter_map(|entry| {
                entry
                    .target
                    .map(|target| (site, entry.shape, entry.proto, target))
            }));
        }
        for set in &self.megamorphic_methods {
            records.extend(
                set.entries[..usize::from(set.len)]
                    .iter()
                    .filter_map(|entry| {
                        entry
                            .target
                            .map(|target| (usize::from(set.site), entry.shape, entry.proto, target))
                    }),
            );
        }
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
            let proto_live = !key.proto.is_heap() || heap.get(key.proto).is_some();
            let env = match entry.target {
                CallTarget::User(_, env) | CallTarget::NumericUser(_, env) => Some(env),
                CallTarget::Native(_) => None,
            };
            proto_live && env.is_none_or(|value| !value.is_heap() || heap.get(value).is_some())
        });
        self.profile.method_cache_dead_after_gc += (before - self.invalidated_methods.len()) as u64;
    }

    pub(super) fn retain_live_method_caches(&mut self) {
        let heap = &self.heap;
        for entries in &mut self.method_caches {
            let len = entries.len();
            retain_entries(heap, entries, len);
        }
        for set in &mut self.megamorphic_methods {
            set.len = retain_entries(heap, &mut set.entries, usize::from(set.len)) as u8;
        }
        self.megamorphic_methods.retain(|set| set.len != 0);
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
        if let Some(set) = self
            .megamorphic_methods
            .iter_mut()
            .find(|set| usize::from(set.site) == site)
        {
            if let Some(entry) = set.entries[..usize::from(set.len)]
                .iter_mut()
                .find(|entry| entry.shape == cache.shape && entry.proto == cache.proto)
            {
                *entry = cache;
            } else if usize::from(set.len) < METHOD_MEGAMORPHIC_LIMIT {
                set.entries[usize::from(set.len)] = cache;
                set.len += 1;
            }
            return;
        }
        let entries = &mut self.method_caches[site];
        if entries
            .iter()
            .any(|entry| entry.shape == cache.shape && entry.proto == cache.proto)
            || entries[1].target.is_none()
        {
            entries[1] = entries[0];
            entries[0] = cache;
            return;
        }
        let mut set = MethodCacheSet {
            site: site as u16,
            len: 3,
            entries: [EMPTY_METHOD_CACHE; METHOD_MEGAMORPHIC_LIMIT],
        };
        set.entries[..2].copy_from_slice(entries);
        set.entries[2] = cache;
        self.megamorphic_methods.push(set);
    }
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
    let key_live = !entry.proto.is_heap() || heap.get(entry.proto).is_some();
    let env = match entry.target {
        Some(CallTarget::User(_, env) | CallTarget::NumericUser(_, env)) => Some(env),
        _ => None,
    };
    key_live && env.is_none_or(|value| !value.is_heap() || heap.get(value).is_some())
}

impl<H: Host> Vm<H> {
    #[inline(always)]
    pub(super) fn call_method_site(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        site: usize,
        this: Value,
    ) -> Result<Value, JsError> {
        let metadata = p.method_sites[site];
        let start = metadata.argument_start as usize;
        let args = &p.method_arguments[start..start + metadata.argument_count as usize];
        self.profile.method_args(args.len());
        let (shape, proto) = self
            .object_data(this)
            .map(|object| (object.shape(), object.proto))
            .unwrap_or((u32::MAX - 1, Value::UNDEFINED));
        let mut cached = self.method_caches[site]
            .iter()
            .find(|entry| entry.shape == shape && (entry.proto == proto || entry.proto == this))
            .and_then(|entry| entry.target);
        #[cfg(feature = "profile-aggregate")]
        let mut cache_tier = self.method_caches[site].iter().position(|entry| {
            entry.shape == shape && (entry.proto == proto || entry.proto == this)
        });
        if cached.is_none() {
            cached = self
                .megamorphic_methods
                .iter()
                .find(|set| usize::from(set.site) == site)
                .and_then(|set| {
                    set.entries[..usize::from(set.len)].iter().find(|entry| {
                        entry.shape == shape && (entry.proto == proto || entry.proto == this)
                    })
                })
                .and_then(|entry| entry.target);
            #[cfg(feature = "profile-aggregate")]
            if cached.is_some() {
                cache_tier = Some(2);
            }
        }
        self.profile.method_cache(cached.is_some());
        #[cfg(feature = "profile-aggregate")]
        self.profile.method_cache_tier(cache_tier);
        let target = if let Some(target) = cached {
            target
        } else {
            let own_callee = self.own_property(this, metadata.atom);
            let callee = match own_callee {
                Some(value) => value,
                None => self.get_field_cached(p, this, metadata.atom, metadata.cache)?,
            };
            let cache_proto = if own_callee.is_some() { this } else { proto };
            let target = match self.heap.get(callee) {
                Some(Cell::Function {
                    kind: FunctionKind::User(id),
                    env,
                    ..
                }) => CallTarget::User(*id, *env),
                Some(Cell::Function {
                    kind: FunctionKind::NumericUser(id),
                    env,
                    ..
                }) => CallTarget::NumericUser(*id, *env),
                Some(Cell::Function {
                    kind: FunctionKind::Native(native),
                    ..
                }) => CallTarget::Native(*native),
                _ => {
                    if std::env::var_os("RQJ_CALL_DIAGNOSTICS").is_some() {
                        eprintln!(
                            "rqj: non-callable method={} receiver={this:?} shape={shape} value={callee:?}",
                            &p.atoms[metadata.atom as usize]
                        );
                    }
                    return Err(JsError("value is not callable".into()));
                }
            };
            #[cfg(feature = "profile-aggregate")]
            self.profile_method_refill(site, shape, cache_proto, target);
            self.record_method_cache(
                site,
                MethodCache {
                    shape,
                    proto: cache_proto,
                    target: Some(target),
                },
            );
            target
        };
        let target_kind = match target {
            CallTarget::Native(_) => 0,
            CallTarget::User(..) => 1,
            CallTarget::NumericUser(..) => 2,
        };
        self.profile.call_target(target_kind, args.len());
        if let CallTarget::NumericUser(id, env) = target {
            return self.call_user_numeric(
                p,
                id,
                env,
                this,
                NumericArguments::Registers {
                    frame,
                    values: args,
                },
            );
        }
        let mut inline = [std::mem::MaybeUninit::<Value>::uninit(); 8];
        debug_assert!(args.len() <= inline.len());
        for (index, register) in args.iter().enumerate() {
            inline[index].write(self.read(frame, *register));
        }
        // SAFETY: method-site argument registers are compiler-issued, capped
        // at eight, and initialize exactly this prefix.
        let arguments =
            unsafe { std::slice::from_raw_parts(inline.as_ptr().cast::<Value>(), args.len()) };
        match target {
            CallTarget::User(id, env) => self.call_user(p, id, env, this, arguments),
            CallTarget::NumericUser(..) => unreachable!(),
            CallTarget::Native(native) => self.call_native(p, native, this, arguments),
        }
    }
}
