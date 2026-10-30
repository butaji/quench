//! Waiter lifecycle belongs to the linear-memory backing, independently of VM frames.

use crate::shared_backing::memory_order;
use super::memory::{MemoryLoad, MemoryStorage, SharedMemory};
use super::{WasmTrap, WasmValue};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub(crate) enum WaitResult {
    Notified = 0,
    NotEqual = 1,
    TimedOut = 2,
}

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
        &self, load: MemoryLoad, address: u64, expected: WasmValue, timeout: i64,
    ) -> Result<WaitResult, WasmTrap> {
        // Validate Wasm alignment/bounds before entering the common waiter protocol.
        // Shared memories only grow, so the checked address remains accessible.
        {
            let _order = memory_order();
            load.read_atomic(&self.lock(), address)?;
        }
        let expected = match expected {
            WasmValue::I32(value) => u64::from(u32::from_ne_bytes(value.to_le_bytes())),
            WasmValue::I64(value) => u64::from_ne_bytes(value.to_le_bytes()),
            _ => unreachable!("validated atomic wait operand"),
        };
        let timeout = (timeout >= 0).then(|| Duration::from_nanos(timeout as u64));
        match self.0.wait(address as usize, load.width(), expected, timeout) {
            Ok(crate::shared_backing::WaitResult::Ok) => Ok(WaitResult::Notified),
            Ok(crate::shared_backing::WaitResult::NotEqual) => Ok(WaitResult::NotEqual),
            Ok(crate::shared_backing::WaitResult::TimedOut) => Ok(WaitResult::TimedOut),
            Ok(crate::shared_backing::WaitResult::Cancelled) => unreachable!("Wasm wait has no cancellation hook"),
            Err(crate::shared_backing::AccessError::Waiters) => Err(WasmTrap::TooManyWaiters),
            Err(_) => Err(WasmTrap::OutOfBoundsMemory),
        }
    }

    pub(crate) fn notify(&self, address: u64, count: u32) -> Result<u32, WasmTrap> {
        {
            let _order = memory_order();
            MemoryLoad::I32Load.read_atomic(&self.lock(), address)?;
        }
        self.0.notify(address as usize, count as usize)
            .map(|count| count as u32).map_err(|_| WasmTrap::OutOfBoundsMemory)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Instant;

    const REGISTRATION_DEADLINE: Duration = Duration::from_secs(2);
    const WORKER_TIMEOUT_NS: i64 = 5_000_000_000;
    const INFINITE_TIMEOUT_NS: i64 = -1;
    const SPURIOUS_WAKE_OBSERVATION: Duration = Duration::from_millis(10);
    const EXPIRING_TIMEOUT_NS: i64 = 1_000_000;

    #[test]
    fn waiter_registration_notification_and_timeout_share_one_lifecycle() {
        let memory = SharedMemory::new(crate::wasm::memory::LinearBytes::heap(vec![0; 32]));
        let (sender, receiver) = mpsc::channel();
        std::thread::scope(|scope| {
            let handles: Vec<_> = [
                (MemoryLoad::I32Load, WasmValue::I32(0), WORKER_TIMEOUT_NS),
                (MemoryLoad::I64Load, WasmValue::I64(0), INFINITE_TIMEOUT_NS),
            ]
            .into_iter()
            .enumerate()
            .map(|(index, (load, expected, timeout))| {
                let memory = &memory;
                let sender = sender.clone();
                scope.spawn(move || {
                    let result = if index == 0 {
                        memory.wait(load, 0, expected, timeout)
                    } else {
                        memory.0.fixed_view().wait(0, load.width(), 0, None).map(|result| match result {
                            crate::shared_backing::WaitResult::Ok => WaitResult::Notified,
                            _ => panic!("the JavaScript waiter must be notified"),
                        }).map_err(|_| WasmTrap::OutOfBoundsMemory)
                    };
                    sender.send(result).unwrap()
                })
            })
            .collect();
            let started = Instant::now();
            let registered = loop {
                if memory.0.waiter_count() == 2 {
                    break true;
                }
                if started.elapsed() >= REGISTRATION_DEADLINE {
                    break false;
                }
                std::thread::yield_now();
            };
            let spurious = receiver.recv_timeout(SPURIOUS_WAKE_OBSERVATION);
            // A byte write is not a notification, and waking must not recompare
            // the expected word after the waiter has already registered.
            memory.lock()[0] = 1;
            let changed_word = receiver.recv_timeout(SPURIOUS_WAKE_OBSERVATION);
            let wrong_address = memory.notify(8, u32::MAX);
            let zero_count = memory.notify(0, 0);
            let first_count = memory.0.fixed_view().notify(0, 1);
            let first_result = receiver.recv_timeout(REGISTRATION_DEADLINE);
            let second_count = memory.notify(0, u32::MAX);
            let second_result = receiver.recv_timeout(REGISTRATION_DEADLINE);
            // Release even a failed infinite-wait test before joining its worker.
            memory.notify(0, u32::MAX).unwrap();
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
        assert_eq!(memory.0.waiter_count(), 0);
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
        assert_eq!(memory.0.waiter_count(), 0);
    }
}
