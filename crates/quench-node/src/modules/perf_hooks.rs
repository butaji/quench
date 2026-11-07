//! `perf_hooks` (`lib/perf_hooks.js`, `lib/internal/perf/*`): the `performance` object, its
//! Node timing and event loop utilization, `timerify`, and the module's exports.
//!
//! The entry timeline and observers live in `perf_timeline`, histograms in
//! `perf_histogram`; this module assembles them and keeps the group's shared data
//! (`api`): private slot keys, the classes' prototypes and the intrinsics they use.
use super::event_target_shared_vm::{argument, get, is_nullish, is_undefined, missing_args, put};
use crate::host::NodeHost;
use quench_runtime::{DescriptorFields, NativeContext, RootId, RootedError};

pub(super) type Context<'a> = NativeContext<'a, NodeHost>;
pub(super) type Outcome = Result<RootId, RootedError>;

/// A class member: (name, operation, length).
pub(super) type Method = (&'static str, &'static str, f64);
/// A getter: (name, operation).
pub(super) type Getter = (&'static str, &'static str);

/// The members of the group's shared data.
pub(super) mod api {
    /// Private keys: a performance entry's fields, a histogram's state, an observer's
    /// callback and observation kind, an entry list's entries, the `performance` brand.
    pub(in crate::modules) const ENTRY_SLOT: &str = "entrySlot";
    pub(in crate::modules) const HISTOGRAM_SLOT: &str = "histogramSlot";
    pub(in crate::modules) const OBSERVER_SLOT: &str = "observerSlot";
    pub(in crate::modules) const LIST_SLOT: &str = "listSlot";
    pub(in crate::modules) const PERFORMANCE_SLOT: &str = "performanceSlot";
    pub(crate) use crate::modules::keys::INSPECT;
    pub(crate) use crate::modules::keys::MAP;
    pub(crate) use crate::modules::keys::MAP_SET;
    pub(in crate::modules) const MAP_CLEAR: &str = "mapClear";
    pub(in crate::modules) const OBJECT_ASSIGN: &str = "objectAssign";
    pub(in crate::modules) const OBJECT_FREEZE: &str = "objectFreeze";
    pub(in crate::modules) const PERFORMANCE: &str = "performance";
    pub(in crate::modules) const ENTRY_PROTOTYPE: &str = "entryPrototype";
    pub(in crate::modules) const NODE_ENTRY_PROTOTYPE: &str = "nodeEntryPrototype";
    pub(in crate::modules) const MARK_PROTOTYPE: &str = "markPrototype";
    pub(in crate::modules) const MEASURE_PROTOTYPE: &str = "measurePrototype";
    pub(in crate::modules) const RESOURCE_PROTOTYPE: &str = "resourcePrototype";
    pub(in crate::modules) const LIST_PROTOTYPE: &str = "listPrototype";
    pub(in crate::modules) const SUPPORTED_ENTRY_TYPES: &str = "supportedEntryTypes";
    pub(in crate::modules) const RECORDABLE_CLASS: &str = "recordableClass";
    pub(in crate::modules) const RECORDABLE_PROTOTYPE: &str = "recordablePrototype";
    pub(in crate::modules) const ELD_PROTOTYPE: &str = "eldPrototype";
}

const PRIVATE_SLOTS: [&str; 5] =
    [api::ENTRY_SLOT, api::HISTOGRAM_SLOT, api::OBSERVER_SLOT, api::LIST_SLOT, api::PERFORMANCE_SLOT];

/// `Performance.prototype`'s web members, each enumerable, in Node's order.
const PERFORMANCE_METHODS: [Method; 11] = [
    ("clearMarks", "perfClearMarks", 0.0),
    ("clearMeasures", "perfClearMeasures", 0.0),
    ("clearResourceTimings", "perfClearResourceTimings", 0.0),
    ("getEntries", "perfGetEntries", 0.0),
    ("getEntriesByName", "perfGetEntriesByName", 1.0),
    ("getEntriesByType", "perfGetEntriesByType", 1.0),
    ("mark", "perfMark", 1.0),
    ("measure", "perfMeasure", 1.0),
    ("now", "perfNow", 0.0),
    ("setResourceTimingBufferSize", "perfSetResourceTimingBufferSize", 1.0),
    ("toJSON", "perfToJson", 0.0),
];
/// The members before `timeOrigin` in `Performance.prototype`'s key order.
const MEMBERS_BEFORE_TIME_ORIGIN: usize = 10;
/// Node's extensions on `Performance.prototype`, not enumerable.
const PERFORMANCE_EXTENSIONS: [Method; 3] = [
    ("eventLoopUtilization", "perfEventLoopUtilization", 2.0),
    ("markResourceTiming", "perfMarkResourceTiming", 7.0),
    ("timerify", "perfTimerify", 1.0),
];

