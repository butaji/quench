//! Shared JavaScript and Wasm data blocks, including their atomic waiter lifecycle.
//! Every access, including non-Atomics views, must use these operations.
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};
// One sequential order for JavaScript and Wasm atomic accesses across data blocks.
// Always acquire this before a backing lock; ordinary byte accesses only need the latter.
static MEMORY_ORDER: Mutex<()> = Mutex::new(());
pub(crate) fn memory_order() -> std::sync::MutexGuard<'static, ()> {
    MEMORY_ORDER.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessError {
    Bounds,
    Alignment,
    Width,
    Maximum,
    Poisoned,
    Waiters,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitResult {
    NotEqual,
    Ok,
    TimedOut,
    Cancelled,
}
#[derive(Debug, Clone, Copy)]
pub enum Rmw {
    Add,
    Sub,
    And,
    Or,
    Xor,
    Exchange,
    CompareExchange(u64),
}
const MAX_ADDRESS_WAITERS: usize = u32::MAX as usize;

#[derive(Default, Debug)]
struct WaitState {
    notified: bool,
    cancelled: bool,
}
#[derive(Debug)]
struct Waiter {
    notified: Mutex<WaitState>,
    wake: Condvar,
    deadline: Option<Instant>,
}
#[derive(Debug)]
pub(crate) struct State {
    pub(crate) bytes: crate::wasm::memory::LinearBytes,
    waiters: HashMap<usize, VecDeque<Arc<Waiter>>>,
}
/// Views share a single storage lock and waiter queue across independent runtimes.
/// No byte slice or raw pointer escapes the storage guard.
#[derive(Debug)]
pub struct SharedBacking {
    state: Arc<Mutex<State>>,
    extent: BackingExtent,
}
#[derive(Debug)]
enum BackingExtent {
    Fixed(usize),
    WasmBuffer(usize),
    Growable { maximum: usize },
    WasmMemory,
}
impl State {
    fn enroll(&mut self, offset: usize, waiter: Arc<Waiter>) -> Result<(), AccessError> {
        if let Some(queue) = self.waiters.get_mut(&offset) {
            if queue.len() >= MAX_ADDRESS_WAITERS { return Err(AccessError::Waiters); }
            queue.try_reserve(1).map_err(|_| AccessError::Waiters)?;
            queue.push_back(waiter);
        } else {
            let mut queue = VecDeque::new();
            queue.try_reserve(1).map_err(|_| AccessError::Waiters)?;
            self.waiters.try_reserve(1).map_err(|_| AccessError::Waiters)?;
            queue.push_back(waiter);
            self.waiters.insert(offset, queue);
        }
        Ok(())
    }
    fn prune_waiters(&mut self, offset: usize) {
        if self.waiters.get(&offset).is_some_and(VecDeque::is_empty) {
            self.waiters.remove(&offset);
        }
        if self.waiters.is_empty() { self.waiters = HashMap::new(); }
    }
}

impl SharedBacking {
    pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn new(length: usize, maximum: usize, growable: bool) -> Result<Arc<Self>, AccessError> {
        if length > maximum || (!growable && length != maximum) {
            return Err(AccessError::Maximum);
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| AccessError::Maximum)?;
        bytes.resize(length, 0);
        Ok(Arc::new(Self {
            state: Arc::new(Mutex::new(State {
                bytes: crate::wasm::memory::LinearBytes::heap(bytes),
                waiters: HashMap::new(),
            })),
            extent: if growable { BackingExtent::Growable { maximum } } else { BackingExtent::Fixed(length) },
        }))
    }
    /// One storage and waiter authority for Wasm and JavaScript agents.
    pub(crate) fn from_wasm(bytes: crate::wasm::memory::LinearBytes) -> Self {
        Self {
            state: Arc::new(Mutex::new(State { bytes, waiters: HashMap::new() })),
            extent: BackingExtent::WasmMemory,
        }
    }
    /// Shared Wasm buffers retain their original length after memory growth.
    pub(crate) fn fixed_view(&self) -> Arc<Self> {
        let length = self.lock().bytes.len();
        Arc::new(Self {
            state: self.state.clone(), extent: BackingExtent::WasmBuffer(length),
        })
    }
    /// The Wasm shared-buffer clone protocol exposes the data block's current
    /// length, even when the source wrapper predates memory growth.
    pub(crate) fn clone_for_agent(self: &Arc<Self>) -> Arc<Self> {
        if matches!(self.extent, BackingExtent::WasmBuffer(_)) { self.fixed_view() } else { self.clone() }
    }
    pub(crate) fn copy_from(&self, source: &Self, input: std::ops::Range<usize>, output: std::ops::Range<usize>) {
        if self.shares_data_block(source) {
            self.lock().bytes.copy_within(input, output.start);
        } else if Arc::as_ptr(&self.state) < Arc::as_ptr(&source.state) {
            let mut destination = self.lock();
            let source = source.lock();
            destination.bytes[output].copy_from_slice(&source.bytes[input]);
        } else {
            let source = source.lock();
            let mut destination = self.lock();
            destination.bytes[output].copy_from_slice(&source.bytes[input]);
        }
    }
    pub(crate) fn shares_data_block(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }
    pub(crate) fn len_for_view(&self) -> Option<usize> {
        match self.extent { BackingExtent::Fixed(length) | BackingExtent::WasmBuffer(length) => Some(length), _ => None }
    }
    pub(crate) fn visible_len(&self, state: &State) -> usize {
        self.len_for_view().unwrap_or(state.bytes.len())
    }
    pub fn length(&self) -> Result<usize, AccessError> {
        let state = self.state.lock().map_err(|_| AccessError::Poisoned)?;
        Ok(self.visible_len(&state))
    }
    #[cfg(test)]
    pub(crate) fn waiter_count(&self) -> usize {
        self.lock().waiters.values().map(VecDeque::len).sum()
    }
    pub fn maximum(&self) -> usize {
        match self.extent {
            BackingExtent::Fixed(length) | BackingExtent::WasmBuffer(length) => length,
            BackingExtent::Growable { maximum } => maximum,
            BackingExtent::WasmMemory => usize::MAX,
        }
    }
    pub fn growable(&self) -> bool {
        matches!(self.extent, BackingExtent::Growable { .. })
    }
    pub fn grow(&self, length: usize) -> Result<(), AccessError> {
        let mut s = self.state.lock().map_err(|_| AccessError::Poisoned)?;
        if !self.growable() || length < s.bytes.len() || length > self.maximum() {
            return Err(AccessError::Maximum);
        }
        if !s.bytes.grow_to(length) { return Err(AccessError::Maximum); }
        Ok(())
    }
    fn range(
        length: usize,
        offset: usize,
        width: usize,
        aligned: bool,
    ) -> Result<std::ops::Range<usize>, AccessError> {
        if !matches!(width, 1 | 2 | 4 | 8) {
            return Err(AccessError::Width);
        }
        if aligned && !offset.is_multiple_of(width) {
            return Err(AccessError::Alignment);
        }
        let end = offset.checked_add(width).ok_or(AccessError::Bounds)?;
        if end > length {
            return Err(AccessError::Bounds);
        }
        Ok(offset..end)
    }
    fn load_locked(&self, s: &State, offset: usize, width: usize) -> Result<u64, AccessError> {
        let r = Self::range(self.visible_len(s), offset, width, true)?;
        let mut bits = 0;
        for (index, byte) in s.bytes[r].iter().enumerate() {
            let lane = if cfg!(target_endian = "little") {
                index
            } else {
                width - index - 1
            };
            bits |= u64::from(*byte) << (lane * u8::BITS as usize)
        }
        Ok(bits)
    }
    fn store_locked(
        &self,
        s: &mut State,
        offset: usize,
        width: usize,
        value: u64,
    ) -> Result<(), AccessError> {
        let r = Self::range(self.visible_len(s), offset, width, true)?;
        for (index, byte) in s.bytes[r].iter_mut().enumerate() {
            let lane = if cfg!(target_endian = "little") {
                index
            } else {
                width - index - 1
            };
            *byte = (value >> (lane * u8::BITS as usize)) as u8;
        }
        Ok(())
    }
    /// Native-endian integer lanes match Core typed-array storage. DataView's
    /// explicit endian policy uses raw byte access.
    pub fn load(&self, offset: usize, width: usize) -> Result<u64, AccessError> {
        let _order = memory_order();
        {
            let state = self.state.lock().map_err(|_| AccessError::Poisoned)?;
            self.load_locked(&state, offset, width)
        }
    }
    pub fn store(&self, offset: usize, width: usize, value: u64) -> Result<(), AccessError> {
        let _order = memory_order();
        {
            let mut state = self.state.lock().map_err(|_| AccessError::Poisoned)?;
            self.store_locked(&mut state, offset, width, value)
        }
    }
    pub fn read_bytes(&self, offset: usize, length: usize) -> Result<Vec<u8>, AccessError> {
        let s = self.state.lock().map_err(|_| AccessError::Poisoned)?;
        let end = offset.checked_add(length).ok_or(AccessError::Bounds)?;
        Ok(s.bytes[..self.visible_len(&s)]
            .get(offset..end)
            .ok_or(AccessError::Bounds)?
            .to_vec())
    }
    pub fn write_bytes(&self, offset: usize, bytes: &[u8]) -> Result<(), AccessError> {
        let mut s = self.state.lock().map_err(|_| AccessError::Poisoned)?;
        let end = offset.checked_add(bytes.len()).ok_or(AccessError::Bounds)?;
        let length = self.visible_len(&s);
        s.bytes[..length]
            .get_mut(offset..end)
            .ok_or(AccessError::Bounds)?
            .copy_from_slice(bytes);
        Ok(())
    }
    pub fn rmw(
        &self,
        offset: usize,
        width: usize,
        value: u64,
        op: Rmw,
    ) -> Result<u64, AccessError> {
        let _order = memory_order();
        let mut s = self.state.lock().map_err(|_| AccessError::Poisoned)?;
        let old = self.load_locked(&s, offset, width)?;
        let mask = u64::MAX >> ((8 - width) * 8);
        let next = match op {
            Rmw::Add => old.wrapping_add(value),
            Rmw::Sub => old.wrapping_sub(value),
            Rmw::And => old & value,
            Rmw::Or => old | value,
            Rmw::Xor => old ^ value,
            Rmw::Exchange => value,
            Rmw::CompareExchange(expected) => {
                if old == (expected & mask) {
                    value
                } else {
                    old
                }
            }
        };
        self.store_locked(&mut s, offset, width, next & mask)?;
        Ok(old)
    }
    /// Enrollment and expected-value check share the storage lock, preventing
    /// lost notifications between the comparison and queue insertion.
    pub fn wait(
        &self,
        offset: usize,
        width: usize,
        expected: u64,
        timeout: Option<Duration>,
    ) -> Result<WaitResult, AccessError> {
        self.wait_inner(offset, width, expected, timeout, None)
    }
    pub fn wait_with_interrupt(
        &self,
        offset: usize,
        width: usize,
        expected: u64,
        timeout: Option<Duration>,
        interrupt: &crate::ExecutionInterrupt,
    ) -> Result<WaitResult, AccessError> {
        self.wait_inner(offset, width, expected, timeout, Some(interrupt))
    }
    fn wait_inner(
        &self,
        offset: usize,
        width: usize,
        expected: u64,
        timeout: Option<Duration>,
        interrupt: Option<&crate::ExecutionInterrupt>,
    ) -> Result<WaitResult, AccessError> {
        if !matches!(width, 4 | 8) {
            return Err(AccessError::Width);
        }
        let deadline = timeout.and_then(|d| Instant::now().checked_add(d));
        let waiter = Arc::new(Waiter {
            notified: Mutex::new(WaitState::default()),
            wake: Condvar::new(),
            deadline,
        });
        {
            let _order = memory_order();
            let mut s = self.lock();
            let old = self.load_locked(&s, offset, width)?;
            let mask = u64::MAX >> ((8 - width) * 8);
            if old != (expected & mask) {
                return Ok(WaitResult::NotEqual);
            }
            s.enroll(offset, waiter.clone())?;
        }
        let _callback = interrupt.map(|interrupt| {
            let waiter = waiter.clone();
            let callback: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                let mut state = waiter.notified.lock().unwrap_or_else(|e| e.into_inner());
                state.cancelled = true;
                waiter.wake.notify_one();
            });
            interrupt.register_termination_waker(&callback);
            callback
        });
        let mut state = waiter.notified.lock().map_err(|_| AccessError::Poisoned)?;
        loop {
            if state.cancelled {
                break;
            }
            if state.notified {
                return Ok(WaitResult::Ok);
            }
            match deadline {
                None => state = waiter.wake.wait(state).map_err(|_| AccessError::Poisoned)?,
                Some(deadline) => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        break;
                    }
                    let (next, _) = waiter
                        .wake
                        .wait_timeout(state, remaining)
                        .map_err(|_| AccessError::Poisoned)?;
                    state = next;
                }
            }
        }
        drop(state);
        let mut s = self.lock();
        let state = waiter.notified.lock().map_err(|_| AccessError::Poisoned)?;
        if let Some(queue) = s.waiters.get_mut(&offset) {
            queue.retain(|w| !Arc::ptr_eq(w, &waiter));
        }
        s.prune_waiters(offset);
        Ok(if state.cancelled {
            WaitResult::Cancelled
        } else if state.notified {
            WaitResult::Ok
        } else {
            WaitResult::TimedOut
        })
    }
    pub fn notify(&self, offset: usize, count: usize) -> Result<usize, AccessError> {
        let _order = memory_order();
        let mut s = self.state.lock().map_err(|_| AccessError::Poisoned)?;
        let mut n = 0;
        if let Some(queue) = s.waiters.get_mut(&offset) {
            while n < count {
                let Some(waiter) = queue.pop_front() else {
                    break;
                };
                let mut state = waiter.notified.lock().map_err(|_| AccessError::Poisoned)?;
                if state.cancelled || waiter.deadline.is_some_and(|d| Instant::now() >= d) {
                    continue;
                }
                state.notified = true;
                waiter.wake.notify_one();
                n += 1;
            }
        }
        s.prune_waiters(offset);
        Ok(n)
    }
}

