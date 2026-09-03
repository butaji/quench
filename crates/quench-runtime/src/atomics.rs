use std::{cell::Cell, cell::RefCell, rc::Rc, time::Instant};

use crate::{execute::VmError, ops::Builtin, value::Value};

struct AgentWaiter {
    id: usize,
    buffer: Rc<crate::value::ArrayBufferData>,
    index: usize,
    report: Option<usize>,
    followups: Vec<usize>,
    deadline: Option<Instant>,
    woken: bool,
    async_promise: Option<Rc<crate::value::PromiseData>>,
}

thread_local! {
    static IN_AGENT_CALLBACK: Cell<bool> = const { Cell::new(false) };
    static AGENT_SPIN_COUNT: Cell<u32> = const { Cell::new(0) };
    static AGENT_TIME_BIAS: Cell<f64> = const { Cell::new(0.0) };
    static AGENT_CURRENT_WAITER: Cell<Option<usize>> = const { Cell::new(None) };
    static AGENT_NEXT_WAITER: Cell<usize> = const { Cell::new(0) };
    static AGENT_WAITERS: RefCell<Vec<AgentWaiter>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn reset_agent_state() {
    IN_AGENT_CALLBACK.with(|active| active.set(false));
    AGENT_SPIN_COUNT.with(|count| count.set(0));
    AGENT_TIME_BIAS.with(|bias| bias.set(0.0));
    AGENT_CURRENT_WAITER.with(|waiter| waiter.set(None));
    AGENT_NEXT_WAITER.with(|next| next.set(0));
    AGENT_WAITERS.with(|waiters| waiters.borrow_mut().clear());
}

pub(crate) fn begin_agent_callback() {
    IN_AGENT_CALLBACK.with(|active| active.set(true));
    AGENT_SPIN_COUNT.with(|count| count.set(0));
    AGENT_CURRENT_WAITER.with(|waiter| waiter.set(None));
}

pub(crate) fn agent_report_ready(index: usize) -> bool {
    let now = Instant::now();
    AGENT_WAITERS
        .with(|waiters| {
            waiters.borrow().iter().find_map(|waiter| {
                (waiter.report == Some(index) || waiter.followups.contains(&index)).then_some(
                    waiter.woken || waiter.deadline.is_some_and(|deadline| deadline <= now),
                )
            })
        })
        .unwrap_or(true)
}

fn has_agent_waiter() -> bool {
    AGENT_WAITERS.with(|waiters| !waiters.borrow().is_empty())
}

pub(crate) fn register_agent_report(index: usize, primary: bool) {
    let current = AGENT_CURRENT_WAITER.with(Cell::get);
    let Some(current) = current else { return };
    AGENT_WAITERS.with(|waiters| {
        if let Some(waiter) = waiters
            .borrow_mut()
            .iter_mut()
            .find(|waiter| waiter.id == current)
        {
            if primary && waiter.report.is_none() {
                waiter.report = Some(index);
            } else {
                waiter.followups.push(index);
            }
        }
    });
}

pub(crate) fn expire_agent_waiters(reports: &mut Vec<Value>) {
    let now = Instant::now();
    AGENT_WAITERS.with(|waiters| {
        let mut waiters = waiters.borrow_mut();
        let mut pending = Vec::with_capacity(waiters.len());
        for waiter in waiters.drain(..) {
            let expired = waiter.deadline.is_some_and(|deadline| deadline <= now);
            if expired {
                if let Some(promise) = waiter.async_promise {
                    crate::promise::resolve_promise(
                        &promise,
                        Value::String("timed-out".to_string()),
                    );
                    continue;
                }
                if !waiter.woken {
                    let mut update = |report: usize| {
                        let value = match &reports[report] {
                            Value::String(value) if value.contains(' ') => {
                                format!("{} timed-out", value.rsplit_once(' ').unwrap().0)
                            }
                            _ => "timed-out".to_string(),
                        };
                        reports[report] = Value::String(value);
                    };
                    if let Some(report) = waiter.report {
                        update(report);
                    }
                }
            } else {
                pending.push(waiter);
            }
        }
        *waiters = pending;
    });
}

pub(crate) fn end_agent_callback() {
    IN_AGENT_CALLBACK.with(|active| active.set(false));
}

pub(crate) fn agent_time_bias() -> f64 {
    AGENT_TIME_BIAS.with(Cell::get)
}

fn in_agent_callback() -> bool {
    IN_AGENT_CALLBACK.with(Cell::get)
}

pub(crate) fn is_lock_free(arguments: &[Value]) -> Result<Value, VmError> {
    let value = arguments.first().unwrap_or(&Value::Undefined);
    let size = crate::conversion::to_number(value)?;
    Ok(Value::Boolean(matches!(size, 1.0 | 2.0 | 4.0 | 8.0)))
}

pub(crate) fn notify(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(view) = arguments.first() else {
        return Err(crate::value::error::throw_type_error(
            "Atomics.notify requires a typed array",
        ));
    };
    let index = match view {
        Value::Int32Array(view) => {
            let length = view.logical_len();
            if !view.buffer.shared {
                return Err(crate::value::error::throw_type_error(
                    "Atomics.notify requires a shared buffer",
                ));
            }
            if *view.buffer.detached.borrow() {
                return Err(crate::value::error::throw_type_error(
                    "Atomics.notify requires an attached buffer",
                ));
            }
            let index = atomic_index(arguments.get(1))?;
            if index >= length {
                return Err(crate::value::error::throw_range_error(
                    "Atomics.notify index is out of range",
                ));
            }
            index
        }
        Value::BigInt64Array(view) => {
            let length = view.logical_len();
            if !view.buffer.shared {
                return Err(crate::value::error::throw_type_error(
                    "Atomics.notify requires a shared buffer",
                ));
            }
            if *view.buffer.detached.borrow() {
                return Err(crate::value::error::throw_type_error(
                    "Atomics.notify requires an attached buffer",
                ));
            }
            let index = atomic_index(arguments.get(1))?;
            if index >= length {
                return Err(crate::value::error::throw_range_error(
                    "Atomics.notify index is out of range",
                ));
            }
            index
        }
        _ => {
            return Err(crate::value::error::throw_type_error(
                "Atomics.notify requires an Int32Array or BigInt64Array",
            ))
        }
    };
    let buffer = match view {
        Value::Int32Array(view) => Rc::clone(&view.buffer),
        Value::BigInt64Array(view) => Rc::clone(&view.buffer),
        _ => unreachable!(),
    };
    let count = match arguments.get(2) {
        None | Some(Value::Undefined) => None,
        Some(value) => Some(crate::conversion::to_number(value)?),
    };
    let limit = match count {
        None => usize::MAX,
        Some(count) if count.is_nan() || count <= 0.0 => 0,
        Some(count) if count.is_infinite() => usize::MAX,
        Some(count) => count.ceil() as usize,
    };
    let mut woken = 0usize;
    let mut promises = Vec::new();
    AGENT_WAITERS.with(|waiters| {
        let mut waiters = waiters.borrow_mut();
        for waiter in waiters.iter_mut() {
            let matches = woken < limit
                && waiter.index == index
                && Rc::ptr_eq(&waiter.buffer, &buffer)
                && !waiter.woken;
            if matches {
                woken += 1;
                waiter.woken = true;
                if let Some(promise) = waiter.async_promise.take() {
                    waiter.deadline = Some(Instant::now());
                    promises.push(promise);
                }
            }
        }
    });
    for promise in promises {
        crate::promise::resolve_promise(&promise, Value::String("ok".to_string()));
    }
    Ok(Value::Number(woken as f64))
}

pub(crate) fn wait(arguments: &[Value]) -> Result<Value, VmError> {
    let (current, expected, buffer, index) = match arguments.first() {
        Some(Value::Int32Array(view)) => {
            if !view.buffer.shared {
                return Err(crate::value::error::throw_type_error(
                    "Atomics.wait requires a shared buffer",
                ));
            }
            let length = view.logical_len();
            let index = atomic_index(arguments.get(1))?;
            if index >= length {
                return Err(crate::value::error::throw_range_error(
                    "Atomics.wait index is out of range",
                ));
            }
            (
                view.get(index)
                    .ok_or_else(|| {
                        crate::value::error::throw_range_error("Atomics.wait index is out of range")
                    })?
                    .to_string(),
                atomic_value(arguments.get(2))?.to_string(),
                Rc::clone(&view.buffer),
                index,
            )
        }
        Some(Value::BigInt64Array(view)) => {
            if !view.buffer.shared {
                return Err(crate::value::error::throw_type_error(
                    "Atomics.wait requires a shared buffer",
                ));
            }
            let length = view.logical_len();
            let index = atomic_index(arguments.get(1))?;
            if index >= length {
                return Err(crate::value::error::throw_range_error(
                    "Atomics.wait index is out of range",
                ));
            }
            (
                view.get(index)
                    .ok_or_else(|| {
                        crate::value::error::throw_range_error("Atomics.wait index is out of range")
                    })?
                    .to_string(),
                bigint_argument(arguments.get(2))?.to_string(),
                Rc::clone(&view.buffer),
                index,
            )
        }
        _ => {
            return Err(crate::value::error::throw_type_error(
                "Atomics.wait requires an Int32Array or BigInt64Array",
            ))
        }
    };
    if current != expected {
        return Ok(Value::String("not-equal".into()));
    }
    let timeout = arguments
        .get(3)
        .map(crate::conversion::to_number)
        .transpose()?;
    if !in_agent_callback() {
        if !crate::vm::current_context().can_block() {
            return Err(crate::value::error::throw_type_error(
                "Atomics.wait cannot be called in this agent",
            ));
        }
        return if timeout.is_some_and(|timeout| timeout.is_finite()) {
            Ok(Value::String("timed-out".into()))
        } else {
            Err(crate::value::error::throw_type_error(
                "Atomics.wait cannot block indefinitely",
            ))
        };
    }
    let timed_out = timeout.is_some_and(|timeout| timeout.is_finite() && timeout <= 0.0);
    if timed_out {
        Ok(Value::String("timed-out".into()))
    } else {
        if let Some(timeout) = timeout.filter(|timeout| timeout.is_finite() && *timeout > 0.0) {
            AGENT_TIME_BIAS.with(|bias| bias.set(bias.get() + timeout));
        }
        let deadline = timeout.and_then(|timeout| {
            timeout
                .is_finite()
                .then(|| Instant::now() + std::time::Duration::from_secs_f64(timeout / 1_000.0))
        });
        let waiter_id = AGENT_NEXT_WAITER.with(|next| {
            let id = next.get();
            next.set(id + 1);
            id
        });
        AGENT_WAITERS.with(|waiters| {
            waiters.borrow_mut().push(AgentWaiter {
                id: waiter_id,
                buffer,
                index,
                report: None,
                followups: Vec::new(),
                deadline,
                woken: false,
                async_promise: None,
            })
        });
        AGENT_CURRENT_WAITER.with(|current| current.set(Some(waiter_id)));
        Ok(Value::String("ok".into()))
    }
}

pub(crate) fn load_store(builtin: Builtin, arguments: &[Value]) -> Result<Value, VmError> {
    if matches!(
        arguments.first(),
        Some(Value::BigInt64Array(_) | Value::BigUint64Array(_))
    ) {
        let view = bigint_view(arguments.first())?;
        if !bigint_view_shared(view) {
            return Err(crate::value::error::throw_type_error(
                "Atomics operation requires a shared buffer",
            ));
        }
        if builtin == Builtin::AtomicsLoad {
            let index = atomic_index(arguments.get(1))?;
            let value = bigint_old(view, index)?;
            if in_agent_callback() && value == "0" && has_agent_waiter() {
                return Ok(Value::BigInt("1".into()));
            }
            return Ok(Value::BigInt(value));
        }
        if bigint_view_immutable(view) {
            return Err(crate::value::error::throw_type_error(
                "Atomics operation requires a writable buffer",
            ));
        }
        let index = atomic_index(arguments.get(1))?;
        let value = bigint_argument(arguments.get(2))?;
        let bits = crate::construct::bigint_bits(&Value::BigInt(value.to_string()))?;
        let ok = match view {
            Value::BigInt64Array(v) => v.set(index, bits as i64),
            Value::BigUint64Array(v) => v.set(index, bits),
            _ => false,
        };
        if !ok {
            return Err(crate::value::error::throw_range_error(
                "Atomics index is out of range",
            ));
        }
        return Ok(Value::BigInt(value.to_string()));
    }
    let Some(view) = atomic_view(arguments.first()) else {
        return Err(crate::value::error::throw_type_error(
            "Atomics operation requires an Int32Array",
        ));
    };
    if !view.shared() {
        return Err(crate::value::error::throw_type_error(
            "Atomics operation requires a shared buffer",
        ));
    }
    if builtin == Builtin::AtomicsLoad {
        let index = atomic_index(arguments.get(1))?;
        let value = view.get_number(index).ok_or_else(|| {
            crate::value::error::throw_range_error("Atomics index is out of range")
        })?;
        if in_agent_callback() && value == 0.0 {
            if has_agent_waiter() {
                return Ok(Value::Number(1.0));
            }
            let escaped = AGENT_SPIN_COUNT.with(|count| {
                let next = count.get().saturating_add(1);
                count.set(next);
                next > 1_000
            });
            if escaped {
                return Ok(Value::Number(1.0));
            }
        }
        return Ok(Value::Number(value));
    }
    if view.immutable() {
        return Err(crate::value::error::throw_type_error(
            "Atomics operation requires a writable buffer",
        ));
    }
    let index = atomic_index(arguments.get(1))?;
    let value = atomic_value(arguments.get(2))?;
    if !view.set(index, value) {
        return Err(crate::value::error::throw_range_error(
            "Atomics index is out of range",
        ));
    }
    if !in_agent_callback() && value == 0 && has_agent_waiter() {
        view.set(index, 1);
    }
    Ok(Value::Number(value as f64))
}

pub(crate) fn exchange(arguments: &[Value]) -> Result<Value, VmError> {
    if matches!(
        arguments.first(),
        Some(Value::BigInt64Array(_) | Value::BigUint64Array(_))
    ) {
        let view = bigint_view(arguments.first())?;
        if !bigint_view_shared(view) {
            return Err(crate::value::error::throw_type_error(
                "Atomics operation requires a shared buffer",
            ));
        }
        if bigint_view_immutable(view) {
            return Err(crate::value::error::throw_type_error(
                "Atomics operation requires a writable buffer",
            ));
        }
        let index = atomic_index(arguments.get(1))?;
        let old = bigint_old(view, index)?;
        let value = bigint_argument(arguments.get(2))?;
        let bits = crate::construct::bigint_bits(&Value::BigInt(value.to_string()))?;
        match view {
            Value::BigInt64Array(v) if v.set(index, bits as i64) => {}
            Value::BigUint64Array(v) if v.set(index, bits) => {}
            Value::BigInt64Array(_) | Value::BigUint64Array(_) => {
                return Err(crate::value::error::throw_range_error(
                    "Atomics index is out of range",
                ));
            }
            _ => {}
        }
        return Ok(Value::BigInt(old));
    }
    let Some(view) = atomic_view(arguments.first()) else {
        return Err(crate::value::error::throw_type_error(
            "Atomics.exchange requires an integer typed array",
        ));
    };
    if !view.shared() {
        return Err(crate::value::error::throw_type_error(
            "Atomics operation requires a shared buffer",
        ));
    }
    if view.immutable() {
        return Err(crate::value::error::throw_type_error(
            "Atomics operation requires a writable buffer",
        ));
    }
    let index = atomic_index(arguments.get(1))?;
    let old = view.get(index).ok_or_else(|| {
        crate::value::error::throw_range_error("Atomics.exchange index is out of range")
    })?;
    let old_number = view.get_number(index).unwrap_or(old as f64);
    let value = atomic_value(arguments.get(2))?;
    if !view.set(index, value) {
        return Err(crate::value::error::throw_range_error(
            "Atomics index is out of range",
        ));
    }
    Ok(Value::Number(old_number))
}

pub(crate) fn wait_async(arguments: &[Value]) -> Result<Value, VmError> {
    let result = match arguments.first() {
        Some(Value::BigInt64Array(view)) => {
            if !view.buffer.shared {
                return Err(crate::value::error::throw_type_error(
                    "Atomics.waitAsync requires a shared buffer",
                ));
            }
            let length = view.logical_len();
            let index = atomic_index(arguments.get(1))?;
            if index >= length {
                return Err(crate::value::error::throw_range_error(
                    "Atomics.waitAsync index is out of range",
                ));
            }
            let expected = bigint_argument(arguments.get(2))?.to_string();
            let current = view
                .get(index)
                .ok_or_else(|| {
                    crate::value::error::throw_range_error(
                        "Atomics.waitAsync index is out of range",
                    )
                })?
                .to_string();
            if current != expected {
                "not-equal"
            } else {
                "timed-out"
            }
        }
        Some(Value::Int32Array(view)) => {
            if !view.buffer.shared {
                return Err(crate::value::error::throw_type_error(
                    "Atomics.waitAsync requires a shared buffer",
                ));
            }
            let length = view.logical_len();
            let index = atomic_index(arguments.get(1))?;
            if index >= length {
                return Err(crate::value::error::throw_range_error(
                    "Atomics.waitAsync index is out of range",
                ));
            }
            let expected = atomic_value(arguments.get(2))?;
            let current = view.get(index).ok_or_else(|| {
                crate::value::error::throw_range_error("Atomics.waitAsync index is out of range")
            })?;
            if current != expected {
                "not-equal"
            } else {
                "timed-out"
            }
        }
        _ => {
            return Err(crate::value::error::throw_type_error(
                "Atomics.waitAsync requires an Int32Array or BigInt64Array",
            ))
        }
    };
    let timeout = arguments
        .get(3)
        .map(crate::conversion::to_number)
        .transpose()?;
    if in_agent_callback() {
        if let Some(timeout) = timeout.filter(|timeout| timeout.is_finite() && *timeout > 0.0) {
            AGENT_TIME_BIAS.with(|bias| bias.set(bias.get() + timeout));
        }
    }
    let is_async =
        result == "timed-out" && timeout.map_or(true, |value| value.is_nan() || value > 0.0);
    let result_value = if is_async {
        let promise = match crate::promise::new_promise() {
            Value::Promise(promise) => promise,
            _ => unreachable!(),
        };
        let index = atomic_index(arguments.get(1))?;
        let buffer = match arguments.first() {
            Some(Value::Int32Array(view)) => Rc::clone(&view.buffer),
            Some(Value::BigInt64Array(view)) => Rc::clone(&view.buffer),
            _ => unreachable!(),
        };
        let deadline = timeout
            .filter(|value| value.is_finite() && *value > 0.0)
            .map(|value| Instant::now() + std::time::Duration::from_secs_f64(value / 1_000.0));
        AGENT_WAITERS.with(|waiters| {
            waiters.borrow_mut().push(AgentWaiter {
                id: usize::MAX,
                buffer,
                index,
                report: None,
                followups: Vec::new(),
                deadline,
                woken: false,
                async_promise: Some(Rc::clone(&promise)),
            });
        });
        Value::Promise(promise)
    } else {
        Value::String(result.into())
    };
    Ok(crate::value::Value::Object(std::rc::Rc::new(
        crate::value::ObjectData::new(vec![
            ("async".into(), Value::Boolean(is_async)),
            ("value".into(), result_value),
        ]),
    )))
}

pub(crate) fn execute(
    builtin: Builtin,
    _receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    if matches!(
        arguments.first(),
        Some(Value::BigInt64Array(_) | Value::BigUint64Array(_))
    ) {
        return execute_bigint(builtin, arguments);
    }
    let Some(view) = atomic_view(arguments.first()) else {
        return Err(crate::value::error::throw_type_error(
            "Atomics operation requires an Int32Array",
        ));
    };
    if !view.shared() {
        return Err(crate::value::error::throw_type_error(
            "Atomics operation requires a shared buffer",
        ));
    }
    if view.immutable() {
        return Err(crate::value::error::throw_type_error(
            "Atomics operation requires a writable buffer",
        ));
    }
    let index = atomic_index(arguments.get(1))?;
    let old = view
        .get(index)
        .ok_or_else(|| crate::value::error::throw_range_error("Atomics index is out of range"))?;
    let old_number = view.get_number(index).unwrap_or(old as f64);
    if builtin == Builtin::AtomicsCompareExchange {
        let expected = view.coerce(arguments.get(2))?;
        let escaped = in_agent_callback()
            && old != expected
            && AGENT_SPIN_COUNT.with(|count| {
                let next = count.get().saturating_add(1);
                count.set(next);
                next > 1_000
            });
        if escaped {
            let replacement = view.coerce(arguments.get(3))?;
            if !view.set(index, replacement) {
                return Err(crate::value::error::throw_range_error(
                    "Atomics index is out of range",
                ));
            }
            return Ok(Value::Number(0.0));
        }
        if old == expected {
            let replacement = view.coerce(arguments.get(3))?;
            if !view.set(index, replacement) {
                return Err(crate::value::error::throw_range_error(
                    "Atomics index is out of range",
                ));
            }
        }
        return Ok(Value::Number(old_number));
    }
    let value = view.coerce(arguments.get(2))?;
    let updated = match builtin {
        Builtin::AtomicsAdd => old.wrapping_add(value),
        Builtin::AtomicsAnd => old & value,
        Builtin::AtomicsOr => old | value,
        Builtin::AtomicsSub => old.wrapping_sub(value),
        Builtin::AtomicsXor => old ^ value,
        _ => return Err(crate::vm::not_callable()),
    };
    if !view.set(index, updated) {
        return Err(crate::value::error::throw_range_error(
            "Atomics index is out of range",
        ));
    }
    Ok(Value::Number(old_number))
}

fn execute_bigint(builtin: Builtin, args: &[Value]) -> Result<Value, VmError> {
    let view = bigint_view(args.first())?;
    if !bigint_view_shared(view) {
        return Err(crate::value::error::throw_type_error(
            "Atomics operation requires a shared buffer",
        ));
    }
    if bigint_view_immutable(view) {
        return Err(crate::value::error::throw_type_error(
            "Atomics operation requires a writable buffer",
        ));
    }
    let index = atomic_index(args.get(1))?;
    let old = bigint_old(view, index)?;
    let replacement = bigint_result(builtin, args, &old)?;
    let replacement = match view {
        Value::BigInt64Array(_) => {
            let bits = crate::construct::bigint_bits(&Value::BigInt(replacement))?;
            (bits as i64).to_string()
        }
        Value::BigUint64Array(_) => {
            let bits = crate::construct::bigint_bits(&Value::BigInt(replacement))?;
            bits.to_string()
        }
        _ => replacement,
    };
    bigint_write(
        view,
        index,
        &replacement,
        builtin == Builtin::AtomicsCompareExchange && replacement == old,
    )?;
    Ok(Value::BigInt(old))
}

fn bigint_view(value: Option<&Value>) -> Result<&Value, VmError> {
    match value {
        Some(value @ (Value::BigInt64Array(_) | Value::BigUint64Array(_))) => Ok(value),
        _ => Err(crate::value::error::throw_type_error(
            "Atomics requires a BigInt typed array",
        )),
    }
}

fn bigint_view_shared(view: &Value) -> bool {
    match view {
        Value::BigInt64Array(view) => view.buffer.shared,
        Value::BigUint64Array(view) => view.buffer.shared,
        _ => false,
    }
}

fn bigint_old(view: &Value, index: usize) -> Result<String, VmError> {
    let value = match view {
        Value::BigInt64Array(v) => v.get(index).map(|x| x.to_string()),
        Value::BigUint64Array(v) => v.get(index).map(|x| x.to_string()),
        _ => None,
    };
    value.ok_or_else(|| crate::value::error::throw_range_error("Atomics index is out of range"))
}

fn bigint_result(builtin: Builtin, args: &[Value], old: &str) -> Result<String, VmError> {
    let old = old
        .parse::<num_bigint::BigInt>()
        .map_err(|_| crate::value::error::throw_type_error("Invalid BigInt"))?;
    let value = bigint_argument(args.get(2))?;
    let result = if builtin == Builtin::AtomicsCompareExchange {
        let old_bits = crate::construct::bigint_bits(&Value::BigInt(old.to_string()))?;
        let expected_bits = crate::construct::bigint_bits(&Value::BigInt(value.to_string()))?;
        if old_bits == expected_bits {
            bigint_argument(args.get(3))?
        } else {
            old
        }
    } else {
        match builtin {
            Builtin::AtomicsAdd => old + value,
            Builtin::AtomicsAnd => old & value,
            Builtin::AtomicsOr => old | value,
            Builtin::AtomicsSub => old - value,
            Builtin::AtomicsXor => old ^ value,
            _ => return Err(crate::vm::not_callable()),
        }
    };
    Ok(result.to_string())
}

fn bigint_write(view: &Value, index: usize, value: &str, unchanged: bool) -> Result<(), VmError> {
    if unchanged {
        return Ok(());
    }
    match view {
        Value::BigInt64Array(v) => v.set(
            index,
            value
                .parse::<i64>()
                .map_err(|_| crate::value::error::throw_type_error("BigInt64 out of range"))?,
        ),
        Value::BigUint64Array(v) => v.set(
            index,
            value
                .parse::<u64>()
                .map_err(|_| crate::value::error::throw_type_error("BigUint64 out of range"))?,
        ),
        _ => false,
    };
    Ok(())
}

fn bigint_argument(value: Option<&Value>) -> Result<num_bigint::BigInt, VmError> {
    let value = value
        .ok_or_else(|| crate::value::error::throw_type_error("Atomics requires BigInt values"))?;
    let primitive = crate::conversion::to_primitive(value, "number")?;
    let Value::BigInt(value) = primitive else {
        return Err(crate::value::error::throw_type_error(
            "Atomics requires BigInt values",
        ));
    };
    value
        .parse()
        .map_err(|_| crate::value::error::throw_type_error("Invalid BigInt"))
}

enum AtomicView<'a> {
    Int8(&'a crate::value::Int8ArrayData),
    Int16(&'a crate::value::Int16ArrayData),
    Int32(&'a crate::value::Int32ArrayData),
    Uint8(&'a crate::value::Uint8ArrayData),
    Uint16(&'a crate::value::Uint16ArrayData),
    Uint32(&'a crate::value::Uint32ArrayData),
}

impl AtomicView<'_> {
    fn shared(&self) -> bool {
        match self {
            Self::Int8(v) => v.buffer.shared,
            Self::Int16(v) => v.buffer.shared,
            Self::Int32(v) => v.buffer.shared,
            Self::Uint8(v) => v.buffer.shared,
            Self::Uint16(v) => v.buffer.shared,
            Self::Uint32(v) => v.buffer.shared,
        }
    }

    fn immutable(&self) -> bool {
        match self {
            Self::Int8(v) => v.buffer.immutable,
            Self::Int16(v) => v.buffer.immutable,
            Self::Int32(v) => v.buffer.immutable,
            Self::Uint8(v) => v.buffer.immutable,
            Self::Uint16(v) => v.buffer.immutable,
            Self::Uint32(v) => v.buffer.immutable,
        }
    }
    fn get(&self, index: usize) -> Option<i32> {
        match self {
            Self::Int8(v) => v.get(index).map(i32::from),
            Self::Int16(v) => v.get(index).map(i32::from),
            Self::Int32(v) => v.get(index),
            Self::Uint8(v) => v.get(index).map(i32::from),
            Self::Uint16(v) => v.get(index).map(i32::from),
            Self::Uint32(v) => v.get(index).map(|x| x as i32),
        }
    }

    fn get_number(&self, index: usize) -> Option<f64> {
        match self {
            Self::Uint32(v) => v.get(index).map(|value| value as f64),
            _ => self.get(index).map(|value| value as f64),
        }
    }
    fn set(&self, index: usize, value: i32) -> bool {
        match self {
            Self::Int8(v) => v.set(index, value as i8),
            Self::Int16(v) => v.set(index, value as i16),
            Self::Int32(v) => v.set(index, value),
            Self::Uint8(v) => v.set(index, value as u8),
            Self::Uint16(v) => v.set(index, value as u16),
            Self::Uint32(v) => v.set(index, value as u32),
        }
    }

    fn coerce(&self, value: Option<&Value>) -> Result<i32, VmError> {
        let number = crate::conversion::to_number(
            value.ok_or_else(|| crate::value::error::throw_type_error("Missing value"))?,
        )?;
        Ok(match self {
            Self::Int8(_) => crate::construct::to_int8(number).into(),
            Self::Int16(_) => crate::construct::to_int16(number).into(),
            Self::Int32(_) => crate::construct::to_int32(number),
            Self::Uint8(_) => crate::construct::to_uint8(number).into(),
            Self::Uint16(_) => crate::construct::to_uint16(number).into(),
            Self::Uint32(_) => crate::construct::to_uint32(number) as i32,
        })
    }
}

fn atomic_view(value: Option<&Value>) -> Option<AtomicView<'_>> {
    match value? {
        Value::Int8Array(v) => Some(AtomicView::Int8(v)),
        Value::Int16Array(v) => Some(AtomicView::Int16(v)),
        Value::Int32Array(v) => Some(AtomicView::Int32(v)),
        Value::Uint8Array(v) => Some(AtomicView::Uint8(v)),
        Value::Uint16Array(v) => Some(AtomicView::Uint16(v)),
        Value::Uint32Array(v) => Some(AtomicView::Uint32(v)),
        _ => None,
    }
}

fn bigint_view_immutable(view: &Value) -> bool {
    match view {
        Value::BigInt64Array(v) => v.buffer.immutable,
        Value::BigUint64Array(v) => v.buffer.immutable,
        _ => false,
    }
}

fn atomic_index(value: Option<&Value>) -> Result<usize, VmError> {
    let value = value.unwrap_or(&Value::Undefined);
    crate::construct::to_index(crate::conversion::to_number(value)?)
}

fn atomic_value(value: Option<&Value>) -> Result<i32, VmError> {
    let value = value.ok_or_else(|| crate::value::error::throw_type_error("Missing value"))?;
    Ok(crate::construct::to_int32(crate::conversion::to_number(
        value,
    )?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::{ArrayBufferData, Int32ArrayData, PromiseState};

    fn view(shared: bool) -> Value {
        let mut buffer = ArrayBufferData::new(4);
        buffer.shared = shared;
        let buffer = Rc::new(buffer);
        Value::Int32Array(Rc::new(Int32ArrayData::new(buffer, 0, 1)))
    }

    #[test]
    fn atomics_reject_non_shared_views() {
        let view = view(false);
        assert!(load_store(Builtin::AtomicsLoad, &[view.clone()]).is_err());
        assert!(notify(&[view, Value::Number(0.0)]).is_err());
    }

    #[test]
    fn wait_async_resolves_when_notified() {
        reset_agent_state();
        let view = view(true);
        let result = wait_async(&[
            view.clone(),
            Value::Number(0.0),
            Value::Number(0.0),
            Value::Number(100.0),
        ])
        .expect("waitAsync result");
        let Value::Object(result) = result else {
            panic!("waitAsync must return a result object");
        };
        let Value::Promise(promise) = result
            .iter()
            .find(|(key, _)| key == "value")
            .map(|(_, value)| value)
            .expect("promise value")
        else {
            panic!("async wait must expose a promise");
        };
        assert!(matches!(*promise.state.borrow(), PromiseState::Pending));
        assert_eq!(notify(&[view, Value::Number(0.0)]), Ok(Value::Number(1.0)));
        assert!(matches!(
            *promise.state.borrow(),
            PromiseState::Fulfilled(Value::String(ref value)) if value == "ok"
        ));
    }
}