/// The extensions before `nodeTiming` in `Performance.prototype`'s key order.
const EXTENSIONS_BEFORE_NODE_TIMING: usize = 1;

/// `performance.nodeTiming`'s milestone getters, each named after its milestone; marks may
/// not take these names, and measures read them as the milestones.
pub(super) const NODE_TIMING_MILESTONES: [&str; 6] = ["nodeStart", "v8Start", "environment", "loopStart", "loopExit", "bootstrapComplete"];
/// `nodeTiming.toJSON()`'s fields after its entry fields, in Node's order.
const NODE_TIMING_JSON: [&str; 7] = ["nodeStart", "v8Start", "bootstrapComplete", "environment", "loopStart", "loopExit", "idleTime"];
const NODE_TIMING_NAME: &str = "node";

/// The nanoseconds of a millisecond and of a second, as Node converts clock readings.
pub(super) const NANOS_PER_MILLI: f64 = 1e6;
const NANOS_PER_SECOND: f64 = 1e9;
const MILLIS_PER_SECOND: f64 = 1e3;
/// The microseconds of a millisecond, as `timeOrigin` converts the origin timestamp.
const MICROS_PER_MILLI: f64 = 1e3;
/// A milestone not yet reached.
const UNREACHED_MILESTONE: f64 = -1.0;
/// `setResourceTimingBufferSize`'s WebIDL `unsigned long`.
const UNSIGNED_LONG_MODULUS: f64 = 4_294_967_296.0;

pub(super) fn operation(name: &str) -> quench_runtime::HostFunctionId {
    crate::host::shared_vm::operation(name)
}

/// The group's shared data, building the module on first use.
pub(super) fn shared_api(c: &mut Context<'_>) -> Outcome {
    let api = c.host_mut().shared_state().borrow().performance_native.api;
    if let Some(api) = api {
        return Ok(api);
    }
    crate::host::shared_vm::commonjs::perf_hooks_module(c)?;
    c.host_mut()
        .shared_state()
        .borrow()
        .performance_native
        .api
        .ok_or_else(|| RootedError::host("perf_hooks data missing"))
}

pub(super) fn api_member(c: &mut Context<'_>, name: &str) -> Outcome {
    let api = shared_api(c)?;
    get(c, api, name)
}

/// The private slot `slot` of `object`, when it carries one.
pub(super) fn slot(c: &mut Context<'_>, object: RootId, slot: &str) -> Result<Option<RootId>, RootedError> {
    if !c.is_object_rooted(object)? && !c.is_callable_rooted(object)? {
        return Ok(None);
    }
    let key = api_member(c, slot)?;
    let value = c.get_property_rooted(object, key)?;
    Ok((!is_undefined(c, value)).then_some(value))
}

pub(super) fn attach_slot(c: &mut Context<'_>, object: RootId, slot: &str, value: RootId) -> Result<(), RootedError> {
    let key = api_member(c, slot)?;
    c.define_data_property_rooted(object, key, value, true, false, true)?;
    Ok(())
}

/// The slot `slot` of the receiver, or `ERR_INVALID_THIS` naming `class`.
pub(super) fn receiver_slot(c: &mut Context<'_>, receiver: RootId, slot_name: &str, class: &str) -> Outcome {
    match slot(c, receiver, slot_name)? {
        Some(value) => Ok(value),
        None => Err(invalid_this(c, class)),
    }
}

pub(super) fn invalid_this(c: &mut Context<'_>, class: &str) -> RootedError {
    super::event_target_shared_vm::invalid_this(c, &format!("Value of \"this\" must be of type {class}"))
}

/// A coded error of `class` (`TypeError` or `RangeError`), thrown.
pub(super) fn coded_error(c: &mut Context<'_>, range: bool, code: &'static str, message: &str) -> RootedError {
    let made = if range { c.range_error_rooted(message) } else { c.type_error_rooted(message) };
    let error = match made {
        Ok(error) => error,
        Err(error) => return error,
    };
    let decorated = if range {
        super::node_errors_shared_vm::decorate_range_error(c, error, code)
    } else {
        super::node_errors_shared_vm::decorate_type_error(c, error, code)
    };
    match decorated {
        Ok(()) => c.throw(error),
        Err(error) => error,
    }
}

pub(super) fn illegal_constructor(c: &mut Context<'_>) -> RootedError {
    coded_error(c, false, "ERR_ILLEGAL_CONSTRUCTOR", "Illegal constructor")
}

