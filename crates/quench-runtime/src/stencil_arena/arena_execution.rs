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
        let entry_rhs = if key == crate::stencil_select::add_const_region_key() {
            values.constant_bits().map(f64::from_bits).unwrap_or(rhs)
        } else {
            rhs
        };
        match self.render_selected_scalar(
            cache,
            key,
            values,
            crate::stencil_select::RegionAbi::ScalarF64Binary,
            true,
            |arena, address| arena.execute_f64(address, lhs, entry_rhs),
        ) {
            Ok(value) => Ok(value),
            Err(_) => fallback(),
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
        self.render_selected_scalar(
            cache,
            key,
            values,
            crate::stencil_select::RegionAbi::ScalarBool,
            false,
            |arena, address| arena.execute_bool(address, lhs, rhs),
        )
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
        self.render_selected_scalar(
            cache,
            key,
            values,
            crate::stencil_select::RegionAbi::ScalarI32,
            false,
            |arena, address| arena.execute_i32(address, lhs, rhs),
        )
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
        self.render_selected_scalar(
            cache,
            key,
            values,
            crate::stencil_select::RegionAbi::ScalarU32,
            false,
            |arena, address| arena.execute_u32(address, lhs, rhs),
        )
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    fn render_selected_scalar<const N: usize, T>(
        &mut self,
        cache: &mut RenderedRegionCache,
        key: crate::stencil_fact::RegionKey,
        values: &PatchValues<'_, N>,
        abi: crate::stencil_select::RegionAbi,
        allow_fallthrough: bool,
        invoke: impl FnOnce(&Self, usize) -> Result<T, ArenaError>,
    ) -> Result<T, ArenaError> {
        let Some(view) = crate::stencil_select::select_physical(key).filter(|view| view.executable)
        else {
            return Err(ArenaError::ProtectionFailed);
        };
        self.render_selected_scalar_view(cache, view, values, abi, allow_fallthrough, invoke)
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    fn render_selected_scalar_view<const N: usize, T>(
        &mut self,
        cache: &mut RenderedRegionCache,
        view: crate::stencil_select::PhysicalStencilView,
        values: &PatchValues<'_, N>,
        abi: crate::stencil_select::RegionAbi,
        allow_fallthrough: bool,
        invoke: impl FnOnce(&Self, usize) -> Result<T, ArenaError>,
    ) -> Result<T, ArenaError> {
        if view.contract().abi != abi || (!allow_fallthrough && view.fallthrough.is_some()) {
            return Err(ArenaError::ProtectionFailed);
        }
        let key = view.key;
        let address = self.render_physical_view_or_get(cache, view, values)?;
        if let Err(error) = self.make_executable() {
            cache.remove(key, view.cache_signature(values), address);
            return Err(error);
        }
        match invoke(self, address) {
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
        self.render_selected_scalar_view(
            cache,
            view,
            values,
            crate::stencil_select::RegionAbi::ScalarF64x3,
            false,
            |arena, address| {
                let entry = arena.f64x3_entry(address)?;
                Ok(entry(lhs, rhs, third))
            },
        )
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
        let published = PublishedEntry::new(identity, byte_len, usize::from(view.entry));
        if self.is_executable() && self.require_publication(address, published).is_ok() {
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
        let checkpoint = self.cursor;
        let address = self.copy_finalized_image(cache, image)?;
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
