//! Bounded effectful code arena for rendered stencils.
//!
//! This is the only stencil module that owns OS memory mapping and `unsafe`.
//! It exposes fallible allocation/copy/patch operations and never executes a
//! partially rendered region.

use crate::bounded_resource::{AtomicBudget, BudgetReservation};
use crate::stencil_fact::{PatchValues, Stencil};
#[cfg(test)]
use crate::stencil_layout::FixupKind;
use crate::stencil_patch::{apply_holes, PatchError};
use crate::stencil_region_layout::{
    compose_selected_controlled_region, compose_selected_region, RegionImageIdentity,
    VerifiedRegionImage,
};
use crate::stencil_select::RenderedRegionCache;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};

mod resource;
#[cfg(feature = "execution-trace")]
pub(crate) use resource::ExecutableResourceSnapshot;

#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
extern "C" {
    fn sys_icache_invalidate(start: *const std::ffi::c_void, size: usize);
}

#[cfg(all(target_arch = "aarch64", not(target_os = "macos")))]
extern "C" {
    fn __clear_cache(start: *const u8, end: *const u8);
}

#[inline]
fn flush_icache(ptr: *const u8, len: usize) {
    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    unsafe {
        sys_icache_invalidate(ptr.cast(), len);
    }
    #[cfg(all(target_arch = "aarch64", not(target_os = "macos")))]
    unsafe {
        __clear_cache(ptr, ptr.add(len));
    }
    #[cfg(not(target_arch = "aarch64"))]
    let _ = (ptr, len);
}

const PAGE: usize = 4096;
static NEXT_ARENA_ID: AtomicU64 = AtomicU64::new(1);

#[cfg(target_arch = "aarch64")]
const STENCIL_ALIGNMENT: usize = 4;
#[cfg(not(target_arch = "aarch64"))]
const STENCIL_ALIGNMENT: usize = 1;

/// Global bound for the disposable physical region pool.  A plan may rotate
/// from an RX slab to a fresh RW slab, but never allocate an unbounded number
/// of executable mappings.
pub const MAX_SHARED_SLAB_BYTES: usize = 4 * MAX_ARENA_BYTES;
/// Workload-independent disposable code budget for one arena.
pub const MAX_ARENA_BYTES: usize = 1 << 20;
const MAX_GLOBAL_SHARED_SLAB_BYTES: usize = 16 * MAX_SHARED_SLAB_BYTES;
static GLOBAL_EXECUTABLE_BUDGET: AtomicBudget = AtomicBudget::new(MAX_GLOBAL_SHARED_SLAB_BYTES);

