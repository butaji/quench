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

mod arena_execution;
mod arena_mapping;
mod arena_render;
mod resource;
mod shared_pool;
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
        pub(crate) fn $name(
            &self,
            address: usize,
        ) -> Result<EntryToken<DispatchEntry>, ArenaError> {
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

mod shared_entries;

impl Drop for ActiveUse<'_> {
    fn drop(&mut self) {
        let active = self.owner.active_dispatches.get();
        self.owner.active_dispatches.set(active.saturating_sub(1));
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
