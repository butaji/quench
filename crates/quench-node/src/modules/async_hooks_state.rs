//! Runtime-neutral identity and rooted state shared by async-hooks adapters.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use quench_runtime::RootId;

pub(crate) const ROOT_ASYNC_ID: u64 = 1;
const FIRST_ASYNC_ID: u64 = ROOT_ASYNC_ID + 1;
const FIRST_LOCAL_STORAGE_ID: u64 = 1;

/// Identity counters shared by the legacy callback adapter and shared VM.
/// Runtime values and callback state remain owned by their respective adapters.
#[derive(Clone, Debug)]
pub(crate) struct AsyncIdentity(Rc<AsyncIdentityCells>);

#[derive(Debug)]
struct AsyncIdentityCells {
    next_async_id: Cell<u64>,
    current_async_id: Cell<u64>,
    next_local_storage_id: Cell<u64>,
}

impl AsyncIdentity {
    pub(crate) fn new() -> Self {
        Self(Rc::new(AsyncIdentityCells {
            next_async_id: Cell::new(FIRST_ASYNC_ID),
            current_async_id: Cell::new(ROOT_ASYNC_ID),
            next_local_storage_id: Cell::new(FIRST_LOCAL_STORAGE_ID),
        }))
    }

    pub(crate) fn allocate_async_id(&self) -> u64 {
        let id = self.0.next_async_id.get();
        self.0.next_async_id.set(id + 1);
        id
    }

    pub(crate) fn current_async_id(&self) -> u64 {
        self.0.current_async_id.get()
    }

    pub(crate) fn replace_current_async_id(&self, id: u64) -> u64 {
        self.0.current_async_id.replace(id)
    }

    pub(crate) fn allocate_local_storage_id(&self) -> u64 {
        let id = self.0.next_local_storage_id.get();
        self.0.next_local_storage_id.set(id + 1);
        id
    }
}

/// Rooted state owned exclusively by the shared-VM async-hooks adapter.
pub(crate) struct SharedAsyncHooksState {
    pub(super) identity: AsyncIdentity,
    pub(super) module: Option<RootId>,
    pub(super) local_stores: HashMap<(u64, u64), RootId>,
    pub(super) store_references: HashMap<RootId, usize>,
    pub(super) destroyed_resources: HashSet<u64>,
}

impl SharedAsyncHooksState {
    pub(crate) fn new(identity: AsyncIdentity) -> Self {
        Self {
            identity,
            module: None,
            local_stores: HashMap::new(),
            store_references: HashMap::new(),
            destroyed_resources: HashSet::new(),
        }
    }

    pub(crate) fn retain_store(&mut self, store: RootId) {
        let references = self.store_references.entry(store).or_default();
        *references = references
            .checked_add(1)
            .expect("shared async store reference count overflow");
    }

    pub(crate) fn release_store(&mut self, store: RootId) -> bool {
        let Some(references) = self.store_references.get_mut(&store) else {
            return false;
        };
        if *references > 1 {
            *references -= 1;
            return false;
        }
        self.store_references.remove(&store);
        true
    }
}
