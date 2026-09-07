use super::*;

impl StencilArena {
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

    pub(super) fn alloc_aligned(
        &mut self,
        size: usize,
        alignment: usize,
    ) -> Result<usize, ArenaError> {
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

    pub(super) fn render_selected_view<const N: usize>(
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

    pub(super) fn render_selected_physical_view<const N: usize>(
        &mut self,
        cache: &mut RenderedRegionCache,
        view: crate::stencil_select::PhysicalStencilView,
        values: &PatchValues<'_, N>,
    ) -> Result<usize, ArenaError> {
        self.render_selected_physical_view_with_control(cache, view, values, None)
    }

    pub(super) fn render_selected_controlled_view<const N: usize>(
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

    pub(super) fn record_publication(
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

    pub(super) fn require_publication(
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

    /// A cache signature is an index, not proof that an immutable code image
    /// is the one selected by the caller. Compare the finalized bytes before
    /// reusing a composed entry so a collision cannot authorize other code.
    pub(super) fn require_region_image(
        &self,
        address: usize,
        image: &VerifiedRegionImage,
    ) -> Result<(), ArenaError> {
        self.require_publication(address, image.identity(), image.bytes().len())?;
        let offset = address
            .checked_sub(self.ptr as usize)
            .ok_or(ArenaError::ProtectionFailed)?;
        let end = offset
            .checked_add(image.bytes().len())
            .ok_or(ArenaError::ProtectionFailed)?;
        if end > self.cursor {
            return Err(ArenaError::ProtectionFailed);
        }
        let published = unsafe {
            std::slice::from_raw_parts(self.ptr.add(offset), image.bytes().len())
        };
        (published == image.bytes())
            .then_some(())
            .ok_or(ArenaError::ProtectionFailed)
    }

    pub(super) fn require_abi(
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
}
