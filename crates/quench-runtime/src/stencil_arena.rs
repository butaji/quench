//! Bounded effectful code arena for rendered stencils.
//!
//! This is the only stencil module that owns OS memory mapping and `unsafe`.
//! It exposes fallible allocation/copy/patch operations and never executes a
//! partially rendered region.

use crate::stencil_fact::{PatchValues, Stencil};
use crate::stencil_patch::{apply_holes, PatchError};
use crate::stencil_select::{select_region, RenderedRegionCache};
use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

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

/// Global bound for the disposable physical region pool.  A plan may rotate
/// from an RX slab to a fresh RW slab, but never allocate an unbounded number
/// of executable mappings.
pub const MAX_SHARED_SLAB_BYTES: usize = 4 * MAX_ARENA_BYTES;
/// Workload-independent disposable code budget for one arena.
pub const MAX_ARENA_BYTES: usize = 1 << 20;

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
}

/// Bounded collection of immutable-after-publication executable slabs.  Region
/// plans share this owner rather than each allocating a 4 KiB mapping. A new
/// slab is created only when every existing slab is RX or exhausted; published
/// slabs remain alive for the pool lifetime, so cached entry addresses cannot
/// dangle during replacement.
pub struct SharedStencilSlab {
    slabs: Vec<StencilArena>,
    slab_capacity: usize,
    active_dispatches: Cell<usize>,
    peak_dispatches: Cell<usize>,
}

struct ActiveUse<'a> {
    owner: &'a SharedStencilSlab,
}

impl Drop for ActiveUse<'_> {
    fn drop(&mut self) {
        let active = self.owner.active_dispatches.get();
        self.owner.active_dispatches.set(active.saturating_sub(1));
    }
}

