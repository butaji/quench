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
        self.state.clear();
    }
}