/// `ERR_OUT_OF_RANGE`: `The value of "<name>" is out of range. It must be <requirement>.`
pub(super) fn out_of_range(c: &mut Context<'_>, name: &str, requirement: &str, value: RootId) -> RootedError {
    let received = match super::util_shared_vm::range_error_received(c, value) {
        Ok(received) => received,
        Err(error) => return error,
    };
    let message = format!("The value of \"{name}\" is out of range. It must be {requirement}. Received {received}");
    coded_error(c, true, "ERR_OUT_OF_RANGE", &message)
}

pub(super) fn type_error_arg(c: &mut Context<'_>, value: RootId, name: &str, expected: &str) -> RootedError {
    match super::util_shared_vm::argument_type_error(c, value, name, expected) {
        Ok(error) | Err(error) => error,
    }
}

/// `validateObject(value, name)`: an object, not null, an array or a function.
pub(super) fn validate_object(c: &mut Context<'_>, value: RootId, name: &str) -> Result<(), RootedError> {
    if c.is_object_rooted(value)? && !is_nullish(c, value) && !c.is_array_rooted(value)? && !c.is_callable_rooted(value)? {
        return Ok(());
    }
    Err(type_error_arg(c, value, name, "Object"))
}

/// `validateNumber(value, name)`.
pub(super) fn validate_number(c: &mut Context<'_>, value: RootId, name: &str) -> Result<f64, RootedError> {
    match c.rooted_value(value).and_then(|value| value.as_number()) {
        Some(number) => Ok(number),
        None => Err(type_error_arg(c, value, name, "number")),
    }
}

pub(super) fn validate_function(c: &mut Context<'_>, value: RootId, name: &str) -> Result<(), RootedError> {
    if c.is_callable_rooted(value)? {
        return Ok(());
    }
    Err(type_error_arg(c, value, name, "function"))
}

/// `${value}`, a symbol throwing.
pub(super) fn to_text(c: &mut Context<'_>, value: RootId) -> Result<String, RootedError> {
    let text = super::event_target_shared_vm::to_dom_string(c, value)?;
    c.to_string(text)
}

pub(super) fn number_of(c: &Context<'_>, value: RootId) -> Option<f64> {
    c.rooted_value(value).and_then(|value| value.as_number())
}

pub(super) fn call_method(c: &mut Context<'_>, receiver: RootId, name: &str, args: &[RootId]) -> Outcome {
    let method = get(c, receiver, name)?;
    c.call_rooted(method, receiver, args)
}

pub(super) fn define_methods(c: &mut Context<'_>, holder: RootId, methods: &[Method], enumerable: bool) -> Result<(), RootedError> {
    for &(name, op, length) in methods {
        let function = c.host_function_named(operation(op), name)?;
        let length = c.number(length);
        c.define_named_data_property_rooted(function, "length", length, false, false, true)?;
        c.define_named_data_property_rooted(holder, name, function, true, enumerable, true)?;
    }
    Ok(())
}

pub(super) fn define_getter(c: &mut Context<'_>, holder: RootId, name: &str, op: &str, data: RootId, enumerable: bool) -> Result<(), RootedError> {
    let getter = c.host_function_named_with_data(operation(op), &format!("get {name}"), data)?;
    let key = c.string_rooted(name);
    let undefined = c.undefined();
    let fields = DescriptorFields {
        get: Some(getter),
        set: Some(undefined),
        enumerable: Some(enumerable),
        configurable: Some(true),
        ..DescriptorFields::default()
    };
    c.define_property_fields_rooted(holder, key, fields)?;
    Ok(())
}

/// `[Symbol.toStringTag]`: `tag`, configurable only.
pub(super) fn define_tag(c: &mut Context<'_>, holder: RootId, tag: &str) -> Result<(), RootedError> {
    let global = c.global_root()?;
    let symbol = intrinsic(c, global, "Symbol")?;
    let key = intrinsic(c, symbol, "toStringTag")?;
    let tag = c.string_rooted(tag);
    c.define_data_property_rooted(holder, key, tag, false, false, true)?;
    Ok(())
}

/// `[util.inspect.custom]`: operation `op` with the class's data.
pub(super) fn define_inspect(c: &mut Context<'_>, holder: RootId, op: &str) -> Result<(), RootedError> {
    let key = api_member(c, api::INSPECT)?;
    let function = c.host_function_named(operation(op), "[nodejs.util.inspect.custom]")?;
    c.define_data_property_rooted(holder, key, function, true, false, true)?;
    Ok(())
}

