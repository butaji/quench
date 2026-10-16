//! `Atomics` on SharedArrayBuffers whose data blocks agents on other threads may hold
//! (hosts that share memory across threads). Every operation on such a block runs inside
//! one process-wide critical section, which also guards the waiter lists: ECMAScript's
//! WaiterList critical section. Operands are converted before entering it, so no guest
//! code ever runs while it is held.

use super::*;
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Agents suspended in `Atomics.wait`, by the address of the element they wait on.
struct WaiterLists(BTreeMap<usize, VecDeque<Arc<Waiter>>>);

struct Waiter {
    notified: AtomicBool,
    wake: Condvar,
}

static CRITICAL_SECTION: Mutex<WaiterLists> = Mutex::new(WaiterLists(BTreeMap::new()));

fn enter_critical_section() -> MutexGuard<'static, WaiterLists> {
    CRITICAL_SECTION.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The milliseconds `Atomics.wait` may suspend: NaN waits forever, negatives not at all.
fn wait_duration(timeout: f64) -> Option<Duration> {
    if timeout.is_nan() || timeout == f64::INFINITY {
        return None;
    }
    const MILLISECONDS_PER_SECOND: f64 = 1000.0;
    Some(Duration::from_secs_f64(timeout.max(0.0) / MILLISECONDS_PER_SECOND))
}

impl<H: Host> Vm<H> {
    /// Whether `view` is a typed array over a data block other threads may share.
    pub(super) fn is_shared_block_view(&self, view: Value) -> bool {
        let Some(Cell::TypedArray { buffer, .. }) = self.heap.get(view) else {
            return false;
        };
        matches!(self.heap.get(*buffer), Some(Cell::ArrayBuffer { bytes, .. }) if bytes.concurrent_block().is_some())
    }

    /// The address of element `index` of a shared-block view, which identifies its waiters.
    fn shared_element_address(&self, view: Value, index: usize) -> Option<usize> {
        let Some(Cell::TypedArray { buffer, object, kind, .. }) = self.heap.get(view) else {
            return None;
        };
        let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get(*buffer) else {
            return None;
        };
        let offset = object.offset + index * kind.width();
        Some(bytes.concurrent_block()?.address(offset) as usize)
    }

    pub(super) fn shared_atomics(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let view = args.first().copied().unwrap_or(Value::UNDEFINED);
        let kind = self.atomic_array_kind(p, view)?;
        let waits = matches!(native, Native::AtomicsWait | Native::AtomicsWaitAsync);
        if (waits || native == Native::AtomicsNotify)
            && !matches!(kind, TypedArrayKind::Int32 | TypedArrayKind::BigInt64)
        {
            let message = if waits {
                "Atomics wait requires shared Int32Array or BigInt64Array"
            } else {
                "Atomics.notify requires Int32Array or BigInt64Array"
            };
            return Err(self.type_error(p, message.into()));
        }
        let index = self.atomic_index(p, args.get(1).copied())?;
        let length = self.typed_array_length(view).unwrap_or(0);
        self.atomic_validate_index(p, length, index)?;
        match native {
            Native::AtomicsNotify => self.shared_notify(p, view, index, args.get(2).copied()),
            Native::AtomicsWait => self.shared_wait(p, kind, view, index, args),
            // Waiting asynchronously needs the host's event loop; Node-hosted agents
            // reach it through the general path, which settles only on this thread.
            Native::AtomicsWaitAsync => self.atomic_wait(p, native, args),
            _ => self.shared_read_modify_write(p, native, kind, view, index, args),
        }
    }

    /// The operand as the element type stores it: a Number, or a BigInt reduced modulo 2^64.
    fn shared_operand(
        &mut self,
        p: &ResidualProgram,
        kind: TypedArrayKind,
        value: Option<Value>,
    ) -> Result<Value, JsError> {
        let value = value.unwrap_or(Value::UNDEFINED);
        if matches!(kind, TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64) {
            let value = self.to_bigint(p, value)?;
            Ok(self.heap.alloc(Cell::BigInt(value.to_string())))
        } else {
            let number = self.to_number(p, value)?;
            Ok(Value::number(if number.is_nan() { 0.0 } else { number.trunc() + 0.0 }))
        }
    }

    fn shared_bigint(&self, value: Value) -> num_bigint::BigInt {
        match self.heap.get(value) {
            Some(Cell::BigInt(text)) => text.parse().unwrap_or_default(),
            _ => num_bigint::BigInt::default(),
        }
    }

    /// Whether two elements of `kind` hold the same stored value.
    fn shared_same_element(&self, kind: TypedArrayKind, left: Value, right: Value) -> bool {
        if matches!(kind, TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64) {
            self.normalize_atomic_bigint(self.shared_bigint(left))
                == self.normalize_atomic_bigint(self.shared_bigint(right))
        } else {
            let number = |value: Value| value.as_number().unwrap_or(0.0);
            self.atomic_number_value(kind, number(left)) == self.atomic_number_value(kind, number(right))
        }
    }

