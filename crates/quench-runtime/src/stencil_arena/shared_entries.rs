use super::*;

impl SharedStencilSlab {
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

    pub(super) fn validate_retained_address(
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
        owned_word_pair_entry,
        word_pair_entry,
        extern "C" fn(u64, u64) -> u64,
        crate::stencil_select::RegionAbi::ScalarWordPair
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
    typed_dispatch_entry!(
        owned_array_copy_loop_entry,
        crate::stencil_select::RegionAbi::ArrayCopyLoop
    );
    typed_dispatch_entry!(
        owned_array_reduction_loop_entry,
        crate::stencil_select::RegionAbi::ArrayReductionLoop
    );
    typed_dispatch_entry!(
        owned_affine_i32_loop_entry,
        crate::stencil_select::RegionAbi::AffineI32Loop
    );
    typed_dispatch_entry!(
        owned_numeric_f64_loop_entry,
        crate::stencil_select::RegionAbi::NumericF64Loop
    );
    typed_dispatch_entry!(
        owned_numeric_i32_bitwise_loop_entry,
        crate::stencil_select::RegionAbi::NumericI32BitwiseLoop
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
    pub(crate) fn word_pair_entry(
        &self,
        address: usize,
    ) -> Result<extern "C" fn(u64, u64) -> u64, ArenaError> {
        self.slab_for(address)
            .ok_or(ArenaError::ProtectionFailed)?
            .word_pair_entry(address)
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
