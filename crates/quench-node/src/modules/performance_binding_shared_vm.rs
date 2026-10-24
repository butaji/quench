//! Genuine performance observer host state; original SDK owns observer policy.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError, Runtime, Value};
include!("performance_constants.rs");
type Context<'a> = NativeContext<'a, NodeHost>;
#[derive(Default)]
pub(crate) struct State {
    counts: Option<RootId>,
    milestones: Option<RootId>,
    uv_metrics: Option<RootId>,
    uv_metrics_bigint: Option<RootId>,
    callback: Option<RootId>,
}
fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(object, key, value, true, true, true)?;
    Ok(())
}
fn unsupported<T>(c: &mut Context<'_>, message: &str) -> Result<T, RootedError> {
    let error = c.error_rooted(message)?;
    super::node_errors_shared_vm::decorate_error(c, error, "ERR_NOT_IMPLEMENTED")?;
    Err(c.throw(error))
}
fn milestone(label: &str) -> usize {
    MILESTONES
        .iter()
        .find(|row| row.0 == label)
        .expect("SDK milestone owner")
        .1
}
fn entry(label: &str) -> Option<usize> {
    ENTRIES.iter().find(|row| row.0 == label).map(|row| row.1)
}
fn typed_array(c: &mut Context<'_>, name: &str, length: usize) -> Result<RootId, RootedError> {
    c.with_startup_intrinsics(|c| {
        let global = c.global_root()?;
        let key = c.string_rooted(name);
        let constructor = c.get_property_rooted(global, key)?;
        let length = c.number(length as f64);
        c.construct_rooted(constructor, constructor, &[length])
    })
}
pub(crate) fn binding(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(c, "\0binding:performance", build)
}
fn build(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let module = c.object_rooted()?;
    let constants = c.object_rooted()?;
    for &(name, value, enumerable) in CONSTANTS {
        let key = c.string_rooted(name);
        let value = c.number(value as f64);
        c.define_data_property_rooted(constants, key, value, false, enumerable, false)?;
    }
    let key = c.string_rooted("constants");
    c.define_data_property_rooted(module, key, constants, false, true, false)?;
    let counts = typed_array(c, "Uint32Array", ENTRIES.len())?;
    put(c, module, "observerCounts", counts)?;
    // Keep Node's public libuv metrics buffers present even on runtimes where
    // native loop counters are unavailable. Their zero initialization matches
    // the pre-event-loop snapshot and remains safe for number/bigint readers.
    let uv_metrics = typed_array(c, "Float64Array", 3)?;
    put(c, module, "uvMetricsBuffer", uv_metrics)?;
    let uv_metrics_bigint = typed_array(c, "BigUint64Array", 3)?;
    put(c, module, "uvMetricsBigIntBuffer", uv_metrics_bigint)?;
    let milestones = typed_array(c, "Float64Array", MILESTONES.len())?;
    for &(_, index) in MILESTONES {
        let key = c.string_rooted(&index.to_string());
        let unknown = c.number(-1.0);
        c.set_property_rooted(milestones, key, unknown, milestones)?;
    }
    let shared = c.host_mut().shared_state();
    let process_control = shared.borrow().process_control.clone();
    let origins = process_control.performance_origins();
    let Some((monotonic_ns, unix_micros)) = origins else {
        return unsupported(
            c,
            "Performance time origin requires native monotonic and wall clocks",
        );
    };
    for (label, value) in [
        ("timeOrigin", monotonic_ns),
        ("timeOriginTimestamp", unix_micros),
        ("nodeStart", monotonic_ns),
        ("v8Start", monotonic_ns + 1_000_000.0),
        ("environment", monotonic_ns + 2_000_000.0),
        ("bootstrapComplete", monotonic_ns + 3_000_000.0),
    ] {
        let key = c.string_rooted(&milestone(label).to_string());
        let value = c.number(value);
        c.set_property_rooted(milestones, key, value, milestones)?;
    }
    put(c, module, "milestones", milestones)?;
    let retained_counts = c.retain(counts)?;
    let retained_milestones = match c.retain(milestones) {
        Ok(root) => root,
        Err(error) => {
            c.release_root(retained_counts);
            return Err(error);
        }
    };
    let retained_uv_metrics = c.retain(uv_metrics)?;
    let retained_uv_metrics_bigint = c.retain(uv_metrics_bigint)?;
    {
        let mut state = shared.borrow_mut();
        state.performance_native.counts = Some(retained_counts);
        state.performance_native.milestones = Some(retained_milestones);
        state.performance_native.uv_metrics = Some(retained_uv_metrics);
        state.performance_native.uv_metrics_bigint = Some(retained_uv_metrics_bigint);
    }
    for (name, operation) in [
        ("uvMetricsInfo", "performanceUvMetricsInfo"),
        ("now", "performanceNativeNow"),
        ("loopIdleTime", "performanceLoopIdleTime"),
        ("setupObservers", "performanceSetupObservers"),
        ("updateGarbageCollectionTracking", "performanceGcTracking"),
        ("notify", "performanceNotify"),
        ("markBootstrapComplete", "performanceBootstrapMark"),
    ] {
        let function = c.host_function_named(crate::host::shared_vm::operation(operation), name)?;
        put(c, module, name, function)?;
    }
    Ok(module)
}
pub(crate) fn now(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    {
        let undefined = c.undefined();
        super::perf_hooks::now(c, undefined, &[])
    }
}

