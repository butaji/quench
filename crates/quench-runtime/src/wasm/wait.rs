//! Waiter lifecycle belongs to the linear-memory backing, independently of VM frames.

use super::atomic::memory_order;
use super::memory::{MemoryLoad, MemoryStorage, SharedMemory};
use super::{WasmTrap, WasmValue};
use std::thread::Thread;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub(crate) enum WaitResult {
    Notified = 0,
    NotEqual = 1,
    TimedOut = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WaitState {
    Waiting,
    Notified,
}

#[derive(Debug)]
pub(super) struct Waiter {
    address: u64,
    state: WaitState,
    thread: Thread,
}

// The Wasm wait protocol traps when adding the 2^32nd waiter at an address.
const MAX_ADDRESS_WAITERS: usize = u32::MAX as usize;
// Negative signed nanosecond timeouts never expire.
const MIN_FINITE_TIMEOUT_NS: i64 = 0;

impl MemoryStorage {
    /// Waiting needs a backing store other agents can notify; an unshared
    /// memory still checks the access before it traps.
    pub(crate) fn wait(
        &self,
        load: MemoryLoad,
        address: u64,
        expected: WasmValue,
        timeout: i64,
    ) -> Result<WaitResult, WasmTrap> {
        match self {
            Self::Unshared(_) => {
                let _order = memory_order();
                load.read_atomic(&self.lock(), address)?;
                Err(WasmTrap::WaitOnUnsharedMemory)
            }
            Self::Shared(memory) => memory.wait(load, address, expected, timeout),
        }
    }

    /// An unshared memory has no waiters to wake.
    pub(crate) fn notify(&self, address: u64, count: u32) -> Result<u32, WasmTrap> {
        match self {
            Self::Unshared(_) => {
                let _order = memory_order();
                MemoryLoad::I32Load.read_atomic(&self.lock(), address)?;
                Ok(0)
            }
            Self::Shared(memory) => memory.notify(address, count),
        }
    }
}

impl SharedMemory {
    pub(crate) fn wait(
        &self,
        load: MemoryLoad,
        address: u64,
        expected: WasmValue,
        timeout: i64,
    ) -> Result<WaitResult, WasmTrap> {
        let order = memory_order();
        let mut memory = self.state();
        let loaded = load.read_atomic(&memory.bytes, address)?;
        if loaded != expected {
            return Ok(WaitResult::NotEqual);
        }
        if memory
            .waiters
            .iter()
            .filter(|waiter| waiter.address == address && waiter.state == WaitState::Waiting)
            .count()
            >= MAX_ADDRESS_WAITERS
        {
            return Err(WasmTrap::TooManyWaiters);
        }
        if timeout == MIN_FINITE_TIMEOUT_NS {
            return Ok(WaitResult::TimedOut);
        }
        memory
            .waiters
            .try_reserve(1)
            .map_err(|_| WasmTrap::TooManyWaiters)?;
        let thread = std::thread::current();
        memory.waiters.push(Waiter {
            address,
            state: WaitState::Waiting,
            thread: thread.clone(),
        });
        let deadline = (timeout >= MIN_FINITE_TIMEOUT_NS)
            .then(|| (Instant::now(), Duration::from_nanos(timeout as u64)));
        // Comparison and registration are one ordered transition. Blocking must
        // release both locks. unpark retains a permit if notification precedes park,
        // closing the unlock-to-suspend race without a per-waiter allocation.
        drop(order);
        let result = loop {
            let index = memory
                .waiters
                .iter()
                .position(|waiter| waiter.thread.id() == thread.id())
                .expect("registered waiter remains until return");
            if memory.waiters[index].state == WaitState::Notified {
                memory.waiters.remove(index);
                break WaitResult::Notified;
            }
            let remaining = match deadline {
                Some((started, duration)) => {
                    let remaining = duration.saturating_sub(started.elapsed());
                    if remaining.is_zero() {
                        memory.waiters.remove(index);
                        break WaitResult::TimedOut;
                    }
                    Some(remaining)
                }
                None => None,
            };
            drop(memory);
            match remaining {
                Some(remaining) => std::thread::park_timeout(remaining),
                None => std::thread::park(),
            }
            memory = self.state();
        };
        // No useful memoized state remains when the last registration completes.
        if memory.waiters.is_empty() {
            memory.waiters = Vec::new();
        }
        Ok(result)
    }

    pub(crate) fn notify(&self, address: u64, count: u32) -> Result<u32, WasmTrap> {
        let _order = memory_order();
        let mut memory = self.state();
        MemoryLoad::I32Load.read_atomic(&memory.bytes, address)?;
        let mut notified = 0;
        for waiter in &mut memory.waiters {
            if notified == count {
                break;
            }
            if waiter.address == address && waiter.state == WaitState::Waiting {
                waiter.state = WaitState::Notified;
                waiter.thread.unpark();
                notified += 1;
            }
        }
        Ok(notified)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    const REGISTRATION_DEADLINE: Duration = Duration::from_secs(2);
    const WORKER_TIMEOUT_NS: i64 = 5_000_000_000;
    const INFINITE_TIMEOUT_NS: i64 = -1;
    const SPURIOUS_WAKE_OBSERVATION: Duration = Duration::from_millis(10);
    const EXPIRING_TIMEOUT_NS: i64 = 1_000_000;

    #[test]
    fn waiter_registration_notification_and_timeout_share_one_lifecycle() {
        let memory = SharedMemory::new(vec![0; 32]);
        let (sender, receiver) = mpsc::channel();
        std::thread::scope(|scope| {
            let handles: Vec<_> = [
                (MemoryLoad::I32Load, WasmValue::I32(0), WORKER_TIMEOUT_NS),
                (MemoryLoad::I64Load, WasmValue::I64(0), INFINITE_TIMEOUT_NS),
            ]
            .into_iter()
            .map(|(load, expected, timeout)| {
                let memory = &memory;
                let sender = sender.clone();
                scope.spawn(move || {
                    sender
                        .send(memory.wait(load, 0, expected, timeout))
                        .unwrap()
                })
            })
            .collect();
            let started = Instant::now();
            let registered = loop {
                if memory.state().waiters.len() == 2 {
                    break true;
                }
                if started.elapsed() >= REGISTRATION_DEADLINE {
                    break false;
                }
                std::thread::yield_now();
            };
            let signals: Vec<_> = memory
                .state()
                .waiters
                .iter()
                .map(|waiter| waiter.thread.clone())
                .collect();
            for signal in &signals {
                signal.unpark();
            }
            let spurious = receiver.recv_timeout(SPURIOUS_WAKE_OBSERVATION);
            // A byte write is not a notification, and waking must not recompare
            // the expected word after the waiter has already registered.
            memory.lock()[0] = 1;
            let changed_word = receiver.recv_timeout(SPURIOUS_WAKE_OBSERVATION);
            let wrong_address = memory.notify(8, u32::MAX);
            let zero_count = memory.notify(0, 0);
            let first_count = memory.notify(0, 1);
            let first_result = receiver.recv_timeout(REGISTRATION_DEADLINE);
            let second_count = memory.notify(0, u32::MAX);
            let second_result = receiver.recv_timeout(REGISTRATION_DEADLINE);
            // Release even a failed infinite-wait test before joining its worker.
            for waiter in &mut memory.state().waiters {
                waiter.state = WaitState::Notified;
                waiter.thread.unpark();
            }
            for handle in handles {
                handle.join().unwrap();
            }
            assert!(registered);
            assert_eq!(spurious, Err(mpsc::RecvTimeoutError::Timeout));
            assert_eq!(changed_word, Err(mpsc::RecvTimeoutError::Timeout));
            assert_eq!(wrong_address, Ok(0));
            assert_eq!(zero_count, Ok(0));
            assert_eq!(first_count, Ok(1));
            assert_eq!(second_count, Ok(1));
            assert_eq!(first_result, Ok(Ok(WaitResult::Notified)));
            assert_eq!(second_result, Ok(Ok(WaitResult::Notified)));
        });
        assert!(memory.state().waiters.is_empty());
        assert_eq!(memory.state().waiters.capacity(), 0);
        memory.lock()[0] = 0;
        assert_eq!(
            memory.wait(
                MemoryLoad::I32Load,
                0,
                WasmValue::I32(0),
                EXPIRING_TIMEOUT_NS
            ),
            Ok(WaitResult::TimedOut)
        );
        assert!(memory.state().waiters.is_empty());
        assert_eq!(memory.state().waiters.capacity(), 0);
    }
}