#[cfg(test)]
pub(crate) fn global_shared_slab_bytes() -> usize {
    GLOBAL_EXECUTABLE_BUDGET.used()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalExecutionWitness {
    pub key: crate::stencil_fact::RegionKey,
    pub name: &'static str,
    pub generated: bool,
    pub fingerprint: Option<&'static str>,
    pub abi: crate::stencil_select::RegionAbi,
    pub entry: u16,
    pub byte_len: usize,
}

#[inline]
fn cache_signature<const N: usize>(stencil: &Stencil, values: &PatchValues<'_, N>) -> u64 {
    // If a stencil has no relocations, its bytes are independent of the
    // quickening site. Do not allocate duplicate executable copies merely
    // because a caller's disposable guard state changed.
    if stencil.holes.is_empty() {
        0
    } else {
        values.signature()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArenaError {
    InvalidCapacity,
    Exhausted,
    MappingFailed,
    ProtectionFailed,
    Patch(PatchError),
}

pub struct StencilArena {
    ptr: *mut u8,
    capacity: usize,
    cursor: usize,
    executable: bool,
    id: u64,
    published_entries: RefCell<HashMap<usize, PublishedEntry>>,
    last_physical_execution: Cell<Option<PhysicalExecutionWitness>>,
    global_charge: BudgetReservation<'static>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PublishedEntry {
    key: crate::stencil_fact::RegionKey,
    signature: u64,
    abi: crate::stencil_select::RegionAbi,
    byte_len: usize,
}

impl PublishedEntry {
    const fn from_image(identity: RegionImageIdentity, byte_len: usize) -> Self {
        Self {
            key: identity.key,
            signature: identity.cache_signature,
            abi: identity.abi,
            byte_len,
        }
    }
}

/// Bounded collection of immutable-after-publication executable slabs.  Region
/// plans share this owner rather than each allocating a 4 KiB mapping. A new
/// slab is created only when every existing slab is RX or exhausted; published
/// slabs remain alive for the pool lifetime, so cached entry addresses cannot
/// dangle during replacement.
pub struct SharedStencilSlab {
    slabs: Vec<StencilArena>,
    cache: RenderedRegionCache,
    slab_capacity: usize,
    active_dispatches: Cell<usize>,
    peak_dispatches: Cell<usize>,
    lease_state: std::rc::Rc<LeaseState>,
    budget: &'static AtomicBudget,
}

struct LeaseState {
    active: Cell<usize>,
    peak: Cell<usize>,
    owners: RefCell<HashMap<u64, usize>>,
    retired: RefCell<HashSet<u64>>,
}

impl LeaseState {
    fn acquire(&self, owner: u64) -> bool {
        if self.is_retired(owner) {
            return false;
        }
        let active = self.active.get().saturating_add(1);
        self.active.set(active);
        self.peak.set(self.peak.get().max(active));
        let mut owners = self.owners.borrow_mut();
        let count = owners.entry(owner).or_default();
        *count = count.saturating_add(1);
        true
    }

    fn release(&self, owner: u64) -> bool {
        self.active.set(self.active.get().saturating_sub(1));
        let mut owners = self.owners.borrow_mut();
        let Some(count) = owners.get_mut(&owner) else {
            return true;
        };
        *count = count.saturating_sub(1);
        if *count == 0 {
            owners.remove(&owner);
            return true;
        }
        false
    }

    fn owns_active_lease(&self, owner: u64) -> bool {
        self.owners.borrow().get(&owner).copied().unwrap_or(0) != 0
    }

    fn retire(&self, owner: u64) {
        self.retired.borrow_mut().insert(owner);
    }

    fn is_retired(&self, owner: u64) -> bool {
        self.retired.borrow().contains(&owner)
    }

    fn forget(&self, owner: u64) {
        self.retired.borrow_mut().remove(&owner);
    }
}

/// A non-owning typed entry token paired with the slab generation that
/// published it. The address alone is not a stable capability: an evicted
/// mapping may be recycled by the OS. Callers pass this token through
/// `with_owned`, which acquires a retaining active lease for the call.
#[derive(Clone, Copy)]
pub(crate) struct EntryToken<F: Copy> {
    address: usize,
    entry_address: usize,
    owner: u64,
    abi: crate::stencil_select::RegionAbi,
    entry: F,
}

impl<F: Copy> EntryToken<F> {
    pub(crate) const fn address(self) -> usize {
        self.address
    }
}

pub(crate) type DispatchEntry = extern "C" fn(*mut std::ffi::c_void) -> u64;

struct ActiveUse<'a> {
    owner: &'a SharedStencilSlab,
}

pub(crate) struct AllocationLease {
    owner: std::rc::Rc<std::cell::RefCell<SharedStencilSlab>>,
    state: std::rc::Rc<LeaseState>,
    address: usize,
    owner_id: u64,
    abi: crate::stencil_select::RegionAbi,
}

impl AllocationLease {
    pub(crate) fn invoke_dispatch(self, context: *mut std::ffi::c_void) -> Result<u64, ArenaError> {
        if context.is_null() {
            return Err(ArenaError::ProtectionFailed);
        }
        let entry = {
            let pool = self
                .owner
                .try_borrow()
                .map_err(|_| ArenaError::ProtectionFailed)?;
            pool.validate_retained_address(self.address, self.owner_id, self.abi)?;
            pool.dispatch_entry_with_abi(self.address, self.abi)?
        };
        Ok(entry(context))
    }

    pub(crate) fn invoke<R>(self, invoke: impl FnOnce() -> R) -> Result<R, ArenaError> {
        let valid = self.owner.try_borrow().ok().is_some_and(|pool| {
            pool.validate_retained_address(self.address, self.owner_id, self.abi)
                .is_ok()
        });
        if !valid {
            return Err(ArenaError::ProtectionFailed);
        }
        Ok(invoke())
    }
}

impl Drop for AllocationLease {
    fn drop(&mut self) {
        if self.state.release(self.owner_id) {
            self.owner.borrow_mut().reclaim_retired_owner(self.owner_id);
        }
    }
}

pub(crate) struct OwnedLease<F: Copy> {
    allocation: AllocationLease,
    token: EntryToken<F>,
}

impl<F: Copy> OwnedLease<F> {
    pub(crate) fn invoke<R>(self, invoke: impl FnOnce(F) -> R) -> Result<R, ArenaError> {
        let token = self.token;
        self.allocation.invoke(|| invoke(token.entry))
    }
}

macro_rules! typed_owned_entry {
    ($name:ident, $entry:ident, $ty:ty, $abi:expr) => {
        pub(crate) fn $name(&self, address: usize) -> Result<EntryToken<$ty>, ArenaError> {
            let entry = self.$entry(address)?;
            let owner = self
                .owner_for(address)
                .ok_or(ArenaError::ProtectionFailed)?;
            self.validate_address(address, owner, $abi)?;
            Ok(EntryToken {
                address,
                entry_address: entry as usize,
                owner,
                abi: $abi,
                entry,
            })
        }
    };
}

macro_rules! typed_dispatch_entry {
    ($name:ident, $abi:expr) => {
        pub(crate) fn $name(&self, address: usize) -> Result<EntryToken<DispatchEntry>, ArenaError> {
            let entry = self.dispatch_entry_with_abi(address, $abi)?;
            let owner = self
                .owner_for(address)
                .ok_or(ArenaError::ProtectionFailed)?;
            self.validate_address(address, owner, $abi)?;
            Ok(EntryToken {
                address,
                entry_address: entry as usize,
                owner,
                abi: $abi,
                entry,
            })
        }
    };
}

impl Drop for ActiveUse<'_> {
    fn drop(&mut self) {
        let active = self.owner.active_dispatches.get();
        self.owner.active_dispatches.set(active.saturating_sub(1));
    }
}

impl SharedStencilSlab {
    pub fn new(slab_capacity: usize) -> Result<Self, ArenaError> {
        Self::new_in_budget(slab_capacity, &GLOBAL_EXECUTABLE_BUDGET)
    }

    fn new_in_budget(
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

    fn total_capacity(&self) -> usize {
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

    fn reclaim_retired_owner(&mut self, owner: u64) -> bool {
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
        if self.active_dispatches.get() != 0 {
            return 0;
        }
        let owners = self.remove_idle_owners(retain);
        for owner in &owners {
            self.cache.remove_owner(*owner);
        }
        owners.len()
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
        if self.active_dispatches.get() != 0 {
            return 0;
        }
        let owners = self.remove_idle_owners(retain);
        for owner in &owners {
            self.cache.remove_owner(*owner);
            cache.remove_owner(*owner);
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

    fn slab_for(&self, address: usize) -> Option<&StencilArena> {
        self.slabs.iter().find(|slab| slab.owns_address(address))
    }

    fn slab_for_mut(&mut self, address: usize) -> Option<&mut StencilArena> {
        self.slabs
            .iter_mut()
            .find(|slab| slab.owns_address(address))
    }

    pub(crate) fn owner_for(&self, address: usize) -> Option<u64> {
        self.slab_for(address).map(StencilArena::id)
    }

    fn validate_address(
        &self,
        address: usize,
        owner: u64,
        abi: crate::stencil_select::RegionAbi,
    ) -> Result<(), ArenaError> {
        if self.lease_state.is_retired(owner) {
            return Err(ArenaError::ProtectionFailed);
        }
        self.validate_retained_address(address, owner, abi)
    }

    fn validate_retained_address(
        &self,
        address: usize,
        owner: u64,
        abi: crate::stencil_select::RegionAbi,
    ) -> Result<(), ArenaError> {
        if self.owner_for(address) != Some(owner) {
            return Err(ArenaError::ProtectionFailed);
        }
        self.slab_for(address)
            .ok_or(ArenaError::ProtectionFailed)?
            .require_abi(address, abi)
    }

    fn validate_token<F: Copy>(&self, owned: EntryToken<F>) -> Result<(), ArenaError> {
        if owned.entry_address != owned.address {
            return Err(ArenaError::ProtectionFailed);
        }
        self.validate_address(owned.address, owned.owner, owned.abi)
    }

    pub(crate) fn entry_token_is_live<F: Copy>(&self, token: EntryToken<F>) -> bool {
        self.validate_token(token).is_ok()
    }

    pub(crate) fn acquire_lease(
        owner: &std::rc::Rc<std::cell::RefCell<Self>>,
        address: usize,
        owner_id: u64,
        abi: crate::stencil_select::RegionAbi,
    ) -> Result<AllocationLease, ArenaError> {
        let state = {
            let pool = owner.borrow();
            pool.validate_address(address, owner_id, abi)?;
            std::rc::Rc::clone(&pool.lease_state)
        };
        if !state.acquire(owner_id) {
            return Err(ArenaError::ProtectionFailed);
        }
        Ok(AllocationLease {
            owner: std::rc::Rc::clone(owner),
            state,
            address,
            owner_id,
            abi,
        })
    }

    pub(crate) fn acquire_address_lease(
        owner: &std::rc::Rc<std::cell::RefCell<Self>>,
        address: usize,
        abi: crate::stencil_select::RegionAbi,
    ) -> Result<AllocationLease, ArenaError> {
        let owner_id = owner
            .borrow()
            .owner_for(address)
            .ok_or(ArenaError::ProtectionFailed)?;
        Self::acquire_lease(owner, address, owner_id, abi)
    }

    pub(crate) fn acquire_owned<F: Copy>(
        owner: &std::rc::Rc<std::cell::RefCell<Self>>,
        token: EntryToken<F>,
    ) -> Result<OwnedLease<F>, ArenaError> {
        {
            let pool = owner.borrow();
            pool.validate_token(token)?;
        }
        let allocation = Self::acquire_lease(owner, token.address, token.owner, token.abi)?;
        Ok(OwnedLease { allocation, token })
    }

    typed_owned_entry!(
        owned_f64_entry,
        f64_entry,
        extern "C" fn(f64, f64) -> f64,
        crate::stencil_select::RegionAbi::ScalarF64Binary
    );
    typed_owned_entry!(
        owned_f64x3_entry,
        f64x3_entry,
        extern "C" fn(f64, f64, f64) -> f64,
        crate::stencil_select::RegionAbi::ScalarF64x3
    );
    typed_owned_entry!(
        owned_bool_entry,
        bool_entry,
        extern "C" fn(f64, f64) -> u64,
        crate::stencil_select::RegionAbi::ScalarBool
    );
    typed_owned_entry!(
        owned_i32_entry,
        i32_entry,
        extern "C" fn(i32, i32) -> i32,
        crate::stencil_select::RegionAbi::ScalarI32
    );
    typed_owned_entry!(
        owned_u32_entry,
        u32_entry,
        extern "C" fn(u32, u32) -> u32,
        crate::stencil_select::RegionAbi::ScalarU32
    );
    typed_owned_entry!(
        owned_f64_unary_entry,
        f64_unary_entry,
        extern "C" fn(f64) -> f64,
        crate::stencil_select::RegionAbi::ScalarF64Unary
    );
    typed_owned_entry!(
        owned_i32_unary_entry,
        i32_unary_entry,
        extern "C" fn(i32) -> i32,
        crate::stencil_select::RegionAbi::ScalarI32
    );
    typed_owned_entry!(
        owned_bool_unary_entry,
        bool_unary_entry,
        extern "C" fn(f64) -> u64,
        crate::stencil_select::RegionAbi::ScalarBool
    );
    typed_owned_entry!(
        owned_word_bool_entry,
        word_bool_entry,
        extern "C" fn(u64) -> u64,
        crate::stencil_select::RegionAbi::ScalarWordBool
    );
    typed_owned_entry!(
        owned_word_pair_bool_entry,
        word_pair_bool_entry,
        extern "C" fn(u64, u64) -> u64,
        crate::stencil_select::RegionAbi::ScalarWordPairBool
    );
    typed_owned_entry!(
        owned_constant_word_entry,
        constant_word_entry,
        extern "C" fn() -> u64,
        crate::stencil_select::RegionAbi::ConstantWord
    );
    typed_owned_entry!(
        owned_tagged_word_entry,
        tagged_word_entry,
        extern "C" fn(*const crate::tagged_value::TaggedValue) -> u64,
        crate::stencil_select::RegionAbi::TaggedWord
    );
    typed_owned_entry!(
        owned_property_guard_entry,
        property_guard_entry,
        extern "C" fn(*mut crate::native_property::NativePropertyReadContext) -> u32,
        crate::stencil_select::RegionAbi::PropertyGuard
    );
    typed_owned_entry!(
        owned_property_write_guard_entry,
        property_write_guard_entry,
        extern "C" fn(*mut crate::native_property::NativePropertyWriteContext) -> u32,
        crate::stencil_select::RegionAbi::PropertyWriteGuard
    );
    typed_owned_entry!(
        owned_compare_branch_entry,
        compare_branch_entry,
        extern "C" fn(*mut crate::native_control::NativeCompareBranchContext) -> u32,
        crate::stencil_select::RegionAbi::CompareBranch
    );
    typed_dispatch_entry!(owned_bridge_entry, crate::stencil_select::RegionAbi::Bridge);
    typed_dispatch_entry!(
        owned_array_kernel_entry,
        crate::stencil_select::RegionAbi::ArrayKernel
    );
    typed_dispatch_entry!(
        owned_array_numeric_loop_entry,
        crate::stencil_select::RegionAbi::ArrayNumericLoop
    );

    pub(crate) fn with_owned<F: Copy, R>(
        &self,
        owned: EntryToken<F>,
        invoke: impl FnOnce(F) -> R,
    ) -> Result<R, ArenaError> {
        self.validate_token(owned)?;
        self.with_active(owned.address, || invoke(owned.entry))
    }

    pub fn make_executable(&mut self, address: usize) -> Result<(), ArenaError> {
        self.slab_for_mut(address)
            .ok_or(ArenaError::ProtectionFailed)?
            .make_executable()
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn f64_entry(
        &self,
        address: usize,
    ) -> Result<extern "C" fn(f64, f64) -> f64, ArenaError> {
        self.slab_for(address)
            .ok_or(ArenaError::ProtectionFailed)?
            .f64_entry(address)
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn f64_unary_entry(
        &self,
        address: usize,
    ) -> Result<extern "C" fn(f64) -> f64, ArenaError> {
        self.slab_for(address)
            .ok_or(ArenaError::ProtectionFailed)?
            .f64_unary_entry(address)
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn bool_entry(
        &self,
        address: usize,
    ) -> Result<extern "C" fn(f64, f64) -> u64, ArenaError> {
        self.slab_for(address)
            .ok_or(ArenaError::ProtectionFailed)?
            .bool_entry(address)
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn i32_entry(
        &self,
        address: usize,
    ) -> Result<extern "C" fn(i32, i32) -> i32, ArenaError> {
        self.slab_for(address)
            .ok_or(ArenaError::ProtectionFailed)?
            .i32_entry(address)
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn i32_unary_entry(
        &self,
        address: usize,
    ) -> Result<extern "C" fn(i32) -> i32, ArenaError> {
        self.slab_for(address)
            .ok_or(ArenaError::ProtectionFailed)?
            .i32_unary_entry(address)
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    pub(crate) fn i32_unary_entry(
        &self,
        _address: usize,
    ) -> Result<extern "C" fn(i32) -> i32, ArenaError> {
        Err(ArenaError::ProtectionFailed)
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn u32_entry(
        &self,
        address: usize,
    ) -> Result<extern "C" fn(u32, u32) -> u32, ArenaError> {
        self.slab_for(address)
            .ok_or(ArenaError::ProtectionFailed)?
            .u32_entry(address)
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn constant_word_entry(
        &self,
        address: usize,
    ) -> Result<extern "C" fn() -> u64, ArenaError> {
        self.slab_for(address)
            .ok_or(ArenaError::ProtectionFailed)?
            .constant_word_entry(address)
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn bool_unary_entry(
        &self,
        address: usize,
    ) -> Result<extern "C" fn(f64) -> u64, ArenaError> {
        self.slab_for(address)
            .ok_or(ArenaError::ProtectionFailed)?
            .bool_unary_entry(address)
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn word_bool_entry(
        &self,
        address: usize,
    ) -> Result<extern "C" fn(u64) -> u64, ArenaError> {
        self.slab_for(address)
            .ok_or(ArenaError::ProtectionFailed)?
            .word_bool_entry(address)
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn word_pair_bool_entry(
        &self,
        address: usize,
    ) -> Result<extern "C" fn(u64, u64) -> u64, ArenaError> {
        self.slab_for(address)
            .ok_or(ArenaError::ProtectionFailed)?
            .word_pair_bool_entry(address)
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn f64x3_entry(
        &self,
        address: usize,
    ) -> Result<extern "C" fn(f64, f64, f64) -> f64, ArenaError> {
        self.slab_for(address)
            .ok_or(ArenaError::ProtectionFailed)?
            .f64x3_entry(address)
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn tagged_word_entry(
        &self,
        address: usize,
    ) -> Result<extern "C" fn(*const crate::tagged_value::TaggedValue) -> u64, ArenaError> {
        self.slab_for(address)
            .ok_or(ArenaError::ProtectionFailed)?
            .tagged_word_entry(address)
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn property_guard_entry(
        &self,
        address: usize,
    ) -> Result<
        extern "C" fn(*mut crate::native_property::NativePropertyReadContext) -> u32,
        ArenaError,
    > {
        self.slab_for(address)
            .ok_or(ArenaError::ProtectionFailed)?
            .property_guard_entry(address)
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn property_write_guard_entry(
        &self,
        address: usize,
    ) -> Result<
        extern "C" fn(*mut crate::native_property::NativePropertyWriteContext) -> u32,
        ArenaError,
    > {
        self.slab_for(address)
            .ok_or(ArenaError::ProtectionFailed)?
            .property_write_guard_entry(address)
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn compare_branch_entry(
        &self,
        address: usize,
    ) -> Result<
        extern "C" fn(*mut crate::native_control::NativeCompareBranchContext) -> u32,
        ArenaError,
    > {
        self.slab_for(address)
            .ok_or(ArenaError::ProtectionFailed)?
            .compare_branch_entry(address)
    }

    pub fn execute_dispatch(
        &self,
        address: usize,
        context: *mut std::ffi::c_void,
    ) -> Result<u64, ArenaError> {
        self.execute_dispatch_with_abi(address, context, crate::stencil_select::RegionAbi::Bridge)
    }

    pub(crate) fn execute_dispatch_with_abi(
        &self,
        address: usize,
        context: *mut std::ffi::c_void,
        abi: crate::stencil_select::RegionAbi,
    ) -> Result<u64, ArenaError> {
        self.slab_for(address).ok_or(ArenaError::ProtectionFailed)?;
        let entry = self.dispatch_entry_with_abi(address, abi)?;
        self.with_active(address, || entry(context))
    }

    pub(crate) fn dispatch_entry_with_abi(
        &self,
        address: usize,
        abi: crate::stencil_select::RegionAbi,
    ) -> Result<extern "C" fn(*mut std::ffi::c_void) -> u64, ArenaError> {
        let slab = self.slab_for(address).ok_or(ArenaError::ProtectionFailed)?;
        slab.dispatch_entry_with_abi(address, abi)
    }

    /// Execute a typed scalar entry while retaining the owning slab.  The
    /// function pointer is valid only for the published allocation; keeping
    /// the active count elevated across the call prevents idle eviction from
    /// reclaiming that allocation between lookup and invocation.
    pub(crate) fn with_active<R>(
        &self,
        address: usize,
        invoke: impl FnOnce() -> R,
    ) -> Result<R, ArenaError> {
        if self.slab_for(address).is_none() {
            return Err(ArenaError::ProtectionFailed);
        }
        let active = self.active_dispatches.get().saturating_add(1);
        self.active_dispatches.set(active);
        self.peak_dispatches
            .set(self.peak_dispatches.get().max(active));
        let _guard = ActiveUse { owner: self };
        let result = invoke();
        Ok(result)
    }
}

impl std::fmt::Debug for SharedStencilSlab {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SharedStencilSlab")
            .field("slabs", &self.slabs.len())
            .field("used", &self.used())
            .field("capacity", &self.capacity())
            .field("active_dispatches", &self.active_dispatches())
            .field("peak_dispatches", &self.peak_dispatches())
            .finish()
    }
}

fn render_arena_physical<const N: usize>(
    arena: &mut StencilArena,
    cache: &mut RenderedRegionCache,
    view: crate::stencil_select::PhysicalStencilView,
    values: &PatchValues<'_, N>,
    control: Option<&crate::stencil_cfg::RegionControlPlan>,
) -> Result<usize, ArenaError> {
    match control {
        Some(control) => arena.render_selected_controlled_view(cache, view, values, control),
        None => arena.render_selected_physical_view(cache, view, values),
    }
}

impl StencilArena {
    pub fn new(capacity: usize) -> Result<Self, ArenaError> {
        Self::new_in_budget(capacity, &GLOBAL_EXECUTABLE_BUDGET)
    }

    fn new_in_budget(capacity: usize, budget: &'static AtomicBudget) -> Result<Self, ArenaError> {
        if capacity == 0 || capacity > MAX_ARENA_BYTES {
            return Err(ArenaError::InvalidCapacity);
        }
        let capacity = capacity
            .checked_add(PAGE - 1)
            .ok_or(ArenaError::InvalidCapacity)?
            & !(PAGE - 1);
        let global_charge = budget.reserve(capacity).ok_or(ArenaError::Exhausted)?;
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                capacity,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANON,
                -1,
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(ArenaError::MappingFailed);
        }
        Ok(Self {
            ptr: ptr.cast(),
            capacity,
            cursor: 0,
            executable: false,
            id: NEXT_ARENA_ID.fetch_add(1, Ordering::Relaxed),
            published_entries: RefCell::new(HashMap::new()),
            last_physical_execution: Cell::new(None),
            global_charge,
        })
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }
    pub fn used(&self) -> usize {
        self.cursor
    }
    pub fn remaining(&self) -> usize {
        self.capacity.saturating_sub(self.cursor)
    }
    pub fn is_executable(&self) -> bool {
        self.executable
    }

    pub fn last_physical_execution(&self) -> Option<PhysicalExecutionWitness> {
        self.last_physical_execution.get()
    }

    fn mark_physical_execution(&self, view: crate::stencil_select::PhysicalStencilView) {
        self.last_physical_execution
            .set(Some(PhysicalExecutionWitness {
                key: view.key,
                name: view.record.name,
                generated: view.generated,
                fingerprint: view.fingerprint,
                abi: view.abi,
                entry: view.entry,
                byte_len: view.stencil.bytes.len()
                    + view.fallthrough.map_or(0, |item| item.stencil.bytes.len()),
            }));
    }

    /// Stable owner token used to validate cached entry pointers.  It is
    /// distinct from the virtual address because the OS may recycle mappings.
    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    pub fn address(&self, offset: usize) -> Option<usize> {
        (offset < self.cursor).then(|| self.ptr.wrapping_add(offset) as usize)
    }

    fn owns_address(&self, address: usize) -> bool {
        let base = self.ptr as usize;
        let end = base.saturating_add(self.cursor);
        address >= base && address < end
    }

    /// Invoke the build-time Number Add+Return stencil using the platform ABI.  The
    /// address must belong to this arena and the arena must already be RX;
    /// otherwise the complete ordinary path remains the only valid option.
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub fn execute_f64(&self, address: usize, lhs: f64, rhs: f64) -> Result<f64, ArenaError> {
        let entry = self.f64_entry(address)?;
        Ok(entry(lhs, rhs))
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub fn execute_bool(&self, address: usize, lhs: f64, rhs: f64) -> Result<bool, ArenaError> {
        let entry = self.bool_entry(address)?;
        Ok(entry(lhs, rhs) != 0)
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub fn execute_i32(&self, address: usize, lhs: i32, rhs: i32) -> Result<i32, ArenaError> {
        let entry = self.i32_entry(address)?;
        Ok(entry(lhs, rhs))
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub fn execute_u32(&self, address: usize, lhs: u32, rhs: u32) -> Result<u32, ArenaError> {
        let entry = self.u32_entry(address)?;
        Ok(entry(lhs, rhs))
    }

    /// Validate an installed numeric entry once, then hand the caller the
    /// typed code pointer for its steady-state loop.  The arena is immutable
    /// after `make_executable`, so a pointer returned here remains valid until
    /// this arena is dropped; callers retain the arena alongside the pointer.
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn f64_entry(
        &self,
        address: usize,
    ) -> Result<extern "C" fn(f64, f64) -> f64, ArenaError> {
        self.require_abi(address, crate::stencil_select::RegionAbi::ScalarF64Binary)?;
        let base = self.ptr as usize;
        let end = base.saturating_add(self.cursor);
        if !self.executable || address < base || address >= end {
            return Err(ArenaError::ProtectionFailed);
        }
        Ok(unsafe { std::mem::transmute(address) })
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn compare_branch_entry(
        &self,
        address: usize,
    ) -> Result<
        extern "C" fn(*mut crate::native_control::NativeCompareBranchContext) -> u32,
        ArenaError,
    > {
        self.require_abi(address, crate::stencil_select::RegionAbi::CompareBranch)?;
        let base = self.ptr as usize;
        let end = base.saturating_add(self.cursor);
        if !self.executable || address < base || address >= end {
            return Err(ArenaError::ProtectionFailed);
        }
        Ok(unsafe { std::mem::transmute(address) })
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn property_write_guard_entry(
        &self,
        address: usize,
    ) -> Result<
        extern "C" fn(*mut crate::native_property::NativePropertyWriteContext) -> u32,
        ArenaError,
    > {
        self.require_abi(
            address,
            crate::stencil_select::RegionAbi::PropertyWriteGuard,
        )?;
        let base = self.ptr as usize;
        let end = base.saturating_add(self.cursor);
        if !self.executable || address < base || address >= end {
            return Err(ArenaError::ProtectionFailed);
        }
        Ok(unsafe { std::mem::transmute(address) })
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn f64_unary_entry(
        &self,
        address: usize,
    ) -> Result<extern "C" fn(f64) -> f64, ArenaError> {
        self.require_abi(address, crate::stencil_select::RegionAbi::ScalarF64Unary)?;
        let base = self.ptr as usize;
        let end = base.saturating_add(self.cursor);
        if !self.executable || address < base || address >= end {
            return Err(ArenaError::ProtectionFailed);
        }
        Ok(unsafe { std::mem::transmute(address) })
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn word_bool_entry(
        &self,
        address: usize,
    ) -> Result<extern "C" fn(u64) -> u64, ArenaError> {
        self.require_abi(address, crate::stencil_select::RegionAbi::ScalarWordBool)?;
        let base = self.ptr as usize;
        let end = base.saturating_add(self.cursor);
        if !self.executable || address < base || address >= end {
            return Err(ArenaError::ProtectionFailed);
        }
        Ok(unsafe { std::mem::transmute(address) })
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn word_pair_bool_entry(
        &self,
        address: usize,
    ) -> Result<extern "C" fn(u64, u64) -> u64, ArenaError> {
        self.require_abi(
            address,
            crate::stencil_select::RegionAbi::ScalarWordPairBool,
        )?;
        let base = self.ptr as usize;
        let end = base.saturating_add(self.cursor);
        if !self.executable || address < base || address >= end {
            return Err(ArenaError::ProtectionFailed);
        }
        Ok(unsafe { std::mem::transmute(address) })
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn bool_unary_entry(
        &self,
        address: usize,
    ) -> Result<extern "C" fn(f64) -> u64, ArenaError> {
        self.require_abi(address, crate::stencil_select::RegionAbi::ScalarBool)?;
        let base = self.ptr as usize;
        let end = base.saturating_add(self.cursor);
        if !self.executable || address < base || address >= end {
            return Err(ArenaError::ProtectionFailed);
        }
        Ok(unsafe { std::mem::transmute(address) })
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn constant_word_entry(
        &self,
        address: usize,
    ) -> Result<extern "C" fn() -> u64, ArenaError> {
        self.require_abi(address, crate::stencil_select::RegionAbi::ConstantWord)?;
        let base = self.ptr as usize;
        let end = base.saturating_add(self.cursor);
        if !self.executable || address < base || address >= end {
            return Err(ArenaError::ProtectionFailed);
        }
        Ok(unsafe { std::mem::transmute(address) })
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn bool_entry(
        &self,
        address: usize,
    ) -> Result<extern "C" fn(f64, f64) -> u64, ArenaError> {
        self.require_abi(address, crate::stencil_select::RegionAbi::ScalarBool)?;
        let base = self.ptr as usize;
        let end = base.saturating_add(self.cursor);
        if !self.executable || address < base || address >= end {
            return Err(ArenaError::ProtectionFailed);
        }
        Ok(unsafe { std::mem::transmute(address) })
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn i32_entry(
        &self,
        address: usize,
    ) -> Result<extern "C" fn(i32, i32) -> i32, ArenaError> {
        self.require_abi(address, crate::stencil_select::RegionAbi::ScalarI32)?;
        let base = self.ptr as usize;
        let end = base.saturating_add(self.cursor);
        if !self.executable || address < base || address >= end {
            return Err(ArenaError::ProtectionFailed);
        }
        Ok(unsafe { std::mem::transmute(address) })
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn i32_unary_entry(
        &self,
        address: usize,
    ) -> Result<extern "C" fn(i32) -> i32, ArenaError> {
        self.require_abi(address, crate::stencil_select::RegionAbi::ScalarI32)?;
        let base = self.ptr as usize;
        let end = base.saturating_add(self.cursor);
        if !self.executable || address < base || address >= end {
            return Err(ArenaError::ProtectionFailed);
        }
        Ok(unsafe { std::mem::transmute(address) })
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    pub(crate) fn i32_unary_entry(
        &self,
        _address: usize,
    ) -> Result<extern "C" fn(i32) -> i32, ArenaError> {
        Err(ArenaError::ProtectionFailed)
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn u32_entry(
        &self,
        address: usize,
    ) -> Result<extern "C" fn(u32, u32) -> u32, ArenaError> {
        self.require_abi(address, crate::stencil_select::RegionAbi::ScalarU32)?;
        let base = self.ptr as usize;
        let end = base.saturating_add(self.cursor);
        if !self.executable || address < base || address >= end {
            return Err(ArenaError::ProtectionFailed);
        }
        Ok(unsafe { std::mem::transmute(address) })
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn f64x3_entry(
        &self,
        address: usize,
    ) -> Result<extern "C" fn(f64, f64, f64) -> f64, ArenaError> {
        self.require_abi(address, crate::stencil_select::RegionAbi::ScalarF64x3)?;
        let base = self.ptr as usize;
        let end = base.saturating_add(self.cursor);
        if !self.executable || address < base || address >= end {
            return Err(ArenaError::ProtectionFailed);
        }
        Ok(unsafe { std::mem::transmute(address) })
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn tagged_word_entry(
        &self,
        address: usize,
    ) -> Result<extern "C" fn(*const crate::tagged_value::TaggedValue) -> u64, ArenaError> {
        self.require_abi(address, crate::stencil_select::RegionAbi::TaggedWord)?;
        let base = self.ptr as usize;
        let end = base.saturating_add(self.cursor);
        if !self.executable || address < base || address >= end {
            return Err(ArenaError::ProtectionFailed);
        }
        Ok(unsafe { std::mem::transmute(address) })
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn property_guard_entry(
        &self,
        address: usize,
    ) -> Result<
        extern "C" fn(*mut crate::native_property::NativePropertyReadContext) -> u32,
        ArenaError,
    > {
        self.require_abi(address, crate::stencil_select::RegionAbi::PropertyGuard)?;
        let base = self.ptr as usize;
        let end = base.saturating_add(self.cursor);
        if !self.executable || address < base || address >= end {
            return Err(ArenaError::ProtectionFailed);
        }
        Ok(unsafe { std::mem::transmute(address) })
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    pub fn execute_f64(&self, _address: usize, _lhs: f64, _rhs: f64) -> Result<f64, ArenaError> {
        Err(ArenaError::ProtectionFailed)
    }

    /// Invoke the generated tagged-word property leaf.  The leaf only loads
    /// from a slot already validated by the complete Rust property gateway;
    /// the caller performs the owning retain when writing the returned bits.
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub fn execute_word(
        &self,
        address: usize,
        slot: *const crate::register_file::SlotWord,
    ) -> Result<u64, ArenaError> {
        self.execute_tagged_word(address, slot.cast())
    }

    /// Invoke a raw tagged-word leaf. The leaf is read-only; ownership is
    /// deliberately handled by the Rust register writer after the return.
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub fn execute_tagged_word(
        &self,
        address: usize,
        word: *const crate::tagged_value::TaggedValue,
    ) -> Result<u64, ArenaError> {
        if word.is_null() {
            return Err(ArenaError::ProtectionFailed);
        }
        let entry = self.tagged_word_entry(address)?;
        Ok(entry(word))
    }

    /// Invoke an executable baseline-entry trampoline.  The generated bytes
    /// receive one opaque context pointer in the platform's first argument
    /// register and tail-call the canonical Rust bridge.  The arena performs
    /// only address/protection checks; the bridge owns all VM semantics.
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub fn execute_dispatch(
        &self,
        address: usize,
        context: *mut std::ffi::c_void,
    ) -> Result<u64, ArenaError> {
        self.execute_dispatch_with_abi(address, context, crate::stencil_select::RegionAbi::Bridge)
    }

    pub fn execute_dispatch_with_abi(
        &self,
        address: usize,
        context: *mut std::ffi::c_void,
        abi: crate::stencil_select::RegionAbi,
    ) -> Result<u64, ArenaError> {
        if context.is_null() {
            return Err(ArenaError::ProtectionFailed);
        }
        Ok(self.dispatch_entry_with_abi(address, abi)?(context))
    }

    pub(crate) fn dispatch_entry_with_abi(
        &self,
        address: usize,
        abi: crate::stencil_select::RegionAbi,
    ) -> Result<extern "C" fn(*mut std::ffi::c_void) -> u64, ArenaError> {
        self.require_abi(address, abi)?;
        Ok(unsafe { std::mem::transmute(address) })
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    pub fn execute_dispatch(
        &self,
        _address: usize,
        _context: *mut std::ffi::c_void,
    ) -> Result<u64, ArenaError> {
        Err(ArenaError::ProtectionFailed)
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    pub fn execute_word(
        &self,
        _address: usize,
        _slot: *const crate::register_file::SlotWord,
    ) -> Result<u64, ArenaError> {
        Err(ArenaError::ProtectionFailed)
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    pub fn execute_tagged_word(
        &self,
        _address: usize,
        _word: *const crate::tagged_value::TaggedValue,
    ) -> Result<u64, ArenaError> {
        Err(ArenaError::ProtectionFailed)
    }

    pub fn alloc(&mut self, size: usize) -> Result<usize, ArenaError> {
        if self.executable {
            return Err(ArenaError::ProtectionFailed);
        }
        let end = self.cursor.checked_add(size).ok_or(ArenaError::Exhausted)?;
        if end > self.capacity {
            return Err(ArenaError::Exhausted);
        }
        let offset = self.cursor;
        self.cursor = end;
        Ok(offset)
    }

    fn alloc_aligned(&mut self, size: usize, alignment: usize) -> Result<usize, ArenaError> {
        let mask = alignment.checked_sub(1).ok_or(ArenaError::Exhausted)?;
        if alignment == 0 || !alignment.is_power_of_two() {
            return Err(ArenaError::Exhausted);
        }
        let aligned = self
            .cursor
            .checked_add(mask)
            .map(|cursor| cursor & !mask)
            .ok_or(ArenaError::Exhausted)?;
        self.alloc(aligned.saturating_sub(self.cursor))?;
        self.alloc(size)
    }

    pub fn copy_and_patch<const N: usize>(
        &mut self,
        stencil: &Stencil,
        values: &PatchValues<'_, N>,
    ) -> Result<usize, ArenaError> {
        let offset = self.alloc_aligned(stencil.bytes.len(), STENCIL_ALIGNMENT)?;
        let result = unsafe {
            std::ptr::copy_nonoverlapping(
                stencil.bytes.as_ptr(),
                self.ptr.add(offset),
                stencil.bytes.len(),
            );
            let dst = std::slice::from_raw_parts_mut(self.ptr.add(offset), stencil.bytes.len());
            apply_holes(dst, stencil.holes, values).map_err(ArenaError::Patch)
        };
        if let Err(error) = result {
            // The allocation is not published when patching fails; roll the
            // bump pointer back so no partial region can ever be selected.
            self.cursor = offset;
            return Err(error);
        }
        Ok(offset)
    }

    /// Render once per canonical key and patch-state signature. A cache hit
    /// returns before allocation, copying, or patching, which is the zero-copy
    /// memoized path.
    pub fn render_or_get<const N: usize>(
        &mut self,
        cache: &mut RenderedRegionCache,
        key: crate::stencil_fact::RegionKey,
        stencil: &Stencil,
        values: &PatchValues<'_, N>,
    ) -> Result<usize, ArenaError> {
        match crate::stencil_select::select_physical(key) {
            Some(view) => {
                if view.stencil.bytes != stencil.bytes || view.stencil.holes != stencil.holes {
                    return Err(ArenaError::ProtectionFailed);
                }
                return self.render_selected_view(cache, view, values);
            }
            None if crate::stencil_select::select_region(key).is_some() => {
                return Err(ArenaError::ProtectionFailed);
            }
            None => {}
        }
        let signature = cache_signature(stencil, values);
        if let Some(address) = cache
            .get_owned(key, signature, self.id)
            .filter(|address| self.owns_address(*address))
        {
            return Ok(address);
        }
        if !stencil.validate() {
            return Err(ArenaError::Patch(PatchError::OutOfBounds));
        }
        let offset = self.copy_and_patch(stencil, values)?;
        let address = self.address(offset).ok_or(ArenaError::Exhausted)?;
        Ok(cache.insert_owned(key, signature, address, self.id))
    }

    fn render_selected_view<const N: usize>(
        &mut self,
        cache: &mut RenderedRegionCache,
        view: crate::stencil_select::PhysicalStencilView,
        values: &PatchValues<'_, N>,
    ) -> Result<usize, ArenaError> {
        if !view.contract().abi_is_well_formed() || !view.stencil.validate() {
            return Err(ArenaError::ProtectionFailed);
        }
        let signature = view.cache_signature(values);
        let identity = RegionImageIdentity::selected(view, values);
        if let Some(address) = cache
            .get_owned(view.key, signature, self.id)
            .filter(|address| self.owns_address(*address))
        {
            self.require_publication(address, identity, view.stencil.bytes.len())?;
            return Ok(address);
        }
        let checkpoint = self.cursor;
        let offset = self.copy_and_patch(view.stencil, values)?;
        let address = self.address(offset).ok_or(ArenaError::Exhausted)?;
        if let Err(error) = self.record_publication(address, identity, view.stencil.bytes.len()) {
            self.cursor = checkpoint;
            return Err(error);
        }
        Ok(cache.insert_owned(view.key, signature, address, self.id))
    }

    fn render_selected_physical_view<const N: usize>(
        &mut self,
        cache: &mut RenderedRegionCache,
        view: crate::stencil_select::PhysicalStencilView,
        values: &PatchValues<'_, N>,
    ) -> Result<usize, ArenaError> {
        self.render_selected_physical_view_with_control(cache, view, values, None)
    }

    fn render_selected_controlled_view<const N: usize>(
        &mut self,
        cache: &mut RenderedRegionCache,
        view: crate::stencil_select::PhysicalStencilView,
        values: &PatchValues<'_, N>,
        control: &crate::stencil_cfg::RegionControlPlan,
    ) -> Result<usize, ArenaError> {
        self.render_selected_physical_view_with_control(cache, view, values, Some(control))
    }

    fn render_selected_physical_view_with_control<const N: usize>(
        &mut self,
        cache: &mut RenderedRegionCache,
        view: crate::stencil_select::PhysicalStencilView,
        values: &PatchValues<'_, N>,
        control: Option<&crate::stencil_cfg::RegionControlPlan>,
    ) -> Result<usize, ArenaError> {
        if view.fallthrough.is_none() {
            return self.render_selected_view(cache, view, values);
        }
        let signature = view.cache_signature(values);
        if let Some(address) = self.cached_executable(cache, view, signature) {
            return Ok(address);
        }
        let image = match control {
            Some(control) => compose_selected_controlled_region(view, control, values),
            None => compose_selected_region(view, values),
        }
        .map_err(|_| ArenaError::ProtectionFailed)?;
        self.publish_composed(cache, &image)
    }

    pub fn render_physical_view_or_get<const N: usize>(
        &mut self,
        cache: &mut RenderedRegionCache,
        view: crate::stencil_select::PhysicalStencilView,
        values: &PatchValues<'_, N>,
    ) -> Result<usize, ArenaError> {
        let selected = crate::stencil_select::select_physical_for_abi(view.key, view.abi)
            .ok_or(ArenaError::ProtectionFailed)?;
        if !view.contract().abi_is_well_formed() || !view.matches(&selected) {
            return Err(ArenaError::ProtectionFailed);
        }
        self.render_selected_physical_view(cache, view, values)
    }

    pub(crate) fn render_controlled_physical_view_or_get<const N: usize>(
        &mut self,
        cache: &mut RenderedRegionCache,
        view: crate::stencil_select::PhysicalStencilView,
        values: &PatchValues<'_, N>,
        control: &crate::stencil_cfg::RegionControlPlan,
    ) -> Result<usize, ArenaError> {
        let selected = crate::stencil_select::select_physical_for_abi(view.key, view.abi)
            .ok_or(ArenaError::ProtectionFailed)?;
        if !view.contract().abi_is_well_formed() || !view.matches(&selected) {
            return Err(ArenaError::ProtectionFailed);
        }
        self.render_selected_controlled_view(cache, view, values, control)
    }

    fn record_publication(
        &self,
        address: usize,
        identity: RegionImageIdentity,
        byte_len: usize,
    ) -> Result<(), ArenaError> {
        let entry = PublishedEntry::from_image(identity, byte_len);
        let mut published = self.published_entries.borrow_mut();
        match published.get(&address) {
            Some(existing) if *existing != entry => Err(ArenaError::ProtectionFailed),
            Some(_) => Ok(()),
            None => {
                published.insert(address, entry);
                Ok(())
            }
        }
    }

    fn require_publication(
        &self,
        address: usize,
        identity: RegionImageIdentity,
        byte_len: usize,
    ) -> Result<(), ArenaError> {
        let expected = PublishedEntry::from_image(identity, byte_len);
        (self.published_entries.borrow().get(&address) == Some(&expected))
            .then_some(())
            .ok_or(ArenaError::ProtectionFailed)
    }

    fn require_abi(
        &self,
        address: usize,
        expected: crate::stencil_select::RegionAbi,
    ) -> Result<(), ArenaError> {
        let actual = self
            .published_entries
            .borrow()
            .get(&address)
            .map(|entry| entry.abi);
        (actual == Some(expected) && self.executable && self.owns_address(address))
            .then_some(())
            .ok_or(ArenaError::ProtectionFailed)
    }

    /// Execute an installed region through the caller-supplied semantic entry
    /// point, with the ordinary interpreter as the complete fallback. The
    /// arena owns only placement; it never invents JavaScript semantics.
    pub fn render_and_execute<const N: usize, T, E>(
        &mut self,
        cache: &mut RenderedRegionCache,
        key: crate::stencil_fact::RegionKey,
        stencil: &Stencil,
        values: &PatchValues<'_, N>,
        execute: impl FnOnce(usize) -> Result<T, E>,
        fallback: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E> {
        let selected = crate::stencil_select::select_physical(key);
        if let Some(view) = selected.filter(|view| {
            view.stencil.bytes == stencil.bytes && view.stencil.holes == stencil.holes
        }) {
            return self.render_view_and_execute(cache, view, values, execute, fallback);
        }
        let signature = selected
            .map(|view| view.cache_signature(values))
            .unwrap_or_else(|| cache_signature(stencil, values));
        match self.render_or_get(cache, key, stencil, values) {
            Ok(address) => {
                // An entry is never handed to an executor while the backing
                // page is writable.  Once RX, this arena is intentionally
                // immutable; later regions use a fresh arena or the complete
                // ordinary fallback rather than violating W^X.
                if self.make_executable().is_err() {
                    cache.remove(key, signature, address);
                    return fallback();
                }
                match execute(address) {
                    Ok(value) => Ok(value),
                    Err(_) => {
                        // Do not leave a failed physical entry looking like a
                        // usable hit. The arena is already RX, so it cannot
                        // be safely repatched; removing the cache entry makes
                        // every later attempt take the complete fallback.
                        cache.remove(key, signature, address);
                        fallback()
                    }
                }
            }
            Err(_) => fallback(),
        }
    }

    fn render_view_and_execute<const N: usize, T, E>(
        &mut self,
        cache: &mut RenderedRegionCache,
        view: crate::stencil_select::PhysicalStencilView,
        values: &PatchValues<'_, N>,
        execute: impl FnOnce(usize) -> Result<T, E>,
        fallback: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E> {
        let signature = view.cache_signature(values);
        let Ok(address) = self.render_selected_view(cache, view, values) else {
            return fallback();
        };
        if self.make_executable().is_err() {
            cache.remove(view.key, signature, address);
            return fallback();
        }
        match execute(address) {
            Ok(value) => Ok(value),
            Err(_) => {
                cache.remove(view.key, signature, address);
                fallback()
            }
        }
    }

    /// Complete admitted-region path: one catalog lookup, then bounded
    /// allocation/copy/patch/protection and a caller-supplied entry point.
    /// Unknown regions never reach the arena and use ordinary semantics.
    pub fn render_selected_or_fallback<const N: usize, T, E>(
        &mut self,
        cache: &mut RenderedRegionCache,
        key: crate::stencil_fact::RegionKey,
        values: &PatchValues<'_, N>,
        execute: impl FnOnce(usize) -> Result<T, E>,
        fallback: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E> {
        let Some(view) = crate::stencil_select::select_physical(key) else {
            return fallback();
        };
        let contract = view.contract();
        if !contract.executable || contract.abi != crate::stencil_select::RegionAbi::ScalarF64Binary
        {
            return fallback();
        }
        // This generic adapter accepts a caller closure and is also used by
        // modeled ABI tests. Only typed machine-entry wrappers may publish an
        // execution witness, so a successful closure cannot masquerade as
        // native instruction execution in diagnostics.
        self.render_view_and_execute(cache, view, values, execute, fallback)
    }

    /// End-to-end executable entry for the proven-number Add+Return region.
    /// The fallback closure remains the semantic owner if selection,
    /// protection, address validation, or execution fails.
    pub fn render_selected_f64<const N: usize>(
        &mut self,
        cache: &mut RenderedRegionCache,
        key: crate::stencil_fact::RegionKey,
        values: &PatchValues<'_, N>,
        lhs: f64,
        rhs: f64,
        fallback: impl FnOnce() -> Result<f64, ArenaError>,
    ) -> Result<f64, ArenaError> {
        let Some(view) = crate::stencil_select::select_physical(key) else {
            return fallback();
        };
        if !view.contract().executable {
            return fallback();
        }
        let address = match self.render_physical_view_or_get(cache, view, values) {
            Ok(address) => address,
            Err(_) => return fallback(),
        };
        let signature = view.cache_signature(values);
        if self.make_executable().is_err() {
            cache.remove(key, signature, address);
            return fallback();
        }
        let entry_rhs = if key == crate::stencil_select::add_const_region_key() {
            values.constant_bits().map(f64::from_bits).unwrap_or(rhs)
        } else {
            rhs
        };
        match self.execute_f64(address, lhs, entry_rhs) {
            Ok(value) => {
                self.mark_physical_execution(view);
                Ok(value)
            }
            Err(_) => {
                cache.remove(key, signature, address);
                fallback()
            }
        }
    }

    pub fn render_selected_bool<const N: usize>(
        &mut self,
        cache: &mut RenderedRegionCache,
        key: crate::stencil_fact::RegionKey,
        values: &PatchValues<'_, N>,
        lhs: f64,
        rhs: f64,
    ) -> Result<bool, ArenaError> {
        let Some(view) = crate::stencil_select::select_physical(key).filter(|view| view.executable)
        else {
            return Err(ArenaError::ProtectionFailed);
        };
        if view.contract().abi != crate::stencil_select::RegionAbi::ScalarBool
            || view.fallthrough.is_some()
        {
            return Err(ArenaError::ProtectionFailed);
        }
        let stencil = view.stencil;
        let address = self.render_physical_view_or_get(cache, view, values)?;
        self.make_executable()?;
        match self.execute_bool(address, lhs, rhs) {
            Ok(value) => {
                self.mark_physical_execution(view);
                Ok(value)
            }
            Err(error) => {
                cache.remove(key, view.cache_signature(values), address);
                Err(error)
            }
        }
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub fn render_selected_i32<const N: usize>(
        &mut self,
        cache: &mut RenderedRegionCache,
        key: crate::stencil_fact::RegionKey,
        values: &PatchValues<'_, N>,
        lhs: i32,
        rhs: i32,
    ) -> Result<i32, ArenaError> {
        let Some(view) = crate::stencil_select::select_physical(key).filter(|view| view.executable)
        else {
            return Err(ArenaError::ProtectionFailed);
        };
        if view.contract().abi != crate::stencil_select::RegionAbi::ScalarI32
            || view.fallthrough.is_some()
        {
            return Err(ArenaError::ProtectionFailed);
        }
        let stencil = view.stencil;
        let address = self.render_physical_view_or_get(cache, view, values)?;
        self.make_executable()?;
        match self.execute_i32(address, lhs, rhs) {
            Ok(value) => {
                self.mark_physical_execution(view);
                Ok(value)
            }
            Err(error) => {
                cache.remove(key, view.cache_signature(values), address);
                Err(error)
            }
        }
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub fn render_selected_u32<const N: usize>(
        &mut self,
        cache: &mut RenderedRegionCache,
        key: crate::stencil_fact::RegionKey,
        values: &PatchValues<'_, N>,
        lhs: u32,
        rhs: u32,
    ) -> Result<u32, ArenaError> {
        let Some(view) = crate::stencil_select::select_physical(key).filter(|view| view.executable)
        else {
            return Err(ArenaError::ProtectionFailed);
        };
        if view.contract().abi != crate::stencil_select::RegionAbi::ScalarU32
            || view.fallthrough.is_some()
        {
            return Err(ArenaError::ProtectionFailed);
        }
        let stencil = view.stencil;
        let address = self.render_physical_view_or_get(cache, view, values)?;
        self.make_executable()?;
        match self.execute_u32(address, lhs, rhs) {
            Ok(value) => {
                self.mark_physical_execution(view);
                Ok(value)
            }
            Err(error) => {
                cache.remove(key, view.cache_signature(values), address);
                Err(error)
            }
        }
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    pub fn render_selected_i32<const N: usize>(
        &mut self,
        _cache: &mut RenderedRegionCache,
        _key: crate::stencil_fact::RegionKey,
        _values: &PatchValues<'_, N>,
        _lhs: i32,
        _rhs: i32,
    ) -> Result<i32, ArenaError> {
        Err(ArenaError::ProtectionFailed)
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    pub fn render_selected_u32<const N: usize>(
        &mut self,
        _cache: &mut RenderedRegionCache,
        _key: crate::stencil_fact::RegionKey,
        _values: &PatchValues<'_, N>,
        _lhs: u32,
        _rhs: u32,
    ) -> Result<u32, ArenaError> {
        Err(ArenaError::ProtectionFailed)
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn render_selected_f64x3<const N: usize>(
        &mut self,
        cache: &mut RenderedRegionCache,
        key: crate::stencil_fact::RegionKey,
        values: &PatchValues<'_, N>,
        lhs: f64,
        rhs: f64,
        third: f64,
    ) -> Result<f64, ArenaError> {
        let Some(view) = crate::stencil_select::select_physical(key).filter(|view| view.executable)
        else {
            return Err(ArenaError::ProtectionFailed);
        };
        if view.contract().abi != crate::stencil_select::RegionAbi::ScalarF64x3
            || view.record.operations != &[crate::ir::Opcode::Add, crate::ir::Opcode::Add]
        {
            return Err(ArenaError::ProtectionFailed);
        }
        let address = self.render_physical_view_or_get(cache, view, values)?;
        self.make_executable()?;
        let entry = self.f64x3_entry(address)?;
        let value = entry(lhs, rhs, third);
        self.mark_physical_execution(view);
        Ok(value)
    }

    fn cached_executable(
        &self,
        cache: &mut RenderedRegionCache,
        view: crate::stencil_select::PhysicalStencilView,
        signature: u64,
    ) -> Option<usize> {
        let address = cache
            .get_owned(view.key, signature, self.id)
            .filter(|address| self.owns_address(*address))?;
        let byte_len =
            view.stencil.bytes.len() + view.fallthrough.map_or(0, |tail| tail.stencil.bytes.len());
        let identity = RegionImageIdentity {
            key: view.key,
            cache_signature: signature,
            abi: view.abi,
        };
        if self.is_executable()
            && self
                .require_publication(address, identity, byte_len)
                .is_ok()
        {
            Some(address)
        } else {
            cache.remove(view.key, signature, address);
            None
        }
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn publish_region_image_or_get(
        &mut self,
        cache: &mut RenderedRegionCache,
        image: &VerifiedRegionImage,
    ) -> Result<usize, ArenaError> {
        let identity = image.identity();
        if let Some(address) = cache
            .get_owned(identity.key, identity.cache_signature, self.id)
            .filter(|address| self.owns_address(*address))
        {
            self.require_publication(address, identity, image.bytes().len())?;
            self.make_executable()?;
            return Ok(address);
        }
        self.publish_composed(cache, image)
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    fn publish_composed(
        &mut self,
        cache: &mut RenderedRegionCache,
        image: &VerifiedRegionImage,
    ) -> Result<usize, ArenaError> {
        let identity = image.identity();
        let bytes = image.bytes();
        let checkpoint = self.cursor;
        let offset = self.alloc_aligned(bytes.len(), STENCIL_ALIGNMENT)?;
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), self.ptr.add(offset), bytes.len()) };
        let address = self.address(offset).ok_or(ArenaError::Exhausted)?;
        if let Err(error) = self.record_publication(address, identity, bytes.len()) {
            self.cursor = checkpoint;
            return Err(error);
        }
        cache.insert_owned(identity.key, identity.cache_signature, address, self.id);
        if let Err(error) = self.make_executable() {
            cache.remove(identity.key, identity.cache_signature, address);
            self.published_entries.borrow_mut().remove(&address);
            self.cursor = checkpoint;
            return Err(error);
        }
        Ok(address)
    }

    /// Flip the entire arena from writable to executable once all regions have
    /// been copied and patched.  No caller receives an executable view before
    /// this succeeds.
    pub fn make_executable(&mut self) -> Result<(), ArenaError> {
        if self.executable {
            return Ok(());
        }
        // AArch64 has separate data/instruction caches. The bytes were copied
        // and patched through the RW mapping, so invalidate the published
        // range before the W^X transition makes it executable.
        flush_icache(self.ptr, self.cursor);
        let result = unsafe {
            libc::mprotect(
                self.ptr.cast(),
                self.capacity,
                libc::PROT_READ | libc::PROT_EXEC,
            )
        };
        if result != 0 {
            return Err(ArenaError::ProtectionFailed);
        }
        self.executable = true;
        Ok(())
    }

    #[cfg(test)]
    fn byte(&self, offset: usize) -> u8 {
        assert!(offset < self.cursor);
        unsafe { *self.ptr.add(offset) }
    }
}

impl Drop for StencilArena {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.ptr.cast(), self.capacity);
        }
    }
}

#[cfg(test)]
#[path = "stencil_arena_accounting_tests.rs"]
mod accounting_tests;

#[cfg(test)]
#[path = "stencil_arena_tests.rs"]
mod tests;
