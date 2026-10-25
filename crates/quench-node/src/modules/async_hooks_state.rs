//! Runtime-neutral identity and rooted state shared by async-hooks adapters.

use std::cell::Cell;
use rustc_hash::{FxHashMap, FxHashSet};
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
pub(super) enum StoreContext { Resource(u64), Job(u64) }
impl From<u64> for StoreContext {
    fn from(id: u64) -> Self { Self::Resource(id) }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct JobContext {
    pub resource: Option<u64>,
    pub execution: u64,
    pub trigger: i64,
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

    pub(super) fn store_context(&self) -> StoreContext { self.0.current_store.get() }
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
    pub(super) context_triggers: FxHashMap<u64, i64>,
    /// Store snapshots have private identities; async-hooks observes their
    /// actual Promise resource identity instead.
    pub(super) job_resources: FxHashMap<u64, JobContext>,
    pub(super) next_job_id: u64,
    pub(super) job_stack: Vec<(u64, i64, StoreContext)>,
    pub(super) promise_resources: FxHashMap<u64, quench_runtime::HostWeakRoot>,
    /// The collection epoch `promise_resources` was last pruned at: entries can only have
    /// died when a collection ran since.
    pub(crate) promise_resources_pruned_epoch: u64,
    pub(super) scoped_resource: Option<(u64, RootId)>,
    /// The scopes `AsyncResource#runInAsyncScope` entered and has not left, innermost last
    /// (`async_hooks_shared_vm::enter_async_scope`).
    pub(super) entered_scopes: Vec<super::async_hooks_shared_vm::EnteredScope>,
    /// Each context's AsyncLocalStorage values, by storage ID: a context's frame is found,
    /// copied and dropped whole, without scanning the other contexts'.
    pub(super) local_stores: FxHashMap<StoreContext, Vec<(u64, RootId)>>,
    pub(super) store_references: FxHashMap<RootId, usize>,
    pub(super) destroyed_resources: FxHashSet<u64>,
    /// The AsyncLocalStorage instances entered since their last `disable()`. Like Node's
    /// storage hook, an enabled one makes promises tracked (`tracks_promises`).
    pub(super) enabled_local_storages: FxHashSet<u64>,
}

impl SharedAsyncHooksState {
    pub(crate) fn new(identity: AsyncIdentity) -> Self {
        Self {
            identity,
            module: None,
            registry: super::async_hook_registry_shared_vm::Registry::new(),
            default_trigger_async_id: None,
            context_triggers: FxHashMap::default(),
            job_resources: FxHashMap::default(),
            next_job_id: 0,
            job_stack: Vec::new(),
            promise_resources: FxHashMap::default(),
            promise_resources_pruned_epoch: 0,
            scoped_resource: None,
            entered_scopes: Vec::new(),
            local_stores: FxHashMap::default(),
            store_references: FxHashMap::default(),
            destroyed_resources: FxHashSet::default(),
            enabled_local_storages: FxHashSet::default(),
        }
    }

    pub(crate) fn tracks_promise_lifecycle_resources(&self) -> bool {
        self.registry.tracks_promise_lifecycle_resources()
    }

    /// Node internal/async_hooks getDefaultTriggerAsyncId: a negative slot
    /// means inherit executionAsyncId; scope never changes execution or ALS.
    /// Node gives Promises async resources only while its promise hook is installed.
    pub(crate) fn tracks_promises(&self) -> bool {
        self.registry.tracks_promises()
    }

    /// Whether a Promise job boundary has work: Promise resources to prune or hooks that
    /// track Promises.
    pub(crate) fn observes_job_boundaries(&self) -> bool {
        self.tracks_promises() || !self.promise_resources.is_empty()
    }

    pub(super) fn default_trigger_async_id(&self) -> f64 {
        self.default_trigger_async_id.filter(|id| !(*id < 0.0))
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

impl SharedAsyncHooksState {
    /// The identity counters and cached module of a bootstrap that ran no async resource:
    /// a restored process continues numbering where the snapshot's left off.
    pub(crate) fn write_snapshot(&self, writer: &mut quench_runtime::SnapshotWriter<'_>) -> Result<(), String> {
        use crate::host::startup_snapshot::{require_quiescent, HostSnap};
        let Self {
            identity,
            module,
            registry,
            default_trigger_async_id,
            context_triggers,
            job_resources,
            next_job_id,
            job_stack,
            promise_resources,
            promise_resources_pruned_epoch: _,
            scoped_resource,
            entered_scopes,
            local_stores,
            store_references,
            destroyed_resources,
            enabled_local_storages,
        } = self;
        let cells = &identity.0;
        require_quiescent(
            cells.current_async_id.get() == ROOT_ASYNC_ID
                && cells.current_trigger_id.get() == ROOT_TRIGGER_ASYNC_ID
                && cells.current_store.get() == StoreContext::Resource(ROOT_ASYNC_ID)
                && default_trigger_async_id.is_none()
                && scoped_resource.is_none()
                && entered_scopes.is_empty(),
            "an async execution context",
        )?;
        require_quiescent(
            context_triggers.is_empty()
                && job_resources.is_empty()
                && job_stack.is_empty()
                && promise_resources.is_empty()
                && local_stores.is_empty()
                && store_references.is_empty()
                && destroyed_resources.is_empty()
                && enabled_local_storages.is_empty(),
            "an async resource",
        )?;
        cells.next_async_id.get().write(writer)?;
        cells.next_local_storage_id.get().write(writer)?;
        next_job_id.write(writer)?;
        module.write(writer)?;
        registry.write_snapshot(writer)
    }

    pub(crate) fn read_snapshot(&mut self, reader: &mut quench_runtime::SnapshotReader<'_, '_>) -> Result<(), String> {
        use crate::host::startup_snapshot::HostSnap;
        self.identity.0.next_async_id.set(HostSnap::read(reader)?);
        self.identity.0.next_local_storage_id.set(HostSnap::read(reader)?);
        self.next_job_id = HostSnap::read(reader)?;
        self.module = HostSnap::read(reader)?;
        self.registry.read_snapshot(reader)
    }
}