    fn shared_read_modify_write(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        kind: TypedArrayKind,
        view: Value,
        index: usize,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let operands = match native {
            Native::AtomicsLoad => Vec::new(),
            Native::AtomicsCompareExchange => vec![
                self.shared_operand(p, kind, args.get(2).copied())?,
                self.shared_operand(p, kind, args.get(3).copied())?,
            ],
            _ => vec![self.shared_operand(p, kind, args.get(2).copied())?],
        };
        // Conversions may run guest code that detaches or shrinks the buffer.
        self.atomic_array_kind(p, view)?;
        if native != Native::AtomicsLoad {
            self.atomic_require_writable(p, view)?;
        }
        let length = self.typed_array_length(view).unwrap_or(0);
        self.atomic_validate_index(p, length, index)?;
        let bigint = matches!(kind, TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64);
        let _section = enter_critical_section();
        let old = self.typed_array_get(view, index).unwrap_or(Value::UNDEFINED);
        let next = match native {
            Native::AtomicsLoad => None,
            Native::AtomicsStore | Native::AtomicsExchange => Some(operands[0]),
            Native::AtomicsCompareExchange => {
                self.shared_same_element(kind, old, operands[0]).then_some(operands[1])
            }
            _ if bigint => {
                let (old, input) = (self.shared_bigint(old), self.shared_bigint(operands[0]));
                let next = match native {
                    Native::AtomicsAdd => old + input,
                    Native::AtomicsSub => old - input,
                    Native::AtomicsAnd => old & input,
                    Native::AtomicsOr => old | input,
                    Native::AtomicsXor => old ^ input,
                    _ => unreachable!("atomic read-modify-write operator"),
                };
                Some(self.heap.alloc(Cell::BigInt(next.to_string())))
            }
            _ => {
                let old = old.as_number().unwrap_or(0.0);
                let input = self.atomic_number_value(kind, operands[0].as_number().unwrap_or(0.0));
                let bits = |value| Self::atomic_number_bits(kind, value);
                Some(Value::number(match native {
                    Native::AtomicsAdd => old + input,
                    Native::AtomicsSub => old - input,
                    Native::AtomicsAnd => (bits(old) & bits(input)) as f64,
                    Native::AtomicsOr => (bits(old) | bits(input)) as f64,
                    Native::AtomicsXor => (bits(old) ^ bits(input)) as f64,
                    _ => unreachable!("atomic read-modify-write operator"),
                }))
            }
        };
        if let Some(next) = next {
            self.typed_array_set(p, view, index, next)?;
        }
        // Store returns its converted operand; the others return the previous element.
        Ok(if native == Native::AtomicsStore { operands[0] } else { old })
    }

    fn shared_wait(
        &mut self,
        p: &ResidualProgram,
        kind: TypedArrayKind,
        view: Value,
        index: usize,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let expected = self.shared_operand(p, kind, args.get(2).copied())?;
        let timeout = self.to_number(p, args.get(3).copied().unwrap_or(Value::UNDEFINED))?;
        if !self.host.can_block() {
            return Err(self.type_error(p, "Atomics.wait cannot block in this agent".into()));
        }
        let address = self
            .shared_element_address(view, index)
            .ok_or_else(|| self.type_error(p, "Atomics.wait requires a shared buffer".into()))?;
        let deadline = wait_duration(timeout).map(|duration| Instant::now() + duration);
        let mut section = enter_critical_section();
        let current = self.typed_array_get(view, index).unwrap_or(Value::UNDEFINED);
        let outcome = if !self.shared_same_element(kind, current, expected) {
            "not-equal"
        } else {
            let waiter = Arc::new(Waiter { notified: AtomicBool::new(false), wake: Condvar::new() });
            section.0.entry(address).or_default().push_back(waiter.clone());
            loop {
                if waiter.notified.load(Ordering::Acquire) {
                    break "ok";
                }
                let remaining = deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()));
                if remaining.is_some_and(|remaining| remaining.is_zero()) {
                    if let Some(list) = section.0.get_mut(&address) {
                        list.retain(|candidate| !Arc::ptr_eq(candidate, &waiter));
                        if list.is_empty() {
                            section.0.remove(&address);
                        }
                    }
                    break "timed-out";
                }
                section = match remaining {
                    Some(remaining) => waiter
                        .wake
                        .wait_timeout(section, remaining)
                        .map_or_else(|poisoned| poisoned.into_inner().0, |(guard, _)| guard),
                    None => waiter.wake.wait(section).unwrap_or_else(|poisoned| poisoned.into_inner()),
                };
            }
        };
        drop(section);
        Ok(self.heap.alloc(Cell::String(outcome.into())))
    }

    fn shared_notify(
        &mut self,
        p: &ResidualProgram,
        view: Value,
        index: usize,
        count: Option<Value>,
    ) -> Result<Value, JsError> {
        let limit = match count {
            None | Some(Value::UNDEFINED) => usize::MAX,
            Some(value) => {
                let count = self.to_number(p, value)?;
                if count.is_nan() || count <= 0.0 {
                    0
                } else if count.is_infinite() {
                    usize::MAX
                } else {
                    count.trunc() as usize
                }
            }
        };
        let Some(address) = self.shared_element_address(view, index) else {
            return Ok(Value::number(0.0));
        };
        let mut section = enter_critical_section();
        let mut woken = 0usize;
        if let Some(list) = section.0.get_mut(&address) {
            while woken < limit {
                let Some(waiter) = list.pop_front() else { break };
                waiter.notified.store(true, Ordering::Release);
                waiter.wake.notify_one();
                woken += 1;
            }
            if list.is_empty() {
                section.0.remove(&address);
            }
        }
        Ok(Value::number(woken as f64))
    }
}
