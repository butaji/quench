//! Thread-safe, identity-preserving backing prototype for genuine SAB integration.
//! Every access, including non-Atomics views, must use these operations.
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessError {
    Bounds,
    Alignment,
    Width,
    Maximum,
    Poisoned,
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
    pub(crate) bytes: Vec<u8>,
    waiters: HashMap<usize, VecDeque<Arc<Waiter>>>,
}
/// One Arc identity is the shared data block across independent runtimes.
/// Mutex synchronization makes lane accesses/RMW linearizable, including
/// overlapping views. No &[u8]/&mut[u8]/raw pointer is exposed across threads.
#[derive(Debug)]
pub struct SharedBacking {
    state: Mutex<State>,
    maximum: usize,
    growable: bool,
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
            state: Mutex::new(State {
                bytes,
                waiters: HashMap::new(),
            }),
            maximum,
            growable,
        }))
    }
    pub fn length(&self) -> Result<usize, AccessError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| AccessError::Poisoned)?
            .bytes
            .len())
    }
    pub fn maximum(&self) -> usize {
        self.maximum
    }
    pub fn growable(&self) -> bool {
        self.growable
    }
    pub fn grow(&self, length: usize) -> Result<(), AccessError> {
        let mut s = self.state.lock().map_err(|_| AccessError::Poisoned)?;
        if !self.growable || length < s.bytes.len() || length > self.maximum {
            return Err(AccessError::Maximum);
        }
        let extra = length - s.bytes.len();
        s.bytes
            .try_reserve_exact(extra)
            .map_err(|_| AccessError::Maximum)?;
        s.bytes.resize(length, 0);
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
        if aligned && offset % width != 0 {
            return Err(AccessError::Alignment);
        }
        let end = offset.checked_add(width).ok_or(AccessError::Bounds)?;
        if end > length {
            return Err(AccessError::Bounds);
        }
        Ok(offset..end)
    }
    fn load_locked(s: &State, offset: usize, width: usize) -> Result<u64, AccessError> {
        let r = Self::range(s.bytes.len(), offset, width, true)?;
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
        s: &mut State,
        offset: usize,
        width: usize,
        value: u64,
    ) -> Result<(), AccessError> {
        let r = Self::range(s.bytes.len(), offset, width, true)?;
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
        {
            let state = self.state.lock().map_err(|_| AccessError::Poisoned)?;
            Self::load_locked(&state, offset, width)
        }
    }
    pub fn store(&self, offset: usize, width: usize, value: u64) -> Result<(), AccessError> {
        {
            let mut state = self.state.lock().map_err(|_| AccessError::Poisoned)?;
            Self::store_locked(&mut state, offset, width, value)
        }
    }
    pub fn read_bytes(&self, offset: usize, length: usize) -> Result<Vec<u8>, AccessError> {
        let s = self.state.lock().map_err(|_| AccessError::Poisoned)?;
        let end = offset.checked_add(length).ok_or(AccessError::Bounds)?;
        Ok(s.bytes
            .get(offset..end)
            .ok_or(AccessError::Bounds)?
            .to_vec())
    }
    pub fn write_bytes(&self, offset: usize, bytes: &[u8]) -> Result<(), AccessError> {
        let mut s = self.state.lock().map_err(|_| AccessError::Poisoned)?;
        let end = offset.checked_add(bytes.len()).ok_or(AccessError::Bounds)?;
        s.bytes
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
        let mut s = self.state.lock().map_err(|_| AccessError::Poisoned)?;
        let old = Self::load_locked(&s, offset, width)?;
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
        Self::store_locked(&mut s, offset, width, next & mask)?;
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
            let mut s = self.lock();
            let old = Self::load_locked(&s, offset, width)?;
            let mask = u64::MAX >> ((8 - width) * 8);
            if old != (expected & mask) {
                return Ok(WaitResult::NotEqual);
            }
            s.waiters
                .entry(offset)
                .or_default()
                .push_back(waiter.clone());
        }
        let callback: Arc<dyn Fn() + Send + Sync> = {
            let waiter = waiter.clone();
            Arc::new(move || {
                let mut state = waiter.notified.lock().unwrap_or_else(|e| e.into_inner());
                state.cancelled = true;
                waiter.wake.notify_one();
            })
        };
        if let Some(interrupt) = interrupt {
            interrupt.register_termination_waker(&callback)
        }
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
            if queue.is_empty() {
                s.waiters.remove(&offset);
            }
        }
        Ok(if state.cancelled {
            WaitResult::Cancelled
        } else if state.notified {
            WaitResult::Ok
        } else {
            WaitResult::TimedOut
        })
    }
    pub fn notify(&self, offset: usize, count: usize) -> Result<usize, AccessError> {
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
            if queue.is_empty() {
                s.waiters.remove(&offset);
            }
        }
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
                if queue.is_empty() {
                    storage.waiters.remove(&self.offset);
                }
            }
        }
        result
    }
}
impl Drop for AsyncWait {
    fn drop(&mut self) {
        let mut storage = self.backing.lock();
        if let Some(queue) = storage.waiters.get_mut(&self.offset) {
            queue.retain(|w| !Arc::ptr_eq(w, &self.waiter));
            if queue.is_empty() {
                storage.waiters.remove(&self.offset);
            }
        }
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
        if !matches!(width, 4 | 8) {
            return Err(AccessError::Width);
        }
        let mut storage = self.lock();
        let old = Self::load_locked(&storage, offset, width)?;
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
        storage
            .waiters
            .entry(offset)
            .or_default()
            .push_back(waiter.clone());
        Ok(AsyncStart::Pending(AsyncWait {
            backing: self.clone(),
            waiter,
            offset,
            deadline,
        }))
    }
}
