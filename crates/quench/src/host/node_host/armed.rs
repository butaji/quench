//! A module's state whose drop and teardown link only where the module first writes it.

use std::mem::ManuallyDrop;
use std::ops::{Deref, DerefMut};

use quench_runtime::{CollectionContext, Runtime};

use crate::host::NodeHost;

/// A state whose module releases what it holds: what collected objects owned, and all of it as
/// the process exits.
pub(crate) trait Teardown: Default {
    /// Releases what the state holds, as the process exits (`cleanup_native_work`).
    fn teardown(_: &mut Runtime<NodeHost>) {}

    /// Releases what collected objects owned, between turns of the event loop.
    fn prune(_: &mut Runtime<NodeHost>) {}

    /// Releases what collected objects owned, as a collection ends (`after_collection`).
    fn prune_at_collection(_: &mut CollectionContext<'_>) {}
}

/// The release hooks of a state a write armed.
#[derive(Clone, Copy)]
pub(crate) struct Hooks {
    pub(crate) teardown: fn(&mut Runtime<NodeHost>),
    pub(crate) prune: fn(&mut Runtime<NodeHost>),
    pub(crate) prune_at_collection: fn(&mut CollectionContext<'_>),
}

/// A module's state, as its default until the module's code first writes it (`DerefMut`): only
/// that write arms its drop and release hooks, so an image whose code never writes it links
/// none of them (a crypto state's native objects, which hold OpenSSL).
pub(crate) struct Armed<T: Teardown> {
    value: ManuallyDrop<T>,
    drop: Option<unsafe fn(&mut ManuallyDrop<T>)>,
    hooks: Option<Hooks>,
}

impl<T: Teardown> Default for Armed<T> {
    fn default() -> Self {
        Self { value: ManuallyDrop::new(T::default()), drop: None, hooks: None }
    }
}

impl<T: Teardown> Armed<T> {
    /// The state, for a write that leaves it holding nothing to release (a snapshot's read of a
    /// state written quiescent).
    pub(crate) fn unarmed(&mut self) -> &mut T {
        &mut self.value
    }

    /// The state, for a write only code that already wrote it reaches (a TLS socket's error,
    /// which only a secure context the state holds makes).
    pub(crate) fn armed_mut(&mut self) -> &mut T {
        debug_assert!(self.hooks.is_some(), "a write to a state no write armed");
        &mut self.value
    }

    /// The release hooks, once a write armed them.
    pub(crate) fn hooks(&self) -> Option<Hooks> {
        self.hooks
    }
}

impl<T: Teardown> Deref for Armed<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T: Teardown> DerefMut for Armed<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.drop = Some(ManuallyDrop::drop);
        self.hooks = Some(Hooks { teardown: T::teardown, prune: T::prune, prune_at_collection: T::prune_at_collection });
        &mut self.value
    }
}

impl<T: Teardown> Drop for Armed<T> {
    fn drop(&mut self) {
        // A state never written is its default, which holds nothing to release.
        if let Some(drop) = self.drop {
            // SAFETY: the value is dropped once, here, and never read after.
            unsafe { drop(&mut self.value) }
        }
    }
}
