//! Scheduler queues and callbacks owned by the shared Node host.

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};

/// Callback queues and scheduler state for the shared Node host.
pub struct SharedEventLoop {
    shared: SharedCallbacks,
}

/// Guest callbacks retained by the shared Node host between execution phases.
pub struct SharedCallback {
    pub callback: quench_runtime::RootId,
    pub receiver: quench_runtime::RootId,
    pub args: Vec<quench_runtime::RootId>,
}

struct SharedCallbacks {
    next_ticks: VecDeque<SharedCallback>,
    immediates: VecDeque<SharedImmediate>,
    next_immediate_id: u64,
    /// Node's Immediate ref count is an aliased Uint32 and follows ref/unref
    /// transitions, not the reflectable per-handle Symbol slot.
    immediate_ref_count: u32,
    /// Node toggles the host's Immediate reference only at counter zero
    /// crossings; wrapping can make this differ from `immediate_ref_count`.
    immediate_ref_latch: bool,
    timers: VecDeque<SharedTimer>,
    next_timer_id: u64,
    /// Live scheduler projections of each timer handle's private `refed` slot.
    firing_timers: HashMap<u64, bool>,
    cancelled_timers: HashSet<u64>,
    listeners: HashMap<SharedEventKey, Vec<SharedProcessListener>>,
    next_listener_id: u64,
    exiting: bool,
}

struct SharedProcessListener {
    id: u64,
    once: bool,
    callback: SharedCallback,
}

#[derive(Clone, Copy)]
pub struct SharedListenerSnapshot {
    pub id: u64,
    pub once: bool,
    pub callback: quench_runtime::RootId,
    pub receiver: quench_runtime::RootId,
}

pub struct SharedImmediate {
    pub id: u64,
    pub owner: SharedImmediateOwner,
    pub callback: SharedCallback,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SharedImmediateOwner {
    Guest,
    Host,
}

pub struct SharedTimer {
    pub id: u64,
    pub due: std::time::Instant,
    pub interval: Option<std::time::Duration>,
    /// Live scheduler projection of the handle's canonical private ref bit.
    pub refed: bool,
    pub callback: SharedCallback,
}

#[derive(Clone, Debug)]
pub enum SharedEventKey {
    String(String),
    Symbol {
        identity: quench_runtime::Value,
        root: quench_runtime::RootId,
    },
}

impl PartialEq for SharedEventKey {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::String(left), Self::String(right)) => left == right,
            (
                Self::Symbol { identity: left, .. },
                Self::Symbol {
                    identity: right, ..
                },
            ) => left == right,
            _ => false,
        }
    }
}

impl Eq for SharedEventKey {}

impl Hash for SharedEventKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::String(value) => {
                0_u8.hash(state);
                value.hash(state);
            }
            Self::Symbol { identity, .. } => {
                1_u8.hash(state);
                identity.hash(state);
            }
        }
    }
}

const FIRST_SHARED_IMMEDIATE_ID: u64 = 1;
const FIRST_SHARED_LISTENER_ID: u64 = 1;

impl Default for SharedCallbacks {
    fn default() -> Self {
        Self {
            next_ticks: VecDeque::new(),
            immediates: VecDeque::new(),
            next_immediate_id: FIRST_SHARED_IMMEDIATE_ID,
            immediate_ref_count: 0,
            immediate_ref_latch: false,
            timers: VecDeque::new(),
            next_timer_id: FIRST_SHARED_IMMEDIATE_ID,
            firing_timers: HashMap::new(),
            cancelled_timers: HashSet::new(),
            listeners: HashMap::new(),
            next_listener_id: FIRST_SHARED_LISTENER_ID,
            exiting: false,
        }
    }
}

impl Default for SharedEventLoop {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedEventLoop {
    pub fn new() -> Self {
        Self {
            shared: SharedCallbacks::default(),
        }
    }

    pub fn queue_shared_next_tick(&mut self, callback: SharedCallback) {
        if !self.shared.exiting {
            self.shared.next_ticks.push_back(callback);
        }
    }

    pub fn reserve_shared_immediate_id(&mut self) -> Option<u64> {
        let id = self.shared.next_immediate_id;
        self.shared.next_immediate_id = id.checked_add(1)?;
        Some(id)
    }

    pub fn reserve_shared_listener_id(&mut self) -> Option<u64> {
        let id = self.shared.next_listener_id;
        self.shared.next_listener_id = id.checked_add(1)?;
        Some(id)
    }

    pub fn queue_shared_immediate(
        &mut self,
        id: u64,
        owner: SharedImmediateOwner,
        callback: SharedCallback,
    ) {
        if owner == SharedImmediateOwner::Guest {
            self.transition_shared_immediate_ref(true);
        }
        self.shared.immediates.push_back(SharedImmediate {
            id,
            owner,
            callback,
        });
    }

