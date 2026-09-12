use std::cell::RefCell;
use std::rc::Rc;

use crate::machine::NativeDispatchError;
use crate::stencil_arena::{ArenaError, SharedStencilSlab, StencilArena};
use crate::stencil_lifecycle::StencilLifecycle;
use crate::stencil_select::RenderedRegionCache;

/// The single storage choice for a physical plan. A plan either owns a local
/// arena or participates in a shared slab; both states cannot coexist.
pub(crate) enum PhysicalStorage {
    Local(Option<StencilArena>),
    Shared(Rc<RefCell<SharedStencilSlab>>),
}

impl PhysicalStorage {
    pub(crate) fn shared(&self) -> Option<Rc<RefCell<SharedStencilSlab>>> {
        match self {
            Self::Shared(shared) => Some(Rc::clone(shared)),
            Self::Local(_) => None,
        }
    }

    pub(crate) fn local(&self) -> Option<&StencilArena> {
        match self {
            Self::Local(arena) => arena.as_ref(),
            Self::Shared(_) => None,
        }
    }

    pub(crate) fn local_mut(&mut self) -> Result<&mut StencilArena, ArenaError> {
        let Self::Local(arena) = self else {
            return Err(ArenaError::MappingFailed);
        };
        if arena.is_none() {
            *arena = Some(StencilArena::new(4096)?);
        }
        arena.as_mut().ok_or(ArenaError::MappingFailed)
    }

    pub(crate) fn reset_local(&mut self) {
        if let Self::Local(arena) = self {
            *arena = None;
        }
    }

    pub(crate) fn used(&self) -> usize {
        match self {
            Self::Local(arena) => arena.as_ref().map_or(0, StencilArena::used),
            Self::Shared(shared) => shared.borrow().used(),
        }
    }
}

/// Disposable cache state paired with the authoritative lifecycle state.
pub(crate) struct PhysicalState {
    pub(crate) cache: RenderedRegionCache,
    pub(crate) lifecycle: StencilLifecycle,
}

/// Shared installation authority for specialized plans whose entry is a
/// typed token. The token is only a derived capability; this object owns the
/// cache, lifecycle and pool lease boundary used to resolve and invoke it.
pub(crate) struct SharedPhysicalEntry<F: Copy> {
    pub(crate) owner: Rc<RefCell<SharedStencilSlab>>,
    pub(crate) state: PhysicalState,
    installed: Option<crate::stencil_arena::EntryToken<F>>,
}

impl<F: Copy> SharedPhysicalEntry<F> {
    pub(crate) fn new(owner: Rc<RefCell<SharedStencilSlab>>) -> Self {
        Self {
            owner,
            state: PhysicalState::new(),
            installed: None,
        }
    }

    pub(crate) fn entry<P, E>(
        &mut self,
        publish: P,
        to_entry: E,
    ) -> Result<crate::stencil_arena::EntryToken<F>, ArenaError>
    where
        P: FnOnce(
            &Rc<RefCell<SharedStencilSlab>>,
            &mut RenderedRegionCache,
        ) -> Result<usize, ArenaError>,
        E: FnOnce(
            &SharedStencilSlab,
            usize,
        ) -> Result<crate::stencil_arena::EntryToken<F>, ArenaError>,
    {
        if self.state.lifecycle.state() == crate::stencil_lifecycle::StencilState::Retired {
            return Err(ArenaError::ProtectionFailed);
        }
        if let Some(entry) = self
            .installed
            .filter(|entry| self.owner.borrow().entry_token_is_live(*entry))
        {
            return Ok(entry);
        }
        self.installed = None;
        self.state.clear();
        let address = match publish(&self.owner, &mut self.state.cache) {
            Ok(address) => address,
            Err(error) => {
                self.state.clear();
                return Err(error);
            }
        };
        let entry = match to_entry(&self.owner.borrow(), address) {
            Ok(entry) => entry,
            Err(error) => {
                let _ = self
                    .owner
                    .borrow_mut()
                    .retire_allocation(address, &mut self.state.cache);
                return Err(error);
            }
        };
        self.installed = Some(entry);
        Ok(entry)
    }

    pub(crate) fn invoke<R>(
        &self,
        entry: crate::stencil_arena::EntryToken<F>,
        call: impl FnOnce(F) -> R,
    ) -> Result<R, ArenaError> {
        let lease = SharedStencilSlab::acquire_owned(&self.owner, entry)?;
        lease.invoke(call)
    }

    /// Invoke the already-installed token without repeating the publication
    /// lookup. The lease acquisition still validates the token and pins its
    /// owning slab for the duration of the call; a stale token is reported so
    /// the caller can clear and republish through `entry`.
    #[inline]
    pub(crate) fn invoke_cached<R>(&self, call: impl FnOnce(F) -> R) -> Result<R, ArenaError> {
        let entry = self.installed.ok_or(ArenaError::ProtectionFailed)?;
        self.invoke(entry, call)
    }

    #[inline]
    pub(crate) fn is_installed(&self) -> bool {
        self.installed.is_some()
    }

    pub(crate) fn clear(&mut self) {
        self.installed = None;
        self.state.clear();
    }