/// A class as a declaration makes it, extending `parent` when given: a read-only `prototype`
/// and the parent class as its own prototype.
pub(super) fn class(c: &mut Context<'_>, name: &str, constructor: &str, length: f64, parent: Option<RootId>) -> Outcome {
    let class = c.host_constructor_named(operation(constructor), name)?;
    let length = c.number(length);
    c.define_named_data_property_rooted(class, "length", length, false, false, true)?;
    let prototype = get(c, class, "prototype")?;
    c.define_named_data_property_rooted(class, "prototype", prototype, false, false, false)?;
    if let Some(parent) = parent {
        c.set_prototype_rooted(class, parent)?;
        let parent_prototype = get(c, parent, "prototype")?;
        c.set_prototype_rooted(prototype, parent_prototype)?;
    }
    Ok(class)
}

pub(super) fn intrinsic(c: &mut Context<'_>, holder: RootId, name: &str) -> Outcome {
    match c.startup_data_property_rooted(holder, name) {
        Some(value) => Ok(value),
        None => get(c, holder, name),
    }
}

/// Node's custom inspection of a host object: `<label> <inspect(projection)>` one level
/// deeper, the receiver itself below depth zero.
pub(super) fn inspect_as(c: &mut Context<'_>, receiver: RootId, args: &[RootId], label: &str, projection: impl FnOnce(&mut Context<'_>) -> Outcome) -> Outcome {
    let depth = argument(c, args, 0);
    if number_of(c, depth).is_some_and(|depth| depth < 0.0) {
        return Ok(receiver);
    }
    let options = argument(c, args, 1);
    let copied = c.object_rooted()?;
    if c.is_object_rooted(options)? {
        let assign = api_member(c, api::OBJECT_ASSIGN)?;
        let undefined = c.undefined();
        c.call_rooted(assign, undefined, &[copied, options])?;
        let depth = get(c, options, "depth")?;
        let deeper = if is_nullish(c, depth) {
            c.null()
        } else {
            let depth = c.to_number(depth)?;
            c.number(depth - 1.0)
        };
        put(c, copied, "depth", deeper)?;
    }
    let projection = projection(c)?;
    let inspect = super::util_shared_vm::internal_inspect(c)?;
    let undefined = c.undefined();
    let text = c.call_rooted(inspect, undefined, &[projection, copied])?;
    let text = c.to_string(text)?;
    Ok(c.string_rooted(&format!("{label} {text}")))
}

/// A plain object of `fields`, in order.
pub(super) fn plain_object(c: &mut Context<'_>, fields: &[(&str, RootId)]) -> Outcome {
    let object = c.object_rooted()?;
    for &(name, value) in fields {
        put(c, object, name, value)?;
    }
    Ok(object)
}

/// The monotonic milliseconds since the time origin.
pub(super) fn elapsed_ms(c: &mut Context<'_>) -> f64 {
    c.host_mut().shared_state().borrow().process_control.performance_elapsed_ms()
}

pub(crate) fn now(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let elapsed = elapsed_ms(c);
    Ok(c.number(elapsed))
}

/// Milestone `label` in milliseconds after the time origin; -1 when not reached.
pub(super) fn milestone_ms(c: &mut Context<'_>, label: &str) -> Result<f64, RootedError> {
    let ns = super::performance_binding_shared_vm::milestone_ns(c, label)?;
    if ns == UNREACHED_MILESTONE {
        return Ok(ns);
    }
    let origin = super::performance_binding_shared_vm::milestone_ns(c, "timeOrigin")?;
    Ok(ns / NANOS_PER_MILLI - origin / NANOS_PER_MILLI)
}

/// The module's exports, in Node's order.
pub(crate) fn module(c: &mut Context<'_>) -> Outcome {
    // The module keeps its time origin, the process's own.
    crate::host::startup_snapshot::refuse_process_state(c, "performance.timeOrigin")?;
    let api = c.null_object_rooted()?;
    for name in PRIVATE_SLOTS {
        let key = c.private_symbol_rooted(Some(name));
        put(c, api, name, key)?;
    }
    let global = c.global_root()?;
    let symbol = intrinsic(c, global, "Symbol")?;
    let symbol_for = intrinsic(c, symbol, "for")?;
    let inspect_name = c.string_rooted("nodejs.util.inspect.custom");
    let inspect = c.call_rooted(symbol_for, symbol, &[inspect_name])?;
    put(c, api, api::INSPECT, inspect)?;
    let map = intrinsic(c, global, "Map")?;
    put(c, api, api::MAP, map)?;
    let map_prototype = intrinsic(c, map, "prototype")?;
    for (member, name) in [(api::MAP_SET, "set"), (api::MAP_CLEAR, "clear")] {
        let method = intrinsic(c, map_prototype, name)?;
        put(c, api, member, method)?;
    }
    let object = intrinsic(c, global, "Object")?;
    for (member, name) in [(api::OBJECT_ASSIGN, "assign"), (api::OBJECT_FREEZE, "freeze")] {
        let method = intrinsic(c, object, name)?;
        put(c, api, member, method)?;
    }
    let retained = c.retain(api)?;
    c.host_mut().shared_state().borrow_mut().performance_native.api = Some(retained);

    let binding = super::performance_binding_shared_vm::binding(c)?;
    let constants = get(c, binding, "constants")?;
    let types = super::perf_timeline::build(c)?;
    let performance_class = performance_class(c, global)?;
    let performance = create_performance(c, performance_class)?;
    put(c, api, api::PERFORMANCE, performance)?;
    let (create_histogram, monitor) = super::perf_histogram::build(c)?;

    let module = c.object_rooted()?;
    put(c, module, "Performance", performance_class)?;
    for (name, value) in types {
        put(c, module, name, value)?;
    }
    put(c, module, "monitorEventLoopDelay", monitor)?;
    put(c, module, "createHistogram", create_histogram)?;
    put(c, module, "performance", performance)?;
    c.define_named_data_property_rooted(module, "constants", constants, false, true, false)?;
    Ok(module)
}