    pub fn shared_guest_immediate_handles(&self) -> Vec<(u64, quench_runtime::RootId)> {
        self.shared
            .immediates
            .iter()
            .filter(|immediate| immediate.owner == SharedImmediateOwner::Guest)
            .map(|immediate| (immediate.id, immediate.callback.receiver))
            .collect()
    }

    pub fn transition_shared_immediate_ref(&mut self, refed: bool) {
        if refed {
            let was_zero = self.shared.immediate_ref_count == 0;
            self.shared.immediate_ref_count = self.shared.immediate_ref_count.wrapping_add(1);
            if was_zero {
                self.shared.immediate_ref_latch = true;
            }
        } else {
            self.shared.immediate_ref_count = self.shared.immediate_ref_count.wrapping_sub(1);
            if self.shared.immediate_ref_count == 0 {
                self.shared.immediate_ref_latch = false;
            }
        }
    }

    pub fn shared_immediate_cutoff(&self) -> Option<u64> {
        self.shared.next_immediate_id.checked_sub(1)
    }

    pub fn take_shared_immediate_through(&mut self, cutoff: u64) -> Option<SharedImmediate> {
        let index = self
            .shared
            .immediates
            .iter()
            .position(|immediate| immediate.id <= cutoff)?;
        self.shared.immediates.remove(index)
    }

    pub fn take_all_shared_immediates(&mut self) -> Vec<SharedImmediate> {
        std::mem::take(&mut self.shared.immediates)
            .into_iter()
            .collect()
    }

    pub fn cancel_shared_guest_immediate(&mut self, id: u64) -> Option<SharedImmediate> {
        let index = self
            .shared
            .immediates
            .iter()
            .position(|item| item.id == id && item.owner == SharedImmediateOwner::Guest)?;
        self.shared.immediates.remove(index)
    }

    pub fn has_refed_shared_immediates(&self) -> bool {
        self.shared.immediate_ref_latch
            || self
                .shared
                .immediates
                .iter()
                .any(|immediate| immediate.owner == SharedImmediateOwner::Host)
    }

    pub fn discard_unreferenced_shared_immediates(&mut self) -> Vec<SharedCallback> {
        let mut retained = VecDeque::new();
        let mut released = Vec::new();
        for immediate in std::mem::take(&mut self.shared.immediates) {
            if immediate.owner == SharedImmediateOwner::Host {
                retained.push_back(immediate);
            } else {
                released.push(immediate.callback);
            }
        }
        self.shared.immediates = retained;
        released
    }

    pub fn reserve_shared_timer_id(&mut self) -> Option<u64> {
        let id = self.shared.next_timer_id;
        self.shared.next_timer_id = id.checked_add(1)?;
        Some(id)
    }

    pub fn queue_shared_timer(
        &mut self,
        id: u64,
        delay: std::time::Duration,
        interval: Option<std::time::Duration>,
        callback: SharedCallback,
    ) {
        self.shared.timers.push_back(SharedTimer {
            id,
            due: std::time::Instant::now() + delay,
            interval,
            refed: true,
            callback,
        });
    }

    pub fn take_due_shared_timer(&mut self, now: std::time::Instant) -> Option<SharedTimer> {
        let index = self
            .shared
            .timers
            .iter()
            .enumerate()
            .filter(|(_, timer)| timer.due <= now)
            .min_by_key(|(_, timer)| timer.due)
            .map(|(index, _)| index)?;
        let timer = self.shared.timers.remove(index)?;
        self.shared.firing_timers.insert(timer.id, timer.refed);
        Some(timer)
    }

    pub fn next_shared_timer_due(&self) -> Option<std::time::Instant> {
        self.shared.timers.iter().map(|timer| timer.due).min()
    }

    pub fn has_refed_shared_timers(&self) -> bool {
        self.shared.timers.iter().any(|timer| timer.refed)
            || self.shared.firing_timers.values().any(|refed| *refed)
    }

    pub fn discard_unreferenced_shared_timers(&mut self) -> Vec<SharedCallback> {
        let mut retained = VecDeque::new();
        let mut released = Vec::new();
        for timer in std::mem::take(&mut self.shared.timers) {
            if timer.refed {
                retained.push_back(timer);
            } else {
                released.push(timer.callback);
                self.shared.cancelled_timers.remove(&timer.id);
            }
        }
        self.shared.timers = retained;
        released
    }

    pub fn set_shared_timer_ref(&mut self, id: u64, refed: bool) {
        if let Some(timer) = self.shared.timers.iter_mut().find(|timer| timer.id == id) {
            timer.refed = refed;
            return;
        }
        if let Some(current) = self.shared.firing_timers.get_mut(&id) {
            *current = refed;
        }
    }