pub(crate) fn uv_metrics_info(
    c: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let state = c.host_mut().shared_state();
    let (number_buffer, bigint_buffer, process_control) = {
        let shared = state.borrow();
        (
            shared.performance_native.uv_metrics,
            shared.performance_native.uv_metrics_bigint,
            shared.process_control.clone(),
        )
    };
    let number_buffer = number_buffer
        .ok_or_else(|| RootedError::host("libuv metrics buffer is not initialized"))?;
    let bigint_buffer = bigint_buffer
        .ok_or_else(|| RootedError::host("libuv bigint metrics buffer is not initialized"))?;
    let count = process_control.event_loop_iterations();
    for (index, value) in [(0, count), (1, count), (2, 0)] {
        let key = c.string_rooted(&index.to_string());
        let number = c.number(value as f64);
        c.set_property_rooted(number_buffer, key, number, number_buffer)?;
        let key = c.string_rooted(&index.to_string());
        let bigint = c.bigint_rooted(value as i128);
        c.set_property_rooted(bigint_buffer, key, bigint, bigint_buffer)?;
    }
    Ok(c.undefined())
}

pub(crate) fn loop_idle_time(
    c: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let shared = c.host_mut().shared_state();
    let control = shared.borrow().process_control.clone();
    let idle_ms = control.event_loop_idle_ms();
    Ok(c.number(idle_ms))
}
pub(crate) fn setup(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let callback = args.first().copied().unwrap_or_else(|| c.undefined());
    if !c.is_callable_rooted(callback)? {
        let error = c.type_error_rooted("Performance observer callback must be a function")?;
        return Err(c.throw(error));
    }
    let retained = c.retain(callback)?;
    let shared = c.host_mut().shared_state();
    let previous = shared
        .borrow_mut()
        .performance_native
        .callback
        .replace(retained);
    if let Some(previous) = previous {
        c.release_root(previous);
    }
    Ok(c.undefined())
}
fn count(c: &mut Context<'_>, index: usize) -> Result<u32, RootedError> {
    let shared = c.host_mut().shared_state();
    let counts = shared
        .borrow()
        .performance_native
        .counts
        .ok_or_else(|| RootedError::host("Performance observer storage not initialized"))?;
    let key = c.string_rooted(&index.to_string());
    let value = c.get_property_rooted(counts, key)?;
    c.rooted_value(value)
        .and_then(Value::as_number)
        .map(|value| value as u32)
        .ok_or_else(|| RootedError::host("Performance observer storage is detached or unavailable"))
}
pub(crate) fn gc_tracking(
    c: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    // Quench currently has no runtime GC callback API. The exposed gc() path
    // reports a forced collection to observers after collection completes.
    Ok(c.undefined())
}

