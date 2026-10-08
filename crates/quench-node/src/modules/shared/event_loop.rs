use std::collections::{HashMap, HashSet, VecDeque};

pub struct EventLoop {
    shared: SharedCallbacks,
}

pub struct SharedCallback {
    pub callback: quench_runtime::RootId,
    pub receiver: quench_runtime::RootId,
    pub args: Vec<quench_runtime::RootId>,
}

struct SharedCallbacks {
    next_ticks: VecDeque<SharedCallback>,
    immediates: VecDeque<SharedImmediate>,
    next_immediate_id: u64,
    timers: VecDeque<SharedTimer>,
    next_timer_id: u64,
    firing_timers: HashSet<u64>,
    cancelled_timers: HashSet<u64>,
    listeners: HashMap<String, Vec<SharedCallback>>,
    exiting: bool,
}

pub struct SharedImmediate { pub id: u64, pub callback: SharedCallback }
pub struct SharedTimer {
    pub id: u64,
    pub due: std::time::Instant,
    pub interval: Option<std::time::Duration>,
    pub callback: SharedCallback,
}

impl Default for SharedCallbacks {
    fn default() -> Self {
        Self {
            next_ticks: VecDeque::new(), immediates: VecDeque::new(), next_immediate_id: 1,
            timers: VecDeque::new(), next_timer_id: 1, firing_timers: HashSet::new(),
            cancelled_timers: HashSet::new(), listeners: HashMap::new(), exiting: false,
        }
    }
}

impl Default for EventLoop { fn default() -> Self { Self::new() } }
impl EventLoop {
    pub fn new() -> Self { Self { shared: SharedCallbacks::default() } }
    pub fn queue_shared_next_tick(&mut self, callback: SharedCallback) { if !self.shared.exiting { self.shared.next_ticks.push_back(callback); } }
    pub fn reserve_shared_immediate_id(&mut self) -> Option<u64> { reserve(&mut self.shared.next_immediate_id) }
    pub fn queue_shared_immediate(&mut self, id: u64, callback: SharedCallback) { self.shared.immediates.push_back(SharedImmediate { id, callback }); }
    pub fn shared_immediate_cutoff(&self) -> Option<u64> { self.shared.next_immediate_id.checked_sub(1) }
    pub fn take_shared_immediate_through(&mut self, cutoff: u64) -> Option<SharedImmediate> {
        let index = self.shared.immediates.iter().position(|item| item.id <= cutoff)?;
        self.shared.immediates.remove(index)
    }
    pub fn cancel_shared_immediate(&mut self, id: u64) -> Option<SharedCallback> {
        let index = self.shared.immediates.iter().position(|item| item.id == id)?;
        self.shared.immediates.remove(index).map(|item| item.callback)
    }
    pub fn reserve_shared_timer_id(&mut self) -> Option<u64> { reserve(&mut self.shared.next_timer_id) }
    pub fn queue_shared_timer(&mut self, id: u64, delay: std::time::Duration, interval: Option<std::time::Duration>, callback: SharedCallback) {
        self.shared.timers.push_back(SharedTimer { id, due: std::time::Instant::now() + delay, interval, callback });
    }
    pub fn take_due_shared_timer(&mut self, now: std::time::Instant) -> Option<SharedTimer> {
        let index = self.shared.timers.iter().enumerate().filter(|(_, t)| t.due <= now).min_by_key(|(_, t)| t.due).map(|(index, _)| index)?;
        let timer = self.shared.timers.remove(index)?; self.shared.firing_timers.insert(timer.id); Some(timer)
    }
    pub fn next_shared_timer_due(&self) -> Option<std::time::Instant> { self.shared.timers.iter().map(|t| t.due).min() }
    pub fn cancel_shared_timer(&mut self, id: u64) -> Option<SharedCallback> {
        if self.shared.firing_timers.contains(&id) { self.shared.cancelled_timers.insert(id); return None; }
        let index = self.shared.timers.iter().position(|t| t.id == id)?;
        self.shared.timers.remove(index).map(|t| t.callback)
    }
    pub fn complete_shared_timer(&mut self, timer: SharedTimer, completed_at: std::time::Instant) -> Option<SharedCallback> {
        self.shared.firing_timers.remove(&timer.id);
        if self.shared.cancelled_timers.remove(&timer.id) { return Some(timer.callback); }
        let Some(interval) = timer.interval else { return Some(timer.callback); };
        let Some(due) = completed_at.checked_add(interval) else { return Some(timer.callback); };
        self.shared.timers.push_back(SharedTimer { id: timer.id, due, interval: Some(interval), callback: timer.callback }); None
    }
    pub fn add_shared_listener(&mut self, event: String, callback: SharedCallback) { self.shared.listeners.entry(event).or_default().push(callback); }
    pub fn take_shared_next_tick(&mut self) -> Option<SharedCallback> { self.shared.next_ticks.pop_front() }
    pub fn has_shared_next_ticks(&self) -> bool { !self.shared.next_ticks.is_empty() }
    pub fn shared_is_exiting(&self) -> bool { self.shared.exiting }
    pub fn begin_shared_exit(&mut self) -> Vec<SharedCallback> {
        if self.shared.exiting { return Vec::new(); }
        self.shared.exiting = true;
        self.shared.listeners.remove("exit").unwrap_or_default()
    }
    pub fn reset_shared(&mut self) { self.shared = SharedCallbacks::default(); }
}

fn reserve(next: &mut u64) -> Option<u64> { let id = *next; *next = id.checked_add(1)?; Some(id) }
