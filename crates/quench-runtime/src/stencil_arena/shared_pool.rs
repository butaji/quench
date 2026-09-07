use super::*;

impl SharedStencilSlab {
    pub fn new(slab_capacity: usize) -> Result<Self, ArenaError> {
        Self::new_in_budget(slab_capacity, &GLOBAL_EXECUTABLE_BUDGET)
    }

    pub(super) fn new_in_budget(
        slab_capacity: usize,
        budget: &'static AtomicBudget,
    ) -> Result<Self, ArenaError> {
        if slab_capacity == 0 || slab_capacity > MAX_ARENA_BYTES {
            return Err(ArenaError::InvalidCapacity);
        }
        let slab_capacity = slab_capacity
            .checked_add(PAGE - 1)
            .ok_or(ArenaError::InvalidCapacity)?
            & !(PAGE - 1);
        Ok(Self {
            slabs: Vec::new(),
            cache: RenderedRegionCache::new(),
            slab_capacity,
            active_dispatches: Cell::new(0),
            peak_dispatches: Cell::new(0),
            lease_state: std::rc::Rc::new(LeaseState {
                active: Cell::new(0),
                peak: Cell::new(0),
                owners: RefCell::new(HashMap::new()),
                retired: RefCell::new(HashSet::new()),
            }),
            budget,
        })
    }

    pub(super) fn total_capacity(&self) -> usize {
        self.slabs.iter().map(StencilArena::capacity).sum()
    }

    pub fn slab_count(&self) -> usize {
        self.slabs.len()
    }

    pub fn used(&self) -> usize {
        self.slabs.iter().map(StencilArena::used).sum()
    }

    pub fn capacity(&self) -> usize {
        self.slabs.iter().map(StencilArena::capacity).sum()
    }

    /// Number of currently executing entries owned by this pool.  Execution
    /// is synchronous today, but keeping this explicit makes the lifetime
    /// contract auditable before any eviction or concurrent publication is
    /// added: active slabs must never be reclaimed.
    pub fn active_dispatches(&self) -> usize {
        self.active_dispatches.get()
    }

    pub fn peak_dispatches(&self) -> usize {
        self.peak_dispatches.get()
    }

    pub fn active_leases(&self) -> usize {
        self.lease_state.active.get()
    }

    pub fn peak_leases(&self) -> usize {
        self.lease_state.peak.get()
    }

    /// Retire the complete allocation generation containing `address`.
    /// Existing leases keep its mapping charged and callable, while cache
    /// lookup and every new lease fail closed. The idle generation is
    /// reclaimed immediately; an active generation is reclaimed by its final
    /// lease release.
    pub(crate) fn retire_allocation(
        &mut self,
        address: usize,
        cache: &mut RenderedRegionCache,
    ) -> Result<(), ArenaError> {
        let owner = self
            .owner_for(address)
            .ok_or(ArenaError::ProtectionFailed)?;
        self.lease_state.retire(owner);
        self.cache.remove_owner(owner);
        cache.remove_owner(owner);
        self.reclaim_retired_owner(owner);
        Ok(())
    }

    pub(super) fn reclaim_retired_owner(&mut self, owner: u64) -> bool {
        if !self.lease_state.is_retired(owner) || self.lease_state.owns_active_lease(owner) {
            return false;
        }
        let Some(index) = self.slabs.iter().position(|slab| slab.id() == owner) else {
            self.lease_state.forget(owner);
            return false;
        };
        self.slabs.remove(index);
        self.cache.remove_owner(owner);
        self.lease_state.forget(owner);
        true
    }

    /// Drop slabs that have no allocation-retaining lease. Each cache entry
    /// carries its slab generation, so an evicted address cannot become
    /// callable again if the OS later reuses that address.
    pub fn evict_idle(&mut self, retain: usize) -> usize {
        self.evict_idle_core(retain, None)
    }

