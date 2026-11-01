//! Shared `perf_hooks` projection for APIs used by Node packages.
//!
//! `performance` keeps Node's buffered user-timing entries: marks and
//! measures stay in its timeline until cleared.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};


pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    // The module keeps its time origin, the process's own.
    crate::host::startup_snapshot::refuse_process_state(context, "performance.timeOrigin")?;
    let factory = crate::host::removed_javascript("node:perf_hooks.js")?;
    let now = context.host_function(crate::host::shared_vm::operation("perfHooksNow"))?;
    let undefined = context.undefined();
    let data = context.null_object_rooted()?;
    let global = context.global_root()?;
    let array = super::event_constructor_shared_vm::get(context, global, "Array")?;
    let prototype = super::event_constructor_shared_vm::get(context, array, "prototype")?;
    let sort = super::event_constructor_shared_vm::get(context, prototype, "sort")?;
    let compare =
        context.host_function(crate::host::shared_vm::operation("perfHooksCompareEntries"))?;
    let coerce = crate::host::removed_javascript("node:perf_hooks/coerce.js")?;
    for (name, value) in [("sort", sort), ("compare", compare), ("coerce", coerce)] {
        let key = context.string_rooted(name);
        context.define_data_property_rooted(data, key, value, false, false, false)?;
    }
    let query = context
        .host_function_with_data(crate::host::shared_vm::operation("perfHooksEntries"), data)?;
    let user_timing = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "perf_hooks.js",
        "internal/perf/usertiming.js",
    )?;
    let resource_timing = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "perf_hooks.js",
        "internal/perf/resource_timing.js",
    )?;
    let observe = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "perf_hooks.js",
        "internal/perf/observe.js",
    )?;
    let node_timing = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "perf_hooks.js",
        "internal/perf/nodetiming.js",
    )?;
    let event_loop = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "perf_hooks.js",
        "internal/perf/event_loop_utilization.js",
    )?;
    let key = context.string_rooted("eventLoopUtilization");
    let event_loop_utilization = context.get_property_rooted(event_loop, key)?;
    let performance_impl = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "perf_hooks.js",
        "internal/perf/performance.js",
    )?;
    let performance_class_key = context.string_rooted("Performance");
    let performance_class = context.get_property_rooted(performance_impl, performance_class_key)?;
    let binding = super::performance_binding_shared_vm::binding(context)?;
    let time_origin_ms = context
        .host_mut()
        .shared_state()
        .borrow()
        .process_control
        .performance_origins()
        .map(|(_, unix_micros)| unix_micros / 1_000.0)
        .ok_or_else(|| RootedError::host("Performance time origin unavailable"))?;
    let time_origin = context.number(time_origin_ms);
    let constants_key = context.string_rooted("constants");
    let constants = context.get_property_rooted(binding, constants_key)?;
    let entry_types = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "perf_hooks.js",
        "internal/perf/performance_entry.js",
    )?;
    let create_entry_key = context.string_rooted("createPerformanceNodeEntry");
    let create_entry = context.get_property_rooted(entry_types, create_entry_key)?;
    let enqueue_key = context.string_rooted("enqueue");
    let enqueue = context.get_property_rooted(observe, enqueue_key)?;
    let mut internal = Vec::new();
    for (module, name) in [
        (resource_timing, "markResourceTiming"),
        (user_timing, "mark"),
        (user_timing, "measure"),
        (user_timing, "clearMarkTimings"),
        (observe, "clearEntriesFromBuffer"),
    ] {
        let key = context.string_rooted(name);
        internal.push(context.get_property_rooted(module, key)?);
    }
    let mark_as_uncloneable =
        context.host_constructor(crate::host::shared_vm::operation("markAsUncloneable"))?;
    let module = context.call_rooted(
        factory,
        undefined,
        &[
            time_origin,
            now,
            query,
            internal[0],
            internal[1],
            internal[2],
            internal[3],
            internal[4],
            node_timing,
            event_loop_utilization,
            create_entry,
            enqueue,
            mark_as_uncloneable,
        ],
    )?;
    let performance = super::event_constructor_shared_vm::get(context, module, "performance")?;
    let timerify = super::event_constructor_shared_vm::get(context, module, "timerify")?;
    let performance_timerify = context.string_rooted("timerify");
    context.define_data_property_rooted(
        performance,
        performance_timerify,
        timerify,
        true,
        true,
        true,
    )?;
    let key = context.string_rooted("timerify");
    context.define_data_property_rooted(module, key, timerify, true, true, true)?;
    let key = context.string_rooted("Performance");
    context.define_data_property_rooted(module, key, performance_class, true, true, true)?;
    let key = context.string_rooted("constants");
    context.define_data_property_rooted(module, key, constants, true, true, true)?;
    let brand = context.symbol_rooted(Some("performance"));
    let yes = context.boolean(true);
    context.define_data_property_rooted(performance, brand, yes, true, true, true)?;
    let key = context.string_rooted("brand");
    context.define_data_property_rooted(data, key, brand, false, false, false)?;
    // The entry and observer types come from the configured SDK policy, which owns observer
    // registration, buffering and dispatch; they are defined when a script first observes one.
    let types = context.host_function_with_data(
        crate::host::shared_vm::operation("perfHooksObserversInitialize"), module,
    )?;
    let names = PUBLIC_TYPES.map(|(_, name)| name);
    context.define_lazy_data_properties_rooted(module, &names, true, types)?;
    let key = context.string_rooted("eventLoopUtilization");
    context.define_data_property_rooted(module, key, event_loop_utilization, true, true, true)?;
    Ok(module)
}

/// The types `perf_hooks` exports, each with the `internal/perf` module defining it.
const PUBLIC_TYPES: [(&str, &str); 6] = [
    ("internal/perf/performance_entry.js", "PerformanceEntry"),
    ("internal/perf/usertiming.js", "PerformanceMark"),
    ("internal/perf/usertiming.js", "PerformanceMeasure"),
    ("internal/perf/resource_timing.js", "PerformanceResourceTiming"),
    ("internal/perf/observe.js", "PerformanceObserver"),
    ("internal/perf/observe.js", "PerformanceObserverEntryList"),
];

/// Defines the public types on `perf_hooks` (the lazy group `module` defines).
pub(crate) fn initialize_observers(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let module = context.host_function_data()?;
    for (library, name) in PUBLIC_TYPES {
        let source = crate::host::shared_vm::commonjs::node_library_module(context, "perf_hooks.js", library)?;
        let key = context.string_rooted(name);
        let value = context.get_property_rooted(source, key)?;
        context.define_data_property_rooted(module, key, value, true, true, true)?;
    }
    Ok(context.undefined())
}

pub(crate) fn now(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let elapsed_ms = context
        .host_mut()
        .shared_state()
        .borrow()
        .process_control
        .performance_elapsed_ms();
    Ok(context.number(elapsed_ms))
}
