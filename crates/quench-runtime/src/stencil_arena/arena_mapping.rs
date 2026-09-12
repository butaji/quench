use super::*;

impl StencilArena {
    pub fn new(capacity: usize) -> Result<Self, ArenaError> {
        Self::new_in_budget(capacity, &GLOBAL_EXECUTABLE_BUDGET)
    }

    pub(super) fn new_in_budget(
        capacity: usize,
        budget: &'static AtomicBudget,
    ) -> Result<Self, ArenaError> {
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

    pub(super) fn mark_physical_execution(&self, view: crate::stencil_select::PhysicalStencilView) {
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

    pub(super) fn owns_address(&self, address: usize) -> bool {
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
    pub(crate) fn word_pair_entry(
        &self,
        address: usize,
    ) -> Result<extern "C" fn(u64, u64) -> u64, ArenaError> {
        self.require_abi(address, crate::stencil_select::RegionAbi::ScalarWordPair)?;
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
    ) -> Result<extern "C" fn(*const crate::native_core::value_word::TaggedValue) -> u64, ArenaError>
    {
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
        word: *const crate::native_core::value_word::TaggedValue,
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
        _word: *const crate::native_core::value_word::TaggedValue,
    ) -> Result<u64, ArenaError> {
        Err(ArenaError::ProtectionFailed)
    }
}