    /// Evict idle slabs and prune their derived cache rows in one ownership
    /// transition.  Callers that only need a count may use `evict_idle`; the
    /// cache-aware form prevents stale generation rows accumulating after a
    /// retirement/rebuild cycle.
    pub fn evict_idle_with_cache(
        &mut self,
        cache: &mut RenderedRegionCache,
        retain: usize,
    ) -> usize {
        self.evict_idle_core(retain, Some(cache))
    }

    fn evict_idle_core(
        &mut self,
        retain: usize,
        mut external: Option<&mut RenderedRegionCache>,
    ) -> usize {
        if self.active_dispatches.get() != 0 {
            return 0;
        }
        let owners = self.remove_idle_owners(retain);
        for owner in &owners {
            self.cache.remove_owner(*owner);
            if let Some(cache) = external.as_deref_mut() {
                cache.remove_owner(*owner);
            }
        }
        owners.len()
    }

    fn remove_idle_owners(&mut self, retain: usize) -> Vec<u64> {
        let remove = self.slabs.len().saturating_sub(retain);
        let mut owners = Vec::with_capacity(remove);
        let state = &self.lease_state;
        self.slabs.retain(|slab| {
            let evict = owners.len() < remove && !state.owns_active_lease(slab.id());
            if evict {
                owners.push(slab.id());
            }
            !evict
        });
        owners
    }

    fn reclaim_for(&mut self, additional: usize, cache: &mut RenderedRegionCache) -> bool {
        if self.total_capacity().saturating_add(additional) <= MAX_SHARED_SLAB_BYTES {
            return true;
        }
        if self.active_dispatches.get() != 0 {
            return false;
        }
        while self.total_capacity().saturating_add(additional) > MAX_SHARED_SLAB_BYTES
            && self.slabs.len() > 1
        {
            let Some(index) = self
                .slabs
                .iter()
                .position(|slab| !self.lease_state.owns_active_lease(slab.id()))
            else {
                return false;
            };
            let owner = self.slabs[index].id();
            self.slabs.remove(index);
            self.cache.remove_owner(owner);
            cache.remove_owner(owner);
        }
        self.total_capacity().saturating_add(additional) <= MAX_SHARED_SLAB_BYTES
    }

    pub fn render_or_get<const N: usize>(
        &mut self,
        cache: &mut RenderedRegionCache,
        key: crate::stencil_fact::RegionKey,
        stencil: &Stencil,
        values: &PatchValues<'_, N>,
    ) -> Result<usize, ArenaError> {
        let signature = crate::stencil_select::select_physical(key)
            .map(|view| view.cache_signature(values))
            .unwrap_or_else(|| cache_signature(stencil, values));
        for slab in &mut self.slabs {
            if self.lease_state.is_retired(slab.id()) {
                continue;
            }
            match slab.render_or_get(&mut self.cache, key, stencil, values) {
                Ok(address) => {
                    let owner = slab.id();
                    cache.insert_owned(key, signature, address, owner);
                    return Ok(address);
                }
                Err(ArenaError::ProtectionFailed | ArenaError::Exhausted) => continue,
                Err(error) => return Err(error),
            }
        }
        if !self.reclaim_for(self.slab_capacity, cache) {
            return Err(ArenaError::Exhausted);
        }
        let mut slab = StencilArena::new_in_budget(self.slab_capacity, self.budget)?;
        let address = match slab.render_or_get(&mut self.cache, key, stencil, values) {
            Ok(address) => address,
            Err(error) => return Err(error),
        };
        cache.insert_owned(key, signature, address, slab.id());
        self.slabs.push(slab);
        Ok(address)
    }

    pub fn render_physical_view_or_get<const N: usize>(
        &mut self,
        cache: &mut RenderedRegionCache,
        view: crate::stencil_select::PhysicalStencilView,
        values: &PatchValues<'_, N>,
    ) -> Result<usize, ArenaError> {
        self.render_physical_view_with_control(cache, view, values, None)
    }

    pub(crate) fn render_controlled_physical_view_or_get<const N: usize>(
        &mut self,
        cache: &mut RenderedRegionCache,
        view: crate::stencil_select::PhysicalStencilView,
        values: &PatchValues<'_, N>,
        control: &crate::stencil_cfg::RegionControlPlan,
    ) -> Result<usize, ArenaError> {
        self.render_physical_view_with_control(cache, view, values, Some(control))
    }