/// Report an explicit `global.gc()` call to Node's existing PerformanceObserver
/// callback. This is the narrow compatibility path until runtime GC hooks exist.
pub(crate) fn notify_forced_gc(c: &mut Context<'_>, duration_ms: f64) -> Result<(), RootedError> {
    // Workers may expose global.gc() before Node's perf_hooks binding has
    // initialized the observer counters. With no counters there can be no GC
    // observer, so notification is a no-op.
    if c.host_mut()
        .shared_state()
        .borrow()
        .performance_native
        .counts
        .is_none()
    {
        return Ok(());
    }
    if count(c, entry("gc").expect("SDK GC slot"))? == 0 {
        return Ok(());
    }
    let shared = c.host_mut().shared_state();
    let callback = shared
        .borrow()
        .performance_native
        .callback
        .ok_or_else(|| RootedError::host("Performance observer callback not registered"))?;
    let name = c.string_rooted("gc");
    let entry_type = c.string_rooted("gc");
    let undefined = c.undefined();
    let start_time = super::perf_hooks::now(c, undefined, &[])?;
    let duration = c.number(duration_ms.max(0.0));
    let detail = c.object_rooted()?;
    let kind_key = c.string_rooted("kind");
    let kind = c.number(4.0); // NODE_PERFORMANCE_GC_MAJOR
    c.define_data_property_rooted(detail, kind_key, kind, true, true, true)?;
    let flags_key = c.string_rooted("flags");
    let flags = c.number(4.0); // NODE_PERFORMANCE_GC_FLAGS_FORCED
    c.define_data_property_rooted(detail, flags_key, flags, true, true, true)?;
    let undefined = c.undefined();
    c.call_rooted(
        callback,
        undefined,
        &[name, entry_type, start_time, duration, detail],
    )?;
    Ok(())
}
pub(crate) fn notify(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let type_root = args.first().copied().unwrap_or_else(|| c.undefined());
    let name = c.to_string(type_root)?;
    if let Some(index) = entry(&name) {
        if count(c, index)? > 0 {
            let shared = c.host_mut().shared_state();
            let callback =
                shared.borrow().performance_native.callback.ok_or_else(|| {
                    RootedError::host("Performance observer callback not registered")
                })?;
            let value = args.get(1).copied().unwrap_or_else(|| c.undefined());
            let undefined = c.undefined();
            c.call_rooted(callback, undefined, &[value])?;
        }
    }
    Ok(c.undefined())
}
pub(crate) fn bootstrap_mark(
    c: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let shared = c.host_mut().shared_state();
    let state = shared.borrow();
    let milestones = state
        .performance_native
        .milestones
        .ok_or_else(|| RootedError::host("Performance milestones not initialized"))?;
    let (monotonic, _) = state
        .process_control
        .performance_origins()
        .ok_or_else(|| RootedError::host("Monotonic time origin unavailable"))?;
    let value =
        (monotonic + state.process_control.uptime() * 1_000_000_000.0).max(monotonic + 3_000_000.0);
    drop(state);
    let key = c.string_rooted(&milestone("bootstrapComplete").to_string());
    let value = c.number(value);
    c.set_property_rooted(milestones, key, value, milestones)?;
    Ok(c.undefined())
}
pub(crate) fn cleanup(runtime: &mut Runtime<NodeHost>) {
    let shared = runtime.host_mut().shared_state();
    let state = std::mem::take(&mut shared.borrow_mut().performance_native);
    for root in [
        state.counts,
        state.milestones,
        state.uv_metrics,
        state.uv_metrics_bigint,
        state.callback,
    ]
    .into_iter()
    .flatten()
    {
        runtime.release_root(root);
    }
}

pub(crate) fn mark_loop_start(runtime: &mut Runtime<NodeHost>) -> Result<(), String> {
    let shared = runtime.host_mut().shared_state();
    let (control, milestones) = {
        let state = shared.borrow();
        (
            state.process_control.clone(),
            state.performance_native.milestones,
        )
    };
    control.start_event_loop();
    let (Some(start), Some(milestones)) = (control.loop_start_ns(), milestones) else {
        return Ok(());
    };
    let key = runtime.root(Value::number(milestone("loopStart") as f64));
    let value = runtime.root(Value::number(start));
    let result = runtime.set_property_rooted(milestones, key, value, milestones);
    runtime.release_root(key);
    runtime.release_root(value);
    match result {
        Ok(true) => Ok(()),
        Ok(false) => Err("Performance loopStart milestone is not writable".into()),
        Err(error) => Err(error.to_string()),
    }
}

pub(crate) fn mark_loop_exit(runtime: &mut Runtime<NodeHost>) -> Result<(), String> {
    let shared = runtime.host_mut().shared_state();
    let (control, milestones, origin) = {
        let state = shared.borrow();
        let Some((origin, _)) = state.process_control.performance_origins() else {
            return Ok(());
        };
        (
            state.process_control.clone(),
            state.performance_native.milestones,
            origin,
        )
    };
    let Some(milestones) = milestones else {
        return Ok(());
    };
    let value = control
        .monotonic_now_ns()
        .unwrap_or_else(|| origin + control.uptime() * 1_000_000_000.0);
    let key = runtime.string_rooted(&milestone("loopExit").to_string());
    let value = runtime.root(Value::number(value));
    runtime
        .set_property_rooted(milestones, key, value, milestones)
        .map_err(|error| error.to_string())?;
    Ok(())
}
