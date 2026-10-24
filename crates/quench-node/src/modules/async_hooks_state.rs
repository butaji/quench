//! Runtime-neutral identity and rooted state shared by async-hooks adapters.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use quench_runtime::RootId;

/// Native event-loop execution has no JavaScript async resource.
pub(crate) const NATIVE_ASYNC_ID: u64 = 0;
pub(crate) const ROOT_ASYNC_ID: u64 = 1;
pub(crate) const ROOT_TRIGGER_ASYNC_ID: i64 = 0;
const FIRST_ASYNC_ID: u64 = ROOT_ASYNC_ID + 1;
const FIRST_LOCAL_STORAGE_ID: u64 = 1;

/// ALS ownership is tagged: public resources and private reaction snapshots
/// remain disjoint even when their integer payloads are equal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum StoreContext {
    Resource(u64),
    Job(u64),
}
impl From<u64> for StoreContext {
    fn from(id: u64) -> Self {
        Self::Resource(id)
    }
}
#[derive(Clone, Copy)]
pub(super) struct JobContext {
    pub resource: Option<u64>,
    pub execution: u64,
    pub trigger: i64,
}

/// State needed to restore a synchronous AsyncResource scope after the guest
/// callback returns. The receiver remains rooted by the active JS call frame.
pub(super) struct AsyncResourceScope {
    pub async_id: u64,
    pub resource_root: RootId,
    pub previous_async_id: u64,
    pub previous_trigger_id: i64,
    pub previous_store: StoreContext,
    pub previous_resource: Option<(u64, RootId)>,
}

/// Identity counters shared by the legacy callback adapter and shared VM.
/// Runtime values and callback state remain owned by their respective adapters.
#[derive(Clone, Debug)]
pub(crate) struct AsyncIdentity(Rc<AsyncIdentityCells>);

#[derive(Debug)]
struct AsyncIdentityCells {
    next_async_id: Cell<u64>,
    current_async_id: Cell<u64>,
    current_trigger_id: Cell<i64>,
    current_store: Cell<StoreContext>,
    next_local_storage_id: Cell<u64>,
}

impl AsyncIdentity {
    pub(crate) fn new() -> Self {
        Self(Rc::new(AsyncIdentityCells {
            next_async_id: Cell::new(FIRST_ASYNC_ID),
            current_async_id: Cell::new(ROOT_ASYNC_ID),
            current_trigger_id: Cell::new(ROOT_TRIGGER_ASYNC_ID),
            current_store: Cell::new(StoreContext::Resource(ROOT_ASYNC_ID)),
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
        self.0.current_store.set(StoreContext::Resource(id));
        self.0.current_async_id.replace(id)
    }

    pub(super) fn store_context(&self) -> StoreContext {
        self.0.current_store.get()
    }
    pub(super) fn replace_store_context(&self, key: StoreContext) -> StoreContext {
        self.0.current_store.replace(key)
    }

    pub(crate) fn current_trigger_id(&self) -> i64 {
        self.0.current_trigger_id.get()
    }
    pub(crate) fn replace_current_trigger_id(&self, id: i64) -> i64 {
        self.0.current_trigger_id.replace(id)
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
    pub(super) registry: super::async_hook_registry_shared_vm::Registry,
    pub(super) default_trigger_async_id: Option<f64>,
    pub(super) context_triggers: HashMap<u64, i64>,
    /// Store snapshots have private identities; async-hooks observes their
    /// actual Promise resource identity instead.
    pub(super) job_resources: HashMap<u64, JobContext>,
    pub(super) next_job_id: u64,
    pub(super) job_stack: Vec<(u64, i64, StoreContext)>,
    pub(super) promise_resources: HashMap<u64, quench_runtime::HostWeakRoot>,
    /// Promise parent identity is kept separately so V8 promise hooks can
    /// report the actual parent Promise without rooting either object.
    pub(super) promise_parents: HashMap<u64, Option<u64>>,
    /// The V8 `promiseHooks` registry is guest-owned, but retained here so
    /// Promise lifecycle callbacks can reach it without loading `v8` eagerly.
    pub(super) promise_hook_registry: Option<RootId>,
    pub(super) scoped_resource: Option<(u64, RootId)>,
    pub(super) resource_scopes: Vec<AsyncResourceScope>,
    pub(super) local_stores: HashMap<(StoreContext, u64), RootId>,
    pub(super) store_references: HashMap<RootId, usize>,
    pub(super) destroyed_resources: HashSet<u64>,
}

impl SharedAsyncHooksState {
    pub(crate) fn new(identity: AsyncIdentity) -> Self {
        Self {
            identity,
            module: None,
            registry: super::async_hook_registry_shared_vm::Registry::new(),
            default_trigger_async_id: None,
            context_triggers: HashMap::new(),
            job_resources: HashMap::new(),
            next_job_id: 0,
            job_stack: Vec::new(),
            promise_resources: HashMap::new(),
            promise_parents: HashMap::new(),
            promise_hook_registry: None,
            scoped_resource: None,
            resource_scopes: Vec::new(),
            local_stores: HashMap::new(),
            store_references: HashMap::new(),
            destroyed_resources: HashSet::new(),
        }
    }

    /// Node internal/async_hooks getDefaultTriggerAsyncId: a negative slot
    /// means inherit executionAsyncId; scope never changes execution or ALS.
    pub(super) fn default_trigger_async_id(&self) -> f64 {
        self.default_trigger_async_id
            .filter(|id| !(*id < 0.0))
            .unwrap_or(self.identity.current_async_id() as f64)
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