    fn render_physical_view_with_control<const N: usize>(
        &mut self,
        cache: &mut RenderedRegionCache,
        view: crate::stencil_select::PhysicalStencilView,
        values: &PatchValues<'_, N>,
        control: Option<&crate::stencil_cfg::RegionControlPlan>,
    ) -> Result<usize, ArenaError> {
        let selected = crate::stencil_select::select_physical_for_abi(view.key, view.abi)
            .ok_or(ArenaError::ProtectionFailed)?;
        if !view.contract().abi_is_well_formed() || !view.matches(&selected) {
            return Err(ArenaError::ProtectionFailed);
        }
        let signature = view.cache_signature(values);
        if let Some(address) = self.render_existing_physical(cache, view, values, control)? {
            return Ok(address);
        }
        self.allocate_physical(cache, view, values, control, signature)
    }

    fn render_existing_physical<const N: usize>(
        &mut self,
        cache: &mut RenderedRegionCache,
        view: crate::stencil_select::PhysicalStencilView,
        values: &PatchValues<'_, N>,
        control: Option<&crate::stencil_cfg::RegionControlPlan>,
    ) -> Result<Option<usize>, ArenaError> {
        let signature = view.cache_signature(values);
        for slab in &mut self.slabs {
            if self.lease_state.is_retired(slab.id()) {
                continue;
            }
            match render_arena_physical(slab, &mut self.cache, view, values, control) {
                Ok(address) => {
                    let owner = slab.id();
                    cache.insert_owned(view.key, signature, address, owner);
                    return Ok(Some(address));
                }
                Err(ArenaError::ProtectionFailed | ArenaError::Exhausted) => continue,
                Err(error) => return Err(error),
            }
        }
        Ok(None)
    }

    fn allocate_physical<const N: usize>(
        &mut self,
        cache: &mut RenderedRegionCache,
        view: crate::stencil_select::PhysicalStencilView,
        values: &PatchValues<'_, N>,
        control: Option<&crate::stencil_cfg::RegionControlPlan>,
        signature: u64,
    ) -> Result<usize, ArenaError> {
        if !self.reclaim_for(self.slab_capacity, cache) {
            return Err(ArenaError::Exhausted);
        }
        let mut slab = StencilArena::new_in_budget(self.slab_capacity, self.budget)?;
        let address = match render_arena_physical(&mut slab, &mut self.cache, view, values, control)
        {
            Ok(address) => address,
            Err(error) => return Err(error),
        };
        cache.insert_owned(view.key, signature, address, slab.id());
        self.slabs.push(slab);
        Ok(address)
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn publish_region_image_or_get(
        &mut self,
        cache: &mut RenderedRegionCache,
        image: &VerifiedRegionImage,
    ) -> Result<usize, ArenaError> {
        let identity = image.identity();
        for slab in &mut self.slabs {
            if self.lease_state.is_retired(slab.id()) {
                continue;
            }
            match slab.publish_region_image_or_get(&mut self.cache, image) {
                Ok(address) => {
                    cache.insert_owned(identity.key, identity.cache_signature, address, slab.id());
                    return Ok(address);
                }
                Err(ArenaError::ProtectionFailed | ArenaError::Exhausted) => continue,
                Err(error) => return Err(error),
            }
        }
        self.allocate_region_image(cache, image)
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    fn allocate_region_image(
        &mut self,
        cache: &mut RenderedRegionCache,
        image: &VerifiedRegionImage,
    ) -> Result<usize, ArenaError> {
        if !self.reclaim_for(self.slab_capacity, cache) {
            return Err(ArenaError::Exhausted);
        }
        let identity = image.identity();
        let mut slab = StencilArena::new_in_budget(self.slab_capacity, self.budget)?;
        let address = slab.publish_region_image_or_get(&mut self.cache, image)?;
        cache.insert_owned(identity.key, identity.cache_signature, address, slab.id());
        self.slabs.push(slab);
        Ok(address)
    }
}