impl SharedStencilSlab {
    pub fn new(slab_capacity: usize) -> Result<Self, ArenaError> {
        if slab_capacity == 0 || slab_capacity > MAX_ARENA_BYTES {
            return Err(ArenaError::InvalidCapacity);
        }
        Ok(Self {
            slabs: Vec::new(),
            slab_capacity,
            active_dispatches: Cell::new(0),
            peak_dispatches: Cell::new(0),
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

    /// Drop published slabs only at an idle ownership boundary.  Callers keep
    /// cache entries, but each entry carries the slab generation, so a later
    /// lookup cannot treat an evicted address as callable.  Shared plans do
    /// not retain typed function pointers; they resolve them while borrowing
    /// the live owner immediately before execution.
    pub fn evict_idle(&mut self, retain: usize) -> usize {
        if self.active_dispatches.get() != 0 {
            return 0;
        }
        let remove = self.slabs.len().saturating_sub(retain);
        if remove == 0 {
            return 0;
        }
        self.slabs.drain(0..remove);
        remove
    }

    fn reclaim_for(&mut self, additional: usize) -> bool {
        if self.active_dispatches.get() != 0 {
            return false;
        }
        while self
            .total_capacity()
            .saturating_add(additional)
            > MAX_SHARED_SLAB_BYTES
            && self.slabs.len() > 1
        {
            self.slabs.remove(0);
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
        for slab in &mut self.slabs {
            match slab.render_or_get(cache, key, stencil, values) {
                Ok(address) => return Ok(address),
                Err(ArenaError::ProtectionFailed | ArenaError::Exhausted) => continue,
                Err(error) => return Err(error),
            }
        }
        if !self.reclaim_for(self.slab_capacity) {
            return Err(ArenaError::Exhausted);
        }
        let mut slab = StencilArena::new(self.slab_capacity)?;
        let address = slab.render_or_get(cache, key, stencil, values)?;
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

    pub fn execute_dispatch(
        &self,
        address: usize,
        context: *mut std::ffi::c_void,
    ) -> Result<u64, ArenaError> {
        let slab = self.slab_for(address).ok_or(ArenaError::ProtectionFailed)?;
        self.with_active(address, || slab.execute_dispatch(address, context))?
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

impl StencilArena {
    pub fn new(capacity: usize) -> Result<Self, ArenaError> {
        if capacity == 0 || capacity > MAX_ARENA_BYTES {
            return Err(ArenaError::InvalidCapacity);
        }
        let capacity = capacity
            .checked_add(PAGE - 1)
            .ok_or(ArenaError::InvalidCapacity)?
            & !(PAGE - 1);
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
        let base = self.ptr as usize;
        let end = base.saturating_add(self.cursor);
        if !self.executable || address < base || address >= end {
            return Err(ArenaError::ProtectionFailed);
        }
        let entry: extern "C" fn(f64, f64) -> f64 = unsafe { std::mem::transmute(address) };
        Ok(entry(lhs, rhs))
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub fn execute_bool(&self, address: usize, lhs: f64, rhs: f64) -> Result<bool, ArenaError> {
        let base = self.ptr as usize;
        let end = base.saturating_add(self.cursor);
        if !self.executable || address < base || address >= end {
            return Err(ArenaError::ProtectionFailed);
        }
        let entry: extern "C" fn(f64, f64) -> u64 = unsafe { std::mem::transmute(address) };
        Ok(entry(lhs, rhs) != 0)
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub fn execute_i32(&self, address: usize, lhs: i32, rhs: i32) -> Result<i32, ArenaError> {
        let base = self.ptr as usize;
        let end = base.saturating_add(self.cursor);
        if !self.executable || address < base || address >= end {
            return Err(ArenaError::ProtectionFailed);
        }
        let entry: extern "C" fn(i32, i32) -> i32 = unsafe { std::mem::transmute(address) };
        Ok(entry(lhs, rhs))
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub fn execute_u32(&self, address: usize, lhs: u32, rhs: u32) -> Result<u32, ArenaError> {
        let base = self.ptr as usize;
        let end = base.saturating_add(self.cursor);
        if !self.executable || address < base || address >= end {
            return Err(ArenaError::ProtectionFailed);
        }
        let entry: extern "C" fn(u32, u32) -> u32 = unsafe { std::mem::transmute(address) };
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
        let base = self.ptr as usize;
        let end = base.saturating_add(self.cursor);
        if !self.executable || address < base || address >= end {
            return Err(ArenaError::ProtectionFailed);
        }
        Ok(unsafe { std::mem::transmute(address) })
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(crate) fn u32_entry(
        &self,
        address: usize,
    ) -> Result<extern "C" fn(u32, u32) -> u32, ArenaError> {
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
        let base = self.ptr as usize;
        let end = base.saturating_add(self.cursor);
        if !self.executable || address < base || address >= end || word.is_null() {
            return Err(ArenaError::ProtectionFailed);
        }
        let entry: extern "C" fn(*const crate::tagged_value::TaggedValue) -> u64 =
            unsafe { std::mem::transmute(address) };
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
        let base = self.ptr as usize;
        let end = base.saturating_add(self.cursor);
        if !self.executable || address < base || address >= end || context.is_null() {
            return Err(ArenaError::ProtectionFailed);
        }
        let entry: extern "C" fn(*mut std::ffi::c_void) -> u64 =
            unsafe { std::mem::transmute(address) };
        Ok(entry(context))
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
        let offset = self.alloc(stencil.bytes.len())?;
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
        match self.render_or_get(cache, key, stencil, values) {
            Ok(address) => {
                // An entry is never handed to an executor while the backing
                // page is writable.  Once RX, this arena is intentionally
                // immutable; later regions use a fresh arena or the complete
                // ordinary fallback rather than violating W^X.
                if self.make_executable().is_err() {
                    cache.remove(key, cache_signature(stencil, values), address);
                    return fallback();
                }
                match execute(address) {
                    Ok(value) => Ok(value),
                    Err(_) => {
                        // Do not leave a failed physical entry looking like a
                        // usable hit. The arena is already RX, so it cannot
                        // be safely repatched; removing the cache entry makes
                        // every later attempt take the complete fallback.
                        cache.remove(key, cache_signature(stencil, values), address);
                        fallback()
                    }
                }
            }
            Err(_) => fallback(),
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
        let Some(record) = select_region(key) else {
            return fallback();
        };
        if !record.executable {
            return fallback();
        }
        self.render_and_execute(cache, key, &record.stencil, values, execute, fallback)
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
        let Some(record) = select_region(key) else {
            return fallback();
        };
        if !record.executable {
            return fallback();
        }
        if let Some((tail, rel32_offset)) = record.fallthrough {
            return self.render_fallthrough_f64(
                cache,
                key,
                &record.stencil,
                tail,
                values,
                rel32_offset,
                lhs,
                rhs,
                fallback,
            );
        }
        let address = match self.render_or_get(cache, key, &record.stencil, values) {
            Ok(address) => address,
            Err(_) => return fallback(),
        };
        if self.make_executable().is_err() {
            cache.remove(key, cache_signature(&record.stencil, values), address);
            return fallback();
        }
        self.execute_f64(address, lhs, rhs).or_else(|_| fallback())
    }

    pub fn render_selected_bool<const N: usize>(
        &mut self,
        cache: &mut RenderedRegionCache,
        key: crate::stencil_fact::RegionKey,
        values: &PatchValues<'_, N>,
        lhs: f64,
        rhs: f64,
    ) -> Result<bool, ArenaError> {
        let Some(record) = select_region(key).filter(|record| record.executable) else {
            return Err(ArenaError::ProtectionFailed);
        };
        if record.fallthrough.is_some() {
            return Err(ArenaError::ProtectionFailed);
        }
        let address = self.render_or_get(cache, key, &record.stencil, values)?;
        self.make_executable()?;
        self.execute_bool(address, lhs, rhs)
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
        let Some(record) = select_region(key).filter(|record| record.executable) else {
            return Err(ArenaError::ProtectionFailed);
        };
        if record.abi != crate::stencil_select::RegionAbi::ScalarI32
            || record.fallthrough.is_some()
        {
            return Err(ArenaError::ProtectionFailed);
        }
        let address = self.render_or_get(cache, key, &record.stencil, values)?;
        self.make_executable()?;
        self.execute_i32(address, lhs, rhs)
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
        let Some(record) = select_region(key).filter(|record| record.executable) else {
            return Err(ArenaError::ProtectionFailed);
        };
        if record.abi != crate::stencil_select::RegionAbi::ScalarU32
            || record.fallthrough.is_some()
        {
            return Err(ArenaError::ProtectionFailed);
        }
        let address = self.render_or_get(cache, key, &record.stencil, values)?;
        self.make_executable()?;
        self.execute_u32(address, lhs, rhs)
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
        let Some(record) = select_region(key).filter(|record| record.executable) else {
            return Err(ArenaError::ProtectionFailed);
        };
        // A fused chain is emitted as one complete stencil and therefore must
        // not recurse through the two-piece fallthrough renderer.
        if record.fallthrough.is_some() {
            return Err(ArenaError::ProtectionFailed);
        }
        let address = self.render_or_get(cache, key, &record.stencil, values)?;
        self.make_executable()?;
        let entry = self.f64x3_entry(address)?;
        Ok(entry(lhs, rhs, third))
    }

    /// Install a two-region Number chain. The head falls through by a direct
    /// `Rel32` jump to the already-installed tail, which returns the value in
    /// `xmm0`. The caller supplies the build-time displacement offset; this
    /// method performs no instruction selection or CFG analysis.
    #[cfg(target_arch = "x86_64")]
    pub fn render_fallthrough_f64<const N: usize>(
        &mut self,
        cache: &mut RenderedRegionCache,
        key: crate::stencil_fact::RegionKey,
        head: &Stencil,
        tail: &Stencil,
        values: &PatchValues<'_, N>,
        rel32_offset: u16,
        lhs: f64,
        rhs: f64,
        fallback: impl FnOnce() -> Result<f64, ArenaError>,
    ) -> Result<f64, ArenaError> {
        let signature = cache_signature(head, values);
        if let Some(address) = cache
            .get_owned(key, signature, self.id)
            .filter(|address| self.owns_address(*address))
        {
            if self.is_executable() {
                return self.execute_f64(address, lhs, rhs).or_else(|_| fallback());
            }
            // A prior protection failure must not make this address a
            // permanent cache hit. Remove it and retry the bounded render.
            cache.remove(key, signature, address);
        }
        if !head.validate() || !tail.validate() {
            return fallback();
        }
        if !head.holes.iter().any(|hole| {
            hole.offset == rel32_offset && matches!(hole.kind, crate::stencil_fact::HoleKind::Rel32)
        }) {
            return fallback();
        }
        let checkpoint = self.cursor;
        let tail_offset = match self.alloc(tail.bytes.len()) {
            Ok(offset) => offset,
            Err(_) => return fallback(),
        };
        let tail_result = unsafe {
            std::ptr::copy_nonoverlapping(
                tail.bytes.as_ptr(),
                self.ptr.add(tail_offset),
                tail.bytes.len(),
            );
            let dst = std::slice::from_raw_parts_mut(self.ptr.add(tail_offset), tail.bytes.len());
            apply_holes(dst, tail.holes, values)
        };
        if tail_result.is_err() {
            self.cursor = checkpoint;
            return fallback();
        }
        let tail_address = match self.address(tail_offset) {
            Some(address) => address,
            None => {
                self.cursor = checkpoint;
                return fallback();
            }
        };
        let head_offset = match self.alloc(head.bytes.len()) {
            Ok(offset) => offset,
            Err(_) => {
                self.cursor = checkpoint;
                return fallback();
            }
        };
        let next_instruction = self.ptr as usize + head_offset + usize::from(rel32_offset) + 4;
        let Some(patched_values) = values.with_relative_target(tail_address, next_instruction)
        else {
            self.cursor = checkpoint;
            return fallback();
        };
        let head_result = unsafe {
            std::ptr::copy_nonoverlapping(
                head.bytes.as_ptr(),
                self.ptr.add(head_offset),
                head.bytes.len(),
            );
            let dst = std::slice::from_raw_parts_mut(self.ptr.add(head_offset), head.bytes.len());
            apply_holes(dst, head.holes, &patched_values)
        };
        if head_result.is_err() {
            self.cursor = checkpoint;
            return fallback();
        }
        let address = match self.address(head_offset) {
            Some(address) => address,
            None => {
                self.cursor = checkpoint;
                return fallback();
            }
        };
        // The displacement is internal to this arena and remains valid while
        // the cached address is owned by it. Match future calls on the
        // caller-visible patch facts, not on the newly allocated addresses.
        cache.insert_owned(key, signature, address, self.id);
        if self.make_executable().is_err() {
            cache.remove(key, signature, address);
            return fallback();
        }
        self.execute_f64(address, lhs, rhs).or_else(|_| fallback())
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    pub fn render_fallthrough_f64<const N: usize>(
        &mut self,
        _cache: &mut RenderedRegionCache,
        _key: crate::stencil_fact::RegionKey,
        _head: &Stencil,
        _tail: &Stencil,
        _values: &PatchValues<'_, N>,
        _rel32_offset: u16,
        _lhs: f64,
        _rhs: f64,
        fallback: impl FnOnce() -> Result<f64, ArenaError>,
    ) -> Result<f64, ArenaError> {
        fallback()
    }

    /// Compose the AArch64 head and tail in one arena. The head carries a
    /// `B`/imm26 relocation to the tail, so the chain has one entry and one
    /// return at the boundary; no Rust callback occurs between them.
    #[cfg(target_arch = "aarch64")]
    pub fn render_fallthrough_f64<const N: usize>(
        &mut self,
        cache: &mut RenderedRegionCache,
        key: crate::stencil_fact::RegionKey,
        head: &Stencil,
        tail: &Stencil,
        values: &PatchValues<'_, N>,
        branch_offset: u16,
        lhs: f64,
        rhs: f64,
        fallback: impl FnOnce() -> Result<f64, ArenaError>,
    ) -> Result<f64, ArenaError> {
        let signature = cache_signature(head, values);
        if let Some(address) = cache
            .get_owned(key, signature, self.id)
            .filter(|address| self.owns_address(*address))
        {
            if self.is_executable() {
                return self.execute_f64(address, lhs, rhs).or_else(|_| fallback());
            }
            cache.remove(key, signature, address);
        }
        if !head.validate() || !tail.validate() {
            return fallback();
        }
        if !head.holes.iter().any(|hole| {
            hole.offset == branch_offset
                && matches!(hole.kind, crate::stencil_fact::HoleKind::Branch26)
        }) {
            return fallback();
        }
        let checkpoint = self.cursor;
        let tail_offset = match self.alloc_aligned(tail.bytes.len(), 4) {
            Ok(offset) => offset,
            Err(_) => return fallback(),
        };
        let tail_result = unsafe {
            std::ptr::copy_nonoverlapping(
                tail.bytes.as_ptr(),
                self.ptr.add(tail_offset),
                tail.bytes.len(),
            );
            let dst = std::slice::from_raw_parts_mut(self.ptr.add(tail_offset), tail.bytes.len());
            apply_holes(dst, tail.holes, values)
        };
        if tail_result.is_err() {
            self.cursor = checkpoint;
            return fallback();
        }
        let tail_address = match self.address(tail_offset) {
            Some(address) => address,
            None => {
                self.cursor = checkpoint;
                return fallback();
            }
        };
        let head_offset = match self.alloc_aligned(head.bytes.len(), 4) {
            Ok(offset) => offset,
            Err(_) => {
                self.cursor = checkpoint;
                return fallback();
            }
        };
        // AArch64 `B` measures its displacement from the branch instruction
        // itself (unlike x86 rel32, which is relative to the following
        // instruction).
        let branch_address = self.ptr as usize + head_offset + usize::from(branch_offset);
        let Some(patched_values) = values.with_relative_target(tail_address, branch_address) else {
            self.cursor = checkpoint;
            return fallback();
        };
        let head_result = unsafe {
            std::ptr::copy_nonoverlapping(
                head.bytes.as_ptr(),
                self.ptr.add(head_offset),
                head.bytes.len(),
            );
            let dst = std::slice::from_raw_parts_mut(self.ptr.add(head_offset), head.bytes.len());
            apply_holes(dst, head.holes, &patched_values)
        };
        if head_result.is_err() {
            self.cursor = checkpoint;
            return fallback();
        }
        let address = match self.address(head_offset) {
            Some(address) => address,
            None => {
                self.cursor = checkpoint;
                return fallback();
            }
        };
        cache.insert_owned(key, signature, address, self.id);
        if self.make_executable().is_err() {
            cache.remove(key, signature, address);
            return fallback();
        }
        self.execute_f64(address, lhs, rhs).or_else(|_| fallback())
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
mod tests {
    use super::*;
    use crate::ir::Opcode;
    use crate::quickening::QuickeningSite;
    use crate::stencil_fact::{Hole, HoleKind, PatchValues, Stencil};

    #[test]
    fn arena_enforces_a_bounded_mapping() {
        let mut arena = StencilArena::new(4096).unwrap();
        assert!(matches!(
            StencilArena::new(MAX_ARENA_BYTES + 1),
            Err(ArenaError::InvalidCapacity)
        ));
        assert_eq!(arena.capacity(), 4096);
        assert_eq!(arena.alloc(4097), Err(ArenaError::Exhausted));
        assert_eq!(arena.alloc(16), Ok(0));
        assert_eq!(arena.used(), 16);
    }

    #[test]
    fn copy_patch_is_atomic_from_the_callers_view() {
        let mut arena = StencilArena::new(4096).unwrap();
        let site = QuickeningSite::<2>::new(Opcode::GetProperty);
        let values = PatchValues::from_site(&site);
        let stencil = Stencil {
            bytes: &[1, 2, 3, 4],
            holes: &[Hole {
                offset: 0,
                kind: HoleKind::Imm32,
            }],
        };
        let offset = arena.copy_and_patch(&stencil, &values).unwrap();
        assert_eq!(offset, 0);
        assert_eq!(arena.byte(0), 0);
    }

    #[test]
    fn render_cache_hit_does_not_allocate_again() {
        let mut arena = StencilArena::new(4096).unwrap();
        let mut cache = crate::stencil_select::RenderedRegionCache::new();
        let site = QuickeningSite::<2>::new(Opcode::GetProperty);
        let values = PatchValues::from_site(&site);
        let stencil = Stencil {
            bytes: &[1, 2, 3],
            holes: &[],
        };
        let key = crate::stencil_fact::RegionKey(7);
        let first = arena
            .render_or_get(&mut cache, key, &stencil, &values)
            .unwrap();
        let used = arena.used();
        let second = arena
            .render_or_get(&mut cache, key, &stencil, &values)
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(arena.used(), used);
    }

    #[test]
    fn rendered_cache_reuses_unpatchable_quickening_state() {
        let mut arena = StencilArena::new(4096).unwrap();
        let mut cache = RenderedRegionCache::new();
        let mut first_site = QuickeningSite::<2>::new(Opcode::GetProperty);
        let second_site = QuickeningSite::<2>::new(Opcode::GetProperty);
        let shape = crate::shape_cache::ShapeId(1);
        let property = crate::shape_cache::PropertyId(2);
        assert!(matches!(
            first_site.observe(shape, property, 7),
            crate::quickening::QuickeningDecision::InstallGuard { .. }
        ));
        let first_values = PatchValues::from_site(&first_site);
        let second_values = PatchValues::from_site(&second_site);
        let stencil = Stencil {
            bytes: &[1, 2, 3],
            holes: &[],
        };
        let key = crate::stencil_fact::RegionKey(12);
        let first = arena
            .render_or_get(&mut cache, key, &stencil, &first_values)
            .unwrap();
        let used = arena.used();
        let second = arena
            .render_or_get(&mut cache, key, &stencil, &second_values)
            .unwrap();
        assert_ne!(first_values.signature(), second_values.signature());
        assert_eq!(first, second);
        assert_eq!(arena.used(), used);
    }

    #[test]
    fn cache_entries_from_another_arena_are_not_executed() {
        let mut arena = StencilArena::new(4096).unwrap();
        let mut cache = RenderedRegionCache::new();
        let site = QuickeningSite::<2>::new(Opcode::GetProperty);
        let values = PatchValues::from_site(&site);
        let stencil = Stencil {
            bytes: &[1, 2, 3],
            holes: &[],
        };
        let key = crate::stencil_fact::RegionKey(9);
        cache.insert(key, values.signature(), usize::MAX);
        let address = arena
            .render_or_get(&mut cache, key, &stencil, &values)
            .unwrap();
        assert_ne!(address, usize::MAX);
        assert_eq!(arena.used(), 3);
    }

    #[test]
    fn rendered_entries_are_owned_by_their_arena_generation() {
        let mut first = StencilArena::new(4096).unwrap();
        let mut cache = RenderedRegionCache::new();
        let site = QuickeningSite::<2>::new(Opcode::GetProperty);
        let values = PatchValues::from_site(&site);
        let stencil = Stencil {
            bytes: &[1, 2, 3],
            holes: &[],
        };
        let key = crate::stencil_fact::RegionKey(91);
        let first_address = first
            .render_or_get(&mut cache, key, &stencil, &values)
            .unwrap();
        let first_owner = first.id();
        assert_eq!(cache.get_owned(key, 0, first_owner), Some(first_address));

        // A cache may outlive a disposable arena.  A new owner must not treat
        // the old raw address as callable, even if the OS later recycles the
        // same virtual mapping.
        drop(first);
        let mut second = StencilArena::new(4096).unwrap();
        assert_ne!(first_owner, second.id());
        assert_eq!(cache.get_owned(key, 0, second.id()), None);
        let second_address = second
            .render_or_get(&mut cache, key, &stencil, &values)
            .unwrap();
        assert_eq!(second.used(), stencil.bytes.len());
        assert_eq!(cache.get_owned(key, 0, second.id()), Some(second_address));
    }

    #[test]
    fn shared_slab_rotates_only_after_publication_and_stays_bounded() {
        let mut pool = SharedStencilSlab::new(4096).unwrap();
        let mut cache = RenderedRegionCache::new();
        let site = QuickeningSite::<2>::new(Opcode::GetProperty);
        let values = PatchValues::from_site(&site);
        static BYTES: [u8; 4096] = [0; 4096];
        let stencil = Stencil {
            bytes: &BYTES,
            holes: &[],
        };
        let first = pool
            .render_or_get(
                &mut cache,
                crate::stencil_fact::RegionKey(101),
                &stencil,
                &values,
            )
            .unwrap();
        pool.make_executable(first).unwrap();
        let second = pool
            .render_or_get(
                &mut cache,
                crate::stencil_fact::RegionKey(102),
                &stencil,
                &values,
            )
            .unwrap();
        assert_ne!(first, second);
        assert_eq!(pool.slab_count(), 2);
        assert_eq!(pool.capacity(), 8192);
        assert!(pool.capacity() <= MAX_SHARED_SLAB_BYTES);
    }

    #[test]
    fn shared_slab_rejects_an_oversized_render_without_publishing() {
        let mut pool = SharedStencilSlab::new(4096).unwrap();
        let mut cache = RenderedRegionCache::new();
        let site = QuickeningSite::<2>::new(Opcode::GetProperty);
        let values = PatchValues::from_site(&site);
        static TOO_LARGE: [u8; 8192] = [0; 8192];
        let stencil = Stencil {
            bytes: &TOO_LARGE,
            holes: &[],
        };
        assert_eq!(
            pool.render_or_get(
                &mut cache,
                crate::stencil_fact::RegionKey(103),
                &stencil,
                &values
            ),
            Err(ArenaError::Exhausted)
        );
        assert_eq!(pool.slab_count(), 0);
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn shared_slab_evicts_only_idle_generations_without_reusing_cache_addresses() {
        let mut pool = SharedStencilSlab::new(4096).unwrap();
        let mut cache = RenderedRegionCache::new();
        let site = QuickeningSite::<2>::new(Opcode::GetProperty);
        let values = PatchValues::from_site(&site);
        static BYTES: [u8; 4096] = [0; 4096];
        let stencil = Stencil {
            bytes: &BYTES,
            holes: &[],
        };
        let first_key = crate::stencil_fact::RegionKey(104);
        let second_key = crate::stencil_fact::RegionKey(105);
        let first = pool
            .render_or_get(&mut cache, first_key, &stencil, &values)
            .unwrap();
        let first_owner = pool.owner_for(first).unwrap();
        pool.make_executable(first).unwrap();
        pool.render_or_get(&mut cache, second_key, &stencil, &values)
            .unwrap();
        pool.active_dispatches.set(1);
        assert_eq!(pool.evict_idle(1), 0);
        pool.active_dispatches.set(0);
        assert_eq!(pool.evict_idle(1), 1);
        assert_eq!(pool.slab_count(), 1);
        assert_eq!(pool.evict_idle(1), 0);
        let replacement = pool
            .render_or_get(&mut cache, first_key, &stencil, &values)
            .unwrap();
        assert_ne!(pool.owner_for(replacement), Some(first_owner));
    }

    #[test]
    fn shared_slab_typed_entry_guard_blocks_eviction_during_call() {
        let mut pool = SharedStencilSlab::new(4096).unwrap();
        let mut cache = RenderedRegionCache::new();
        let site = QuickeningSite::<2>::new(Opcode::Add);
        let values = PatchValues::from_site(&site);
        static BYTES: [u8; 4] = [0; 4];
        let address = pool
            .render_or_get(
                &mut cache,
                crate::stencil_fact::RegionKey(106),
                &Stencil {
                    bytes: &BYTES,
                    holes: &[],
                },
                &values,
            )
            .unwrap();
        assert_eq!(pool.active_dispatches(), 0);
        let observed = pool
            .with_active(address, || pool.active_dispatches())
            .unwrap();
        assert_eq!(observed, 1);
        assert_eq!(pool.active_dispatches(), 0);
        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = pool.with_active(address, || panic!("simulated helper unwind"));
        }));
        assert!(unwind.is_err());
        assert_eq!(pool.active_dispatches(), 0);
        assert_eq!(pool.evict_idle(0), 1);
    }

    #[test]
    fn shared_slab_reclaims_oldest_idle_generation_at_global_cap() {
        let mut pool = SharedStencilSlab::new(MAX_ARENA_BYTES).unwrap();
        let mut cache = RenderedRegionCache::new();
        let site = QuickeningSite::<2>::new(Opcode::GetProperty);
        let values = PatchValues::from_site(&site);
        static BYTES: [u8; MAX_ARENA_BYTES] = [0; MAX_ARENA_BYTES];
        let stencil = Stencil {
            bytes: &BYTES,
            holes: &[],
        };
        for raw_key in 0..=MAX_SHARED_SLAB_BYTES / MAX_ARENA_BYTES {
            let key = crate::stencil_fact::RegionKey(200 + raw_key as u64);
            if raw_key == MAX_SHARED_SLAB_BYTES / MAX_ARENA_BYTES {
                pool.active_dispatches.set(1);
                assert_eq!(
                    pool.render_or_get(&mut cache, key, &stencil, &values),
                    Err(ArenaError::Exhausted)
                );
                pool.active_dispatches.set(0);
            }
            let address = pool.render_or_get(&mut cache, key, &stencil, &values).unwrap();
            pool.make_executable(address).unwrap();
        }
        assert_eq!(pool.capacity(), MAX_SHARED_SLAB_BYTES);
        assert_eq!(pool.slab_count(), 4);
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn shared_slab_accounts_active_native_execution() {
        let key = crate::stencil_select::array_numeric_loop_region_key();
        let record = crate::stencil_select::select_region(key).expect("array loop row");
        let site = QuickeningSite::<2>::new(Opcode::LoadLocal);
        let values = PatchValues::from_site(&site);
        let mut pool = SharedStencilSlab::new(4096).unwrap();
        let mut cache = RenderedRegionCache::new();
        let address = pool
            .render_or_get(&mut cache, key, &record.stencil, &values)
            .unwrap();
        pool.make_executable(address).unwrap();
        let mut data = vec![2.0, 3.0];
        let interrupt = std::sync::atomic::AtomicBool::new(false);
        let mut raw = crate::vm::NativeArrayLoopContext {
            data: data.as_mut_ptr(),
            len: data.len(),
            index: 0,
            end: data.len(),
            addend: 1.0,
            result: 0.0,
            interrupt: &interrupt,
        };
        assert_eq!(pool.active_dispatches(), 0);
        assert_eq!(
            pool.execute_dispatch(
                address,
                (&mut raw as *mut crate::vm::NativeArrayLoopContext).cast()
            )
            .unwrap(),
            1
        );
        assert_eq!(pool.active_dispatches(), 0);
        assert_eq!(pool.peak_dispatches(), 1);
        assert_eq!(data, vec![3.0, 4.0]);
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    #[test]
    fn executable_add_region_matches_ordinary_number_semantics() {
        let mut arena = StencilArena::new(4096).unwrap();
        let mut cache = RenderedRegionCache::new();
        let site = QuickeningSite::<2>::new(Opcode::Add);
        let values = PatchValues::from_site(&site);
        let key = crate::stencil_select::fallthrough_region_key();
        assert_eq!(
            arena.render_selected_f64(&mut cache, key, &values, 20.5, 22.25, || Ok(7.0)),
            Ok(42.75)
        );
        assert_eq!(20.5_f64 + 22.25_f64, 42.75);

        let negative_zero = arena
            .render_selected_f64(&mut cache, key, &values, -0.0, -0.0, || Ok(1.0))
            .unwrap();
        assert_eq!(negative_zero.to_bits(), (-0.0_f64).to_bits());

        let nan = arena
            .render_selected_f64(&mut cache, key, &values, f64::NAN, 1.0, || Ok(1.0))
            .unwrap();
        assert!(nan.is_nan());
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    #[test]
    fn executable_equality_region_matches_numeric_semantics() {
        let key = crate::stencil_select::compare_equal_region_key();
        let record = crate::stencil_select::select_region(key).expect("equality declaration");
        let site = QuickeningSite::<2>::new(Opcode::Binary);
        let values = PatchValues::from_site(&site);
        let mut arena = StencilArena::new(4096).unwrap();
        let mut cache = RenderedRegionCache::new();
        let address = arena
            .render_or_get(&mut cache, key, &record.stencil, &values)
            .unwrap();
        arena.make_executable().unwrap();
        assert!(arena.execute_bool(address, 4.0, 4.0).unwrap());
        assert!(!arena.execute_bool(address, 4.0, 5.0).unwrap());
        assert!(!arena.execute_bool(address, f64::NAN, f64::NAN).unwrap());
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    #[test]
    fn executable_ordered_regions_reject_unordered_nan() {
        let site = QuickeningSite::<2>::new(Opcode::Binary);
        let values = PatchValues::from_site(&site);
        let cases = [
            (crate::stencil_select::compare_less_region_key(), 1.0, 2.0),
            (crate::stencil_select::compare_less_equal_region_key(), 2.0, 2.0),
            (crate::stencil_select::compare_greater_region_key(), 2.0, 1.0),
            (crate::stencil_select::compare_greater_equal_region_key(), 2.0, 2.0),
        ];
        for (key, lhs, rhs) in cases {
            let record = crate::stencil_select::select_region(key)
                .expect("comparison declaration");
            let mut arena = StencilArena::new(4096).unwrap();
            let mut cache = RenderedRegionCache::new();
            let address = arena
                .render_or_get(&mut cache, key, &record.stencil, &values)
                .unwrap();
            arena.make_executable().unwrap();
            assert!(arena.execute_bool(address, lhs, rhs).unwrap());
            assert!(!arena.execute_bool(address, f64::NAN, rhs).unwrap());
        }
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    #[test]
    fn executable_i32_bitwise_regions_match_signed_results() {
        let site = QuickeningSite::<2>::new(Opcode::Binary);
        let values = PatchValues::from_site(&site);
        let cases = [
            (crate::stencil_select::bitwise_and_region_key(), 0xF0F0_i32, 0x0FF0_i32),
            (crate::stencil_select::bitwise_or_region_key(), 0xF000_i32, 0x00F0_i32),
            (crate::stencil_select::bitwise_xor_region_key(), -1_i32, 0x0F0F_i32),
        ];
        for (key, lhs, rhs) in cases {
            let record = crate::stencil_select::select_region(key).expect("bitwise declaration");
            let mut arena = StencilArena::new(4096).unwrap();
            let mut cache = RenderedRegionCache::new();
            let address = arena
                .render_or_get(&mut cache, key, &record.stencil, &values)
                .unwrap();
            arena.make_executable().unwrap();
            let expected = match key {
                key if key == crate::stencil_select::bitwise_and_region_key() => lhs & rhs,
                key if key == crate::stencil_select::bitwise_or_region_key() => lhs | rhs,
                _ => lhs ^ rhs,
            };
            assert_eq!(arena.execute_i32(address, lhs, rhs).unwrap(), expected);
        }
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    #[test]
    fn executable_shift_regions_mask_counts_and_preserve_unsigned_result() {
        let site = QuickeningSite::<2>::new(Opcode::Binary);
        let values = PatchValues::from_site(&site);
        for (key, lhs, rhs, expected) in [
            (crate::stencil_select::shift_left_region_key(), 1_i32, 32_i32, 1_i32),
            (crate::stencil_select::shift_right_region_key(), -8_i32, 1_i32, -4_i32),
        ] {
            let record = crate::stencil_select::select_region(key).expect("shift declaration");
            let mut arena = StencilArena::new(4096).unwrap();
            let mut cache = RenderedRegionCache::new();
            let address = arena
                .render_or_get(&mut cache, key, &record.stencil, &values)
                .unwrap();
            arena.make_executable().unwrap();
            assert_eq!(arena.execute_i32(address, lhs, rhs).unwrap(), expected);
        }
        let key = crate::stencil_select::shift_right_zero_region_key();
        let record = crate::stencil_select::select_region(key).expect("unsigned shift declaration");
        let mut arena = StencilArena::new(4096).unwrap();
        let mut cache = RenderedRegionCache::new();
        let address = arena
            .render_or_get(&mut cache, key, &record.stencil, &values)
            .unwrap();
        arena.make_executable().unwrap();
        assert_eq!(arena.execute_u32(address, u32::MAX, 1), Ok(2_147_483_647));
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    #[test]
    fn executable_add_const_region_uses_patched_constant_data() {
        let mut arena = StencilArena::new(4096).unwrap();
        let mut cache = RenderedRegionCache::new();
        let site = QuickeningSite::<2>::new(Opcode::AddConst);
        let values = PatchValues::from_site(&site).with_constant_bits(2.5_f64.to_bits());
        let key = crate::stencil_select::add_const_region_key();
        let result = arena.render_selected_f64(&mut cache, key, &values, 4.0, 0.0, || Ok(99.0));
        assert_eq!(result, Ok(6.5));
        assert_eq!(
            arena.used(),
            if cfg!(target_arch = "aarch64") {
                24
            } else {
                21
            }
        );
        assert_eq!(
            arena.byte(if cfg!(target_arch = "aarch64") {
                16
            } else {
                13
            }),
            0
        );
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    #[test]
    fn executable_property_leaf_matches_raw_tagged_word_load() {
        let mut arena = StencilArena::new(4096).unwrap();
        let mut cache = RenderedRegionCache::new();
        let site = QuickeningSite::<2>::new(Opcode::GetN);
        let values = PatchValues::from_site(&site);
        let key = crate::stencil_select::property_region_key();
        let record = crate::stencil_select::select_region(key).expect("property row");
        let address = arena
            .render_or_get(&mut cache, key, &record.stencil, &values)
            .unwrap();
        arena.make_executable().unwrap();
        let word = crate::tagged_value::TaggedValue::from_bits(0x1234_5678_9ABC_DEF0);
        assert_eq!(arena.execute_tagged_word(address, &word), Ok(word.bits()));
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn installed_region_falls_through_to_next_region() {
        let mut arena = StencilArena::new(4096).unwrap();
        let mut cache = RenderedRegionCache::new();
        let site = QuickeningSite::<2>::new(Opcode::Add);
        let values = PatchValues::from_site(&site);
        let head = Stencil {
            bytes: &[0xF2, 0x0F, 0x58, 0xC1, 0xE9, 0, 0, 0, 0],
            holes: &[Hole {
                offset: 5,
                kind: HoleKind::Rel32,
            }],
        };
        let tail = Stencil {
            bytes: &[0xC3],
            holes: &[],
        };
        assert_eq!(
            arena.render_fallthrough_f64(
                &mut cache,
                crate::stencil_fact::RegionKey(44),
                &head,
                &tail,
                &values,
                5,
                10.25,
                2.5,
                || Ok(0.0),
            ),
            Ok(12.75)
        );
        assert_eq!(arena.used(), head.bytes.len() + tail.bytes.len());
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    #[test]
    fn generated_fallthrough_region_is_selected_by_canonical_key() {
        let mut arena = StencilArena::new(4096).unwrap();
        let mut cache = RenderedRegionCache::new();
        let site = QuickeningSite::<2>::new(Opcode::Add);
        let values = PatchValues::from_site(&site);
        let key = crate::stencil_select::fallthrough_region_key();
        assert_eq!(
            arena.render_selected_f64(&mut cache, key, &values, 1.25, 2.75, || Ok(0.0)),
            Ok(4.0)
        );
        let used = arena.used();
        assert_eq!(
            arena.render_selected_f64(&mut cache, key, &values, 3.5, 4.5, || Ok(0.0)),
            Ok(8.0)
        );
        assert_eq!(arena.used(), used);
        assert_eq!(cache.len(), 1);
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    #[test]
    fn generated_dispatch_entry_calls_the_patched_bridge() {
        extern "C" fn probe(context: *mut std::ffi::c_void) -> u64 {
            assert!(!context.is_null());
            0xD15A_7C1u64
        }
        let mut arena = StencilArena::new(4096).unwrap();
        let mut cache = RenderedRegionCache::new();
        let site = QuickeningSite::<2>::new(Opcode::Move);
        let values = PatchValues::from_site(&site).with_pointer_bits(probe as *const () as usize);
        let key = crate::stencil_select::dispatch_region_key();
        let record = crate::stencil_select::select_region(key).expect("dispatch catalog row");
        let address = arena
            .render_or_get(&mut cache, key, &record.stencil, &values)
            .unwrap();
        arena.make_executable().unwrap();
        let mut marker = 0u8;
        let status = arena
            .execute_dispatch(address, (&mut marker as *mut u8).cast())
            .unwrap();
        assert_eq!(status, 0xD15A_7C1u64);
    }

    #[test]
    fn render_failure_uses_complete_fallback() {
        let mut arena = StencilArena::new(4096).unwrap();
        let mut cache = crate::stencil_select::RenderedRegionCache::new();
        let site = QuickeningSite::<2>::new(Opcode::GetProperty);
        let values = PatchValues::from_site(&site);
        let stencil = Stencil {
            bytes: &[1],
            holes: &[Hole {
                offset: 0,
                kind: HoleKind::Ptr64,
            }],
        };
        let result = arena.render_and_execute(
            &mut cache,
            crate::stencil_fact::RegionKey(8),
            &stencil,
            &values,
            |_| Ok::<_, ()>(99),
            || Ok::<_, ()>(7),
        );
        assert_eq!(result, Ok(7));
        assert_eq!(arena.used(), 0);
    }

    #[test]
    fn execution_failure_removes_the_published_cache_entry() {
        let mut arena = StencilArena::new(4096).unwrap();
        let mut cache = crate::stencil_select::RenderedRegionCache::new();
        let site = QuickeningSite::<2>::new(Opcode::GetProperty);
        let values = PatchValues::from_site(&site);
        let stencil = Stencil {
            bytes: &[0xC3],
            holes: &[],
        };
        let result = arena.render_and_execute(
            &mut cache,
            crate::stencil_fact::RegionKey(9),
            &stencil,
            &values,
            |_| Err::<u32, _>(()),
            || Ok::<_, ()>(23),
        );
        assert_eq!(result, Ok(23));
        assert_eq!(cache.len(), 0);
        assert_eq!(arena.used(), 1);
    }

    #[test]
    fn data_only_region_returns_to_ordinary_semantics_without_execution() {
        let mut arena = StencilArena::new(4096).unwrap();
        let mut cache = RenderedRegionCache::new();
        let site = QuickeningSite::<2>::new(Opcode::GetProperty);
        let values = PatchValues::from_site(&site);
        let result = arena.render_selected_or_fallback(
            &mut cache,
            crate::stencil_fact::RegionKey::from_opcodes(
                crate::stencil_fact::RegionId(2),
                &[crate::ir::Opcode::GetProperty],
            ),
            &values,
            |_| Err::<u32, _>(()),
            || Ok::<_, ()>(17),
        );
        assert_eq!(result, Ok(17));
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn unknown_region_never_allocates_before_fallback() {
        let mut arena = StencilArena::new(4096).unwrap();
        let mut cache = RenderedRegionCache::new();
        let site = QuickeningSite::<2>::new(Opcode::GetProperty);
        let values = PatchValues::from_site(&site);
        let result = arena.render_selected_or_fallback(
            &mut cache,
            crate::stencil_fact::RegionKey(0),
            &values,
            |_| Ok::<_, ()>(99),
            || Ok::<_, ()>(7),
        );
        assert_eq!(result, Ok(7));
        assert_eq!(arena.used(), 0);
    }
}