fn performance_class(c: &mut Context<'_>, global: RootId) -> Outcome {
    let event_target = intrinsic(c, global, "EventTarget")?;
    let class = class(c, "Performance", "perfConstructor", 0.0, Some(event_target))?;
    let prototype = get(c, class, "prototype")?;
    let (before, after) = PERFORMANCE_METHODS.split_at(MEMBERS_BEFORE_TIME_ORIGIN);
    define_methods(c, prototype, before, true)?;
    let undefined = c.undefined();
    define_getter(c, prototype, "timeOrigin", "perfTimeOrigin", undefined, true)?;
    define_methods(c, prototype, after, true)?;
    define_tag(c, prototype, "Performance")?;
    let (utilization, resource_and_timerify) = PERFORMANCE_EXTENSIONS.split_at(EXTENSIONS_BEFORE_NODE_TIMING);
    define_methods(c, prototype, utilization, false)?;
    let node_timing = node_timing(c)?;
    c.define_named_data_property_rooted(prototype, "nodeTiming", node_timing, true, false, true)?;
    define_methods(c, prototype, resource_and_timerify, false)?;
    let key = c.string_rooted(BUFFER_FULL_HANDLER);
    let getter = c.host_function_named(crate::host::shared_vm::operation!("perfGetBufferFullHandler"), &format!("get {BUFFER_FULL_HANDLER}"))?;
    let setter = c.host_function_named(crate::host::shared_vm::operation!("perfSetBufferFullHandler"), &format!("set {BUFFER_FULL_HANDLER}"))?;
    let fields = DescriptorFields { get: Some(getter), set: Some(setter), enumerable: Some(true), configurable: Some(true), ..DescriptorFields::default() };
    c.define_property_fields_rooted(prototype, key, fields)?;
    define_inspect(c, prototype, "perfInspect")?;
    Ok(class)
}

/// `performance`: an EventTarget carrying the brand the methods check, a record holding its
/// event handler attribute.
fn create_performance(c: &mut Context<'_>, class: RootId) -> Outcome {
    let performance = c.object_rooted()?;
    let prototype = get(c, class, "prototype")?;
    c.set_prototype_rooted(performance, prototype)?;
    let global = c.global_root()?;
    let event_target = intrinsic(c, global, "EventTarget")?;
    let maximum = get(c, event_target, "defaultMaxListeners")?;
    super::event_target_shared_vm::initialize_target(c, performance, maximum)?;
    let state = c.null_object_rooted()?;
    attach_slot(c, performance, api::PERFORMANCE_SLOT, state)?;
    Ok(performance)
}

pub(crate) fn performance_constructor(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    Err(illegal_constructor(c))
}

fn brand(c: &mut Context<'_>, receiver: RootId) -> Result<(), RootedError> {
    receiver_slot(c, receiver, api::PERFORMANCE_SLOT, "Performance").map(drop)
}

/// A method's required argument: `ERR_MISSING_ARGS` naming it when there is none.
fn required(c: &mut Context<'_>, args: &[RootId], name: &str) -> Outcome {
    match args.first() {
        Some(&value) => Ok(value),
        None => Err(missing_args(c, &[name])),
    }
}

/// An optional name: undefined, or `${name}`.
fn optional_text(c: &mut Context<'_>, value: RootId) -> Result<Option<String>, RootedError> {
    if is_undefined(c, value) {
        return Ok(None);
    }
    to_text(c, value).map(Some)
}

pub(crate) fn clear_marks(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    brand(c, receiver)?;
    let name = argument(c, args, 0);
    let name = optional_text(c, name)?;
    super::perf_timeline::clear_mark_timings(c, name.as_deref())?;
    super::perf_timeline::clear_buffer(c, super::perf_timeline::Buffer::Mark, name.as_deref());
    Ok(c.undefined())
}