#[derive(Debug)]
pub enum AsyncStart {
    Ready(WaitResult),
    Pending(AsyncWait),
}
#[derive(Debug)]
pub struct AsyncWait {
    backing: Arc<SharedBacking>,
    waiter: Arc<Waiter>,
    offset: usize,
    deadline: Option<Instant>,
}
impl AsyncWait {
    pub fn poll(&self) -> Option<WaitResult> {
        let mut storage = self.backing.lock();
        let state = self
            .waiter
            .notified
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let result = if state.cancelled {
            Some(WaitResult::Cancelled)
        } else if state.notified {
            Some(WaitResult::Ok)
        } else if self.deadline.is_some_and(|d| Instant::now() >= d) {
            Some(WaitResult::TimedOut)
        } else {
            None
        };
        if result.is_some() {
            if let Some(queue) = storage.waiters.get_mut(&self.offset) {
                queue.retain(|w| !Arc::ptr_eq(w, &self.waiter));
            }
            storage.prune_waiters(self.offset);
        }
        result
    }
}
impl Drop for AsyncWait {
    fn drop(&mut self) {
        let mut storage = self.backing.lock();
        if let Some(queue) = storage.waiters.get_mut(&self.offset) {
            queue.retain(|w| !Arc::ptr_eq(w, &self.waiter));
        }
        storage.prune_waiters(self.offset);
    }
}
impl SharedBacking {
    pub fn wait_async(
        self: &Arc<Self>,
        offset: usize,
        width: usize,
        expected: u64,
        timeout: Option<Duration>,
    ) -> Result<AsyncStart, AccessError> {
        let _order = memory_order();
        if !matches!(width, 4 | 8) {
            return Err(AccessError::Width);
        }
        let mut storage = self.lock();
        let old = self.load_locked(&storage, offset, width)?;
        let mask = u64::MAX >> ((8 - width) * 8);
        if old != (expected & mask) {
            return Ok(AsyncStart::Ready(WaitResult::NotEqual));
        }
        if timeout == Some(Duration::ZERO) {
            return Ok(AsyncStart::Ready(WaitResult::TimedOut));
        }
        let deadline = timeout.and_then(|d| Instant::now().checked_add(d));
        let waiter = Arc::new(Waiter {
            notified: Mutex::new(WaitState::default()),
            wake: Condvar::new(),
            deadline,
        });
        storage.enroll(offset, waiter.clone())?;
        Ok(AsyncStart::Pending(AsyncWait {
            backing: self.clone(),
            waiter,
            offset,
            deadline,
        }))
    }
}
