//! Scheduler queues and callbacks owned by the shared Node host.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::collections::{HashMap, HashSet, VecDeque};

/// Callback queues and scheduler state for the shared Node host.
pub struct SharedEventLoop {
    shared: SharedCallbacks,
}

/// Async identity and retained resource owned by one queued operation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CallbackLifetime {
    Operation,
    Resource,
}
pub struct CallbackExecution {
    pub lifetime: CallbackLifetime,
    pub optional_callback: Option<RootId>,
    pub id: u64,
    pub resource: RootId,
}

/// Retained invocation view; resource-owned mutable slots remain canonical.
pub struct SharedCallback {
    pub execution: Option<CallbackExecution>,
    pub callback: quench_runtime::RootId,
    pub receiver: quench_runtime::RootId,
    pub args: Vec<quench_runtime::RootId>,
}

/// Retain a native-context callback into Node's canonical next-tick queue.
pub(crate) fn queue_next_tick(
    context: &mut NativeContext<'_, NodeHost>,
    callback: RootId,
    receiver: RootId,
    args: &[RootId],
) -> Result<(), RootedError> {
    let resource = context.object_rooted()?;
    let key = context.string_rooted("callback");
    context.define_data_property_rooted(resource, key, callback, true, true, true)?;
    let values = if args.is_empty() {
        context.undefined()
    } else {
        context.array_rooted(args)?
    };
    let key = context.string_rooted("args");
    context.define_data_property_rooted(resource, key, values, true, true, true)?;
    let id = super::async_hooks_shared_vm::create_context(&context.host_mut().shared_state());
    super::async_hooks_shared_vm::emit_init(context, resource, "TickObject", id)?;
    let execution = Some(CallbackExecution {
        lifetime: crate::modules::shared_event_loop::CallbackLifetime::Operation,
        optional_callback: None,
        id,
        resource: context.retain(resource)?,
    });
    // The job object owns the mutable callback/argument slots. The retained
    // callable is only an invocation view, as for Timeout handles.
    let invocation = context.host_function_with_data(
        crate::host::shared_vm::operation("asyncTickInvoke"),
        receiver,
    )?;
    let callback = context.retain(invocation)?;
    let receiver = context.retain(resource)?;
    let args = Vec::new();
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .scheduler
        .queue_shared_next_tick(SharedCallback {
            execution,
            callback,
            receiver,
            args,
        });
    Ok(())
}

pub(crate) fn invoke_tick(
    context: &mut NativeContext<'_, NodeHost>,
    resource: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let receiver = context.host_function_data()?;
    let key = context.string_rooted("callback");
    let callback = context.get_property_rooted(resource, key)?;
    let key = context.string_rooted("args");
    let args = context.get_property_rooted(resource, key)?;
    if context
        .rooted_value(args)
        .is_some_and(|value| value.is_undefined())
    {
        context.call_rooted(callback, receiver, &[])
    } else {
        let apply = context.with_startup_intrinsics(|context| {
            let global = context.global_root()?;
            let key = context.string_rooted("Reflect");
            let reflect = context.get_property_rooted(global, key)?;
            let key = context.string_rooted("apply");
            context.get_property_rooted(reflect, key)
        })?;
        let undefined = context.undefined();
        context.call_rooted(apply, undefined, &[callback, receiver, args])
    }
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
    firing_timers: HashMap<u64, SharedFiringTimer>,
    cancelled_timers: HashSet<u64>,
    exiting: bool,
}

struct SharedFiringTimer {
    refed: bool,
    refreshed_due: Option<std::time::Instant>,
    repeat_started: Option<std::time::Instant>,
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

const FIRST_SHARED_IMMEDIATE_ID: u64 = 1;

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

    pub fn shared_active_timer_counts(&self) -> (usize, usize) {
        let timers = self
            .shared
            .timers
            .iter()
            .filter(|timer| timer.refed)
            .count()
            + self
                .shared
                .firing_timers
                .iter()
                .filter(|(id, refed)| refed.refed && !self.shared.cancelled_timers.contains(id))
                .count();
        (timers, self.shared.immediate_ref_count as usize)
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

    pub fn take_shared_native_immediate(&mut self) -> Option<SharedImmediate> {
        let index = self
            .shared
            .immediates
            .iter()
            .position(|immediate| immediate.owner == SharedImmediateOwner::Host)?;
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
        self.shared.firing_timers.insert(
            timer.id,
            SharedFiringTimer {
                refed: timer.refed,
                refreshed_due: None,
                repeat_started: None,
            },
        );
        Some(timer)
    }

    pub fn next_shared_timer_due(&self) -> Option<std::time::Instant> {
        self.shared.timers.iter().map(|timer| timer.due).min()
    }

    pub fn has_refed_shared_timers(&self) -> bool {
        self.shared.timers.iter().any(|timer| timer.refed)
            || self.shared.firing_timers.values().any(|timer| timer.refed)
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
            current.refed = refed;
        }
    }

    pub fn begin_shared_timer_repeat(&mut self, id: u64) {
        if let Some(timer) = self.shared.firing_timers.get_mut(&id) {
            timer.repeat_started = Some(std::time::Instant::now());
        }
    }

    /// Refresh the existing queue/firing owner; the caller creates a new owner
    /// only when the previous callback has completed and been released.
    pub fn refresh_shared_timer(&mut self, id: u64, delay: std::time::Duration) -> bool {
        let due = std::time::Instant::now() + delay;
        if let Some(index) = self.shared.timers.iter().position(|timer| timer.id == id) {
            let mut timer = self
                .shared
                .timers
                .remove(index)
                .expect("located queued timer");
            timer.due = due;
            self.shared.timers.push_back(timer);
            self.shared.cancelled_timers.remove(&id);
            return true;
        }
        if let Some(timer) = self.shared.firing_timers.get_mut(&id) {
            timer.refreshed_due = Some(due);
            self.shared.cancelled_timers.remove(&id);
            return true;
        }
        false
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
        let firing = self.shared.firing_timers.remove(&timer.id);
        if let Some(firing) = &firing {
            timer.refed = firing.refed;
        }
        if self.shared.cancelled_timers.remove(&timer.id) {
            return Some(timer.callback);
        }
        let next_due = match timer.interval {
            Some(interval) => firing
                .as_ref()
                .and_then(|firing| firing.repeat_started)
                .unwrap_or(completed_at)
                .checked_add(interval),
            None => firing.and_then(|firing| firing.refreshed_due),
        };
        let Some(next_due) = next_due else {
            return Some(timer.callback);
        };
        self.shared.timers.push_back(SharedTimer {
            id: timer.id,
            due: next_due,
            interval: timer.interval,
            refed: timer.refed,
            callback: timer.callback,
        });
        None
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

    pub fn begin_shared_exit(&mut self) -> bool {
        if self.shared.exiting {
            return false;
        }
        self.shared.exiting = true;
        true
    }

    pub fn reset_shared(&mut self) {
        self.shared = SharedCallbacks::default();
    }
}

/// Resolve a native mutable callback slot without inventing a guest wrapper.
pub(crate) fn optional_callback(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let callback = context.get_property_rooted(receiver, args[0])?;
    if context.is_callable_rooted(callback)? {
        Ok(callback)
    } else {
        Ok(context.undefined())
    }
}