    pub(crate) fn retire(&mut self) {
        if let Some(entry) = self.installed.take() {
            let _ = self
                .owner
                .borrow_mut()
                .retire_allocation(entry.address(), &mut self.state.cache);
        }
        self.state.retire();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    #[test]
    fn shared_entry_reuses_one_verified_token_and_lease_boundary() {
        let owner = Rc::new(RefCell::new(SharedStencilSlab::new(4096).unwrap()));
        let key = crate::stencil_select::numeric_region_key(crate::ir::Opcode::Add).unwrap();
        let view = crate::stencil_select::select_physical_for_abi(
            key,
            crate::stencil_select::RegionAbi::ScalarF64Binary,
        )
        .unwrap();
        let site = crate::quickening::QuickeningSite::<2>::new(crate::ir::Opcode::Add);
        let values = crate::stencil_fact::PatchValues::from_site(&site);
        let mut installation =
            SharedPhysicalEntry::<extern "C" fn(f64, f64) -> f64>::new(Rc::clone(&owner));
        let entry = installation
            .entry(
                |owner, cache| {
                    let address = owner
                        .borrow_mut()
                        .render_physical_view_or_get(cache, view, &values)?;
                    owner.borrow_mut().make_executable(address)?;
                    Ok(address)
                },
                |pool, address| pool.owned_f64_entry(address),
            )
            .unwrap();
        assert_eq!(installation.invoke(entry, |call| call(2.0, 3.0)), Ok(5.0));
        assert_eq!(installation.invoke_cached(|call| call(4.0, 5.0)), Ok(9.0));
        let reused = installation
            .entry(
                |_owner, _cache| panic!("live installation rendered twice"),
                |_pool, _address| panic!("live installation converted twice"),
            )
            .unwrap();
        assert_eq!(reused.address(), entry.address());
        assert_eq!(owner.borrow().active_leases(), 0);
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    #[test]
    fn retired_shared_entry_cannot_reenter_after_cache_clear() {
        let owner = Rc::new(RefCell::new(SharedStencilSlab::new(4096).unwrap()));
        let key = crate::stencil_select::numeric_region_key(crate::ir::Opcode::Add).unwrap();
        let view = crate::stencil_select::select_physical_for_abi(
            key,
            crate::stencil_select::RegionAbi::ScalarF64Binary,
        )
        .unwrap();
        let site = crate::quickening::QuickeningSite::<2>::new(crate::ir::Opcode::Add);
        let values = crate::stencil_fact::PatchValues::from_site(&site);
        let mut installation = SharedPhysicalEntry::<extern "C" fn(f64, f64) -> f64>::new(owner);
        installation
            .entry(
                |owner, cache| {
                    let address = owner
                        .borrow_mut()
                        .render_physical_view_or_get(cache, view, &values)?;
                    owner.borrow_mut().make_executable(address)?;
                    Ok(address)
                },
                |pool, address| pool.owned_f64_entry(address),
            )
            .unwrap();
        installation.retire();
        assert!(matches!(
            installation.entry(|_, _| unreachable!(), |_, _| unreachable!()),
            Err(ArenaError::ProtectionFailed)
        ));
    }
}

impl PhysicalState {
    pub(crate) fn new() -> Self {
        Self {
            cache: RenderedRegionCache::new(),
            lifecycle: StencilLifecycle::new(),
        }
    }

    pub(crate) fn clear(&mut self) {
        self.cache.clear();
        self.lifecycle.reset();
    }

    pub(crate) fn retire(&mut self) {
        self.cache.clear();
        self.lifecycle.retire();
    }

    pub(crate) fn apply_dispatch_outcome<T>(
        &mut self,
        result: &Result<T, NativeDispatchError>,
        published: Option<(&Rc<RefCell<SharedStencilSlab>>, usize)>,
    ) {
        match result {
            Err(NativeDispatchError::Physical(_)) => self.clear(),
            Err(NativeDispatchError::Committed { .. }) => {
                if let Some((arena, address)) = published {
                    let _ = arena
                        .borrow_mut()
                        .retire_allocation(address, &mut self.cache);
                }
                self.retire();
            }
            _ => {}
        }
    }
}

/// One installed-state authority for a semantic specialization. `I` remains a
/// closed plan-local ABI enum, preserving valid polymorphism without unrelated
/// optional callable pointers.
pub(crate) struct PhysicalInstallation<I> {
    pub(crate) storage: PhysicalStorage,
    pub(crate) state: PhysicalState,
    installed: I,
}

impl<I: Copy> PhysicalInstallation<I> {
    pub(crate) fn local(unpublished: I) -> Self {
        Self {
            storage: PhysicalStorage::Local(None),
            state: PhysicalState::new(),
            installed: unpublished,
        }
    }

    pub(crate) fn use_shared(&mut self, shared: Rc<RefCell<SharedStencilSlab>>) {
        self.storage = PhysicalStorage::Shared(shared);
    }

    pub(crate) fn installed(&self) -> I {
        self.installed
    }

    pub(crate) fn publish(&mut self, installed: I) {
        self.installed = installed;
    }

    pub(crate) fn clear(&mut self, unpublished: I) {
        self.installed = unpublished;
        self.storage.reset_local();
        self.state.clear();
    }

    pub(crate) fn apply_dispatch_outcome<T>(
        &mut self,
        result: &Result<T, NativeDispatchError>,
        published: Option<(&Rc<RefCell<SharedStencilSlab>>, usize)>,
        unpublished: I,
    ) {
        match result {
            Err(NativeDispatchError::Physical(_)) => {
                self.installed = unpublished;
                self.storage.reset_local();
            }
            Err(NativeDispatchError::Committed { .. }) => {
                self.installed = unpublished;
                self.storage.reset_local();
            }
            _ => {}
        }
        self.state.apply_dispatch_outcome(result, published);
    }
}