pub(crate) fn clear_measures(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    clear(c, receiver, args, super::perf_timeline::Buffer::Measure)
}

pub(crate) fn clear_resource_timings(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    clear(c, receiver, args, super::perf_timeline::Buffer::Resource)
}

fn clear(c: &mut Context<'_>, receiver: RootId, args: &[RootId], buffer: super::perf_timeline::Buffer) -> Outcome {
    brand(c, receiver)?;
    let name = argument(c, args, 0);
    let name = optional_text(c, name)?;
    super::perf_timeline::clear_buffer(c, buffer, name.as_deref());
    Ok(c.undefined())
}

pub(crate) fn get_entries(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Outcome {
    brand(c, receiver)?;
    super::perf_timeline::buffered_entries(c, None, None)
}

pub(crate) fn get_entries_by_name(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    brand(c, receiver)?;
    let name = required(c, args, "name")?;
    let name = to_text(c, name)?;
    let kind = argument(c, args, 1);
    let kind = optional_text(c, kind)?;
    super::perf_timeline::buffered_entries(c, Some(&name), kind.as_deref())
}

pub(crate) fn get_entries_by_type(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    brand(c, receiver)?;
    let kind = required(c, args, "type")?;
    let kind = to_text(c, kind)?;
    super::perf_timeline::buffered_entries(c, None, Some(&kind))
}

pub(crate) fn mark(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    brand(c, receiver)?;
    let name = required(c, args, "name")?;
    let options = argument(c, args, 1);
    super::perf_timeline::mark(c, name, options)
}

pub(crate) fn measure(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    brand(c, receiver)?;
    let name = required(c, args, "name")?;
    let start_or_options = argument(c, args, 1);
    let end_mark = argument(c, args, 2);
    super::perf_timeline::measure(c, name, start_or_options, end_mark)
}

pub(crate) fn performance_now(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Outcome {
    brand(c, receiver)?;
    let elapsed = elapsed_ms(c);
    Ok(c.number(elapsed))
}

/// `setResourceTimingBufferSize(maxSize)`: `maxSize` as a WebIDL `unsigned long`.
pub(crate) fn set_resource_timing_buffer_size(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    brand(c, receiver)?;
    let size = required(c, args, "maxSize")?;
    let size = c.to_number(size)?;
    let size = if size.is_finite() { size.trunc().rem_euclid(UNSIGNED_LONG_MODULUS) } else { 0.0 };
    super::perf_timeline::set_resource_limit(c, size as usize);
    Ok(c.undefined())
}

pub(crate) fn time_origin(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Outcome {
    brand(c, receiver)?;
    let micros = super::performance_binding_shared_vm::milestone_ns(c, "timeOriginTimestamp")?;
    Ok(c.number(micros / MICROS_PER_MILLI))
}

pub(crate) fn to_json(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Outcome {
    brand(c, receiver)?;
    let node_timing = get(c, receiver, "nodeTiming")?;
    let time_origin = get(c, receiver, "timeOrigin")?;
    let utilization = call_method(c, receiver, "eventLoopUtilization", &[])?;
    plain_object(c, &[("nodeTiming", node_timing), ("timeOrigin", time_origin), ("eventLoopUtilization", utilization)])
}

pub(crate) fn performance_inspect(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    inspect_as(c, receiver, args, "Performance", |c| {
        let node_timing = get(c, receiver, "nodeTiming")?;
        let time_origin = get(c, receiver, "timeOrigin")?;
        plain_object(c, &[("nodeTiming", node_timing), ("timeOrigin", time_origin)])
    })
}

/// `onresourcetimingbufferfull`, the event handler attribute of `BUFFER_FULL_EVENT`, its
/// holder kept with the brand.
const BUFFER_FULL_HANDLER: &str = "onresourcetimingbufferfull";
pub(super) const BUFFER_FULL_EVENT: &str = "resourcetimingbufferfull";

pub(crate) fn get_buffer_full_handler(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Outcome {
    let state = receiver_slot(c, receiver, api::PERFORMANCE_SLOT, "Performance")?;
    super::event_target_shared_vm::event_handler(c, state, BUFFER_FULL_HANDLER)
}

pub(crate) fn set_buffer_full_handler(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    let state = receiver_slot(c, receiver, api::PERFORMANCE_SLOT, "Performance")?;
    let value = argument(c, args, 0);
    super::event_target_shared_vm::set_event_handler(c, receiver, state, BUFFER_FULL_HANDLER, BUFFER_FULL_EVENT, value)?;
    Ok(c.undefined())
}

pub(crate) fn mark_resource_timing(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    super::perf_timeline::mark_resource_timing(c, args)
}

/// `process.hrtime()` in milliseconds, as `eventLoopUtilization` reads it.
fn hrtime_ms(c: &mut Context<'_>) -> f64 {
    let control = c.host_mut().shared_state().borrow().process_control.clone();
    let ns = control.monotonic_now_ns().unwrap_or_default().trunc();
    let seconds = (ns / NANOS_PER_SECOND).trunc();
    seconds * MILLIS_PER_SECOND + (ns - seconds * NANOS_PER_SECOND) / NANOS_PER_MILLI
}

fn utilization_object(c: &mut Context<'_>, idle: f64, active: f64) -> Outcome {
    let idle_root = c.number(idle);
    let active_root = c.number(active);
    let utilization = c.number(active / (idle + active));
    plain_object(c, &[("idle", idle_root), ("active", active_root), ("utilization", utilization)])
}

fn field_number(c: &mut Context<'_>, object: RootId, name: &str) -> Result<f64, RootedError> {
    let value = get(c, object, name)?;
    c.to_number(value)
}

/// `eventLoopUtilization(util1, util2)`: the loop's idle and active milliseconds since it
/// started, since `util1`, or between `util1` and `util2`.
pub(crate) fn event_loop_utilization(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let loop_start = super::performance_binding_shared_vm::milestone_ns(c, "loopStart")? / NANOS_PER_MILLI;
    if loop_start <= 0.0 {
        let zero = c.number(0.0);
        return plain_object(c, &[("idle", zero), ("active", zero), ("utilization", zero)]);
    }
    let idle_time = c.host_mut().shared_state().borrow().process_control.event_loop_idle_ms();
    let first = argument(c, args, 0);
    let second = argument(c, args, 1);
    if c.truthy_rooted(second)? {
        let idle = field_number(c, first, "idle")? - field_number(c, second, "idle")?;
        let active = field_number(c, first, "active")? - field_number(c, second, "active")?;
        return utilization_object(c, idle, active);
    }
    let active = hrtime_ms(c) - loop_start - idle_time;
    if !c.truthy_rooted(first)? {
        return utilization_object(c, idle_time, active);
    }
    let idle = idle_time - field_number(c, first, "idle")?;
    let active = active - field_number(c, first, "active")?;
    utilization_object(c, idle, active)
}

/// `performance.nodeTiming`: an entry named `node` whose milestones read the clock, its
/// class's prototype under `PerformanceEntry.prototype`.
fn node_timing(c: &mut Context<'_>) -> Outcome {
    let class = c.host_constructor_named(crate::host::shared_vm::operation!("perfNodeTimingConstructor"), "PerformanceNodeTiming")?;
    let prototype = get(c, class, "prototype")?;
    let entry_prototype = api_member(c, api::ENTRY_PROTOTYPE)?;
    c.set_prototype_rooted(prototype, entry_prototype)?;
    define_inspect(c, prototype, "perfNodeTimingInspect")?;
    define_methods(c, prototype, &[("toJSON", "perfNodeTimingToJson", 0.0)], false)?;
    let timing = c.object_rooted()?;
    c.set_prototype_rooted(timing, prototype)?;
    let name = c.string_rooted(NODE_TIMING_NAME);
    let zero = c.number(0.0);
    for (field, value) in [("name", name), ("entryType", name), ("startTime", zero)] {
        c.define_named_data_property_rooted(timing, field, value, false, true, true)?;
    }
    let undefined = c.undefined();
    define_getter(c, timing, "duration", "perfNodeTimingNow", undefined, true)?;
    for milestone in NODE_TIMING_MILESTONES {
        let label = c.string_rooted(milestone);
        define_getter(c, timing, milestone, "perfNodeTimingMilestone", label, true)?;
    }
    define_getter(c, timing, "idleTime", "perfNodeTimingIdleTime", undefined, true)?;
    define_getter(c, timing, "uvMetricsInfo", "perfNodeTimingUvMetrics", undefined, true)?;
    Ok(timing)
}

pub(crate) fn node_timing_constructor(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    Err(illegal_constructor(c))
}

pub(crate) fn node_timing_now(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let elapsed = elapsed_ms(c);
    Ok(c.number(elapsed))
}

pub(crate) fn node_timing_milestone(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let label = c.host_function_data()?;
    let label = c.to_string(label)?;
    let value = milestone_ms(c, &label)?;
    Ok(c.number(value))
}

pub(crate) fn node_timing_idle_time(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let idle = c.host_mut().shared_state().borrow().process_control.event_loop_idle_ms();
    Ok(c.number(idle))
}

pub(crate) fn node_timing_uv_metrics(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let iterations = c.host_mut().shared_state().borrow().process_control.event_loop_iterations() as f64;
    let loop_count = c.number(iterations);
    let waiting = c.number(0.0);
    plain_object(c, &[("loopCount", loop_count), ("events", loop_count), ("eventsWaiting", waiting)])
}

fn node_timing_projection(c: &mut Context<'_>, receiver: RootId) -> Outcome {
    let name = c.string_rooted(NODE_TIMING_NAME);
    let mut fields = vec![("name", name), ("entryType", name)];
    for field in ["startTime", "duration"].into_iter().chain(NODE_TIMING_JSON) {
        fields.push((field, get(c, receiver, field)?));
    }
    plain_object(c, &fields)
}

pub(crate) fn node_timing_to_json(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Outcome {
    node_timing_projection(c, receiver)
}

pub(crate) fn node_timing_inspect(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    inspect_as(c, receiver, args, "PerformanceNodeTiming", |c| node_timing_projection(c, receiver))
}

/// `timerify(fn, options)`: `fn` wrapped to time each call or construction as a `function`
/// entry, recording the nanoseconds in `options.histogram` when given.
pub(crate) fn timerify(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let function = argument(c, args, 0);
    validate_function(c, function, "fn")?;
    let options = argument(c, args, 1);
    let options = if is_undefined(c, options) { c.object_rooted()? } else { options };
    validate_object(c, options, "options")?;
    let histogram = get(c, options, "histogram")?;
    if !is_undefined(c, histogram) {
        let record = if super::perf_histogram::is_histogram(c, histogram)? { get(c, histogram, "record")? } else { c.undefined() };
        if !c.is_callable_rooted(record)? {
            return Err(type_error_arg(c, histogram, "options.histogram", "RecordableHistogram"));
        }
    }
    let data = c.record_rooted(&TIMERIFIED, &[function, histogram])?;
    let timerified = c.host_constructor_named_with_data(crate::host::shared_vm::operation!("perfTimerified"), "", data)?;
    let length = get(c, function, "length")?;
    c.define_named_data_property_rooted(timerified, "length", length, false, true, false)?;
    let name = get(c, function, "name")?;
    let name = c.to_string(name)?;
    let name = c.string_rooted(&format!("timerified {name}"));
    c.define_named_data_property_rooted(timerified, "name", name, false, true, false)?;
    Ok(timerified)
}

/// A timerified function's data: the function and the histogram.
const TIMERIFIED: [&str; 2] = ["function", "histogram"];
/// A timed call's completion: its function's data, its start and its arguments.
const TIMED_CALL: [&str; 3] = ["timerified", "start", "arguments"];

pub(crate) fn timerified(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    let data = c.host_function_data()?;
    let function = get(c, data, "function")?;
    let constructing = c.new_target_rooted().is_some();
    let start = elapsed_ms(c);
    let result = if constructing { c.construct_rooted(function, function, args)? } else { c.call_rooted(function, receiver, args)? };
    let start = c.number(start);
    let arguments = c.array_rooted(args)?;
    let call = c.record_rooted(&TIMED_CALL, &[data, start, arguments])?;
    if !constructing && !is_nullish(c, result) {
        let finally = get(c, result, "finally")?;
        if c.is_callable_rooted(finally)? {
            let complete = c.host_function_named_with_data(crate::host::shared_vm::operation!("perfTimedComplete"), "bound processComplete", call)?;
            return c.call_rooted(finally, result, &[complete]);
        }
    }
    complete_timed_call(c, call)?;
    Ok(result)
}

pub(crate) fn timed_complete(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let call = c.host_function_data()?;
    complete_timed_call(c, call)?;
    Ok(c.undefined())
}

/// `processComplete`: the duration recorded in the histogram and enqueued as an entry carrying
/// the call's arguments.
fn complete_timed_call(c: &mut Context<'_>, call: RootId) -> Result<(), RootedError> {
    let data = get(c, call, "timerified")?;
    let start = get(c, call, "start")?;
    let start = number_of(c, start).unwrap_or_default();
    let duration = elapsed_ms(c) - start;
    let histogram = get(c, data, "histogram")?;
    if !is_undefined(c, histogram) {
        let nanos = c.number((duration * NANOS_PER_MILLI).ceil());
        call_method(c, histogram, "record", &[nanos])?;
    }
    let function = get(c, data, "function")?;
    let name = get(c, function, "name")?;
    let arguments = get(c, call, "arguments")?;
    let entry = super::perf_timeline::node_entry(c, name, "function", start, duration, arguments)?;
    for (index, value) in c.array_like_values_rooted(arguments)?.into_iter().enumerate() {
        let key = c.number(index as f64);
        c.set_property_rooted(entry, key, value, entry)?;
    }
    super::perf_timeline::enqueue(c, entry)
}
