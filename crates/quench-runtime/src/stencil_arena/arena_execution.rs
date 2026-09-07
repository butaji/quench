use super::*;

impl StencilArena {
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

    pub(super) fn cached_executable(
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
            self.require_region_image(address, image)?;
            self.make_executable()?;
            return Ok(address);
        }
        self.publish_composed(cache, image)
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    pub(super) fn publish_composed(
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
    pub(super) fn byte(&self, offset: usize) -> u8 {
        assert!(offset < self.cursor);
        unsafe { *self.ptr.add(offset) }
    }
}