    pub fn cancel_shared_timer(&mut self, id: u64) -> Option<SharedCallback> {
        if self.shared.firing_timers.contains_key(&id) {
            self.shared.cancelled_timers.insert(id);
            return None;
        }
        let index = self.shared.timers.iter().position(|timer| timer.id == id)?;
        self.shared.timers.remove(index).map(|timer| timer.callback)
    }

    pub fn complete_shared_timer(
        &mut self,
        mut timer: SharedTimer,
        completed_at: std::time::Instant,
    ) -> Option<SharedCallback> {
        if let Some(refed) = self.shared.firing_timers.remove(&timer.id) {
            timer.refed = refed;
        }
        if self.shared.cancelled_timers.remove(&timer.id) {
            return Some(timer.callback);
        }
        let Some(interval) = timer.interval else {
            return Some(timer.callback);
        };
        let Some(next_due) = completed_at.checked_add(interval) else {
            return Some(timer.callback);
        };
        self.shared.timers.push_back(SharedTimer {
            id: timer.id,
            due: next_due,
            interval: Some(interval),
            refed: timer.refed,
            callback: timer.callback,
        });
        None
    }

    pub fn add_shared_listener(
        &mut self,
        event: SharedEventKey,
        id: u64,
        callback: SharedCallback,
        once: bool,
    ) -> (bool, Option<quench_runtime::RootId>) {
        let new_event = !self.shared.listeners.contains_key(&event);
        let duplicate_root = match (&event, new_event) {
            (SharedEventKey::Symbol { root, .. }, false) => Some(*root),
            _ => None,
        };
        self.shared
            .listeners
            .entry(event)
            .or_default()
            .push(SharedProcessListener { id, once, callback });
        (new_event, duplicate_root)
    }

    pub fn shared_listener_snapshots(&self, event: &SharedEventKey) -> Vec<SharedListenerSnapshot> {
        self.shared
            .listeners
            .get(event)
            .into_iter()
            .flatten()
            .map(|listener| SharedListenerSnapshot {
                id: listener.id,
                once: listener.once,
                callback: listener.callback.callback,
                receiver: listener.callback.receiver,
            })
            .collect()
    }

    pub fn take_shared_once_listener(
        &mut self,
        event: &SharedEventKey,
        id: u64,
    ) -> Option<(SharedCallback, Option<quench_runtime::RootId>, bool)> {
        let listeners = self.shared.listeners.get_mut(event)?;
        let index = listeners
            .iter()
            .position(|listener| listener.id == id && listener.once)?;
        let listener = listeners.remove(index);
        if !listeners.is_empty() {
            return Some((listener.callback, None, false));
        }
        let (event, _) = self.shared.listeners.remove_entry(event)?;
        let event_root = match event {
            SharedEventKey::Symbol { root, .. } => Some(root),
            SharedEventKey::String(_) => None,
        };
        Some((listener.callback, event_root, true))
    }

    pub fn remove_shared_listener(
        &mut self,
        event: &SharedEventKey,
        id: u64,
    ) -> Option<(SharedCallback, Option<quench_runtime::RootId>, bool)> {
        let listeners = self.shared.listeners.get_mut(event)?;
        let index = listeners.iter().position(|listener| listener.id == id)?;
        let listener = listeners.remove(index);
        if !listeners.is_empty() {
            return Some((listener.callback, None, false));
        }
        let (event, _) = self.shared.listeners.remove_entry(event)?;
        let event_root = match event {
            SharedEventKey::Symbol { root, .. } => Some(root),
            SharedEventKey::String(_) => None,
        };
        Some((listener.callback, event_root, true))
    }

    pub fn shared_listener_count(&self) -> usize {
        self.shared.listeners.len()
    }

    pub fn take_shared_next_tick(&mut self) -> Option<SharedCallback> {
        self.shared.next_ticks.pop_front()
    }

    pub fn has_shared_next_ticks(&self) -> bool {
        !self.shared.next_ticks.is_empty()
    }

    pub fn shared_is_exiting(&self) -> bool {
        self.shared.exiting
    }

    pub fn begin_shared_exit(&mut self) -> Vec<SharedCallback> {
        if self.shared.exiting {
            return Vec::new();
        }
        self.shared.exiting = true;
        let key = SharedEventKey::String("exit".to_owned());
        self.shared
            .listeners
            .remove(&key)
            .unwrap_or_default()
            .into_iter()
            .map(|listener| listener.callback)
            .collect()
    }

    pub fn reset_shared(&mut self) {
        self.shared = SharedCallbacks::default();
    }
}
