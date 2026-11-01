//! The performance timeline (`lib/internal/perf/{performance_entry,usertiming,
//! resource_timing,observe}.js`): entries, the buffers `performance` keeps, the mark
//! timings measures read, and the observers entries are delivered to.
//!
//! An entry's fields are one record in a private slot (`ENTRY_FIELDS`); the buffers,
//! mark timings and connected observers are host state (`Timeline`), each entry retained
//! while a buffer holds it.
use super::event_target_shared_vm::{argument, get, is_nullish, is_undefined, missing_args, put};
use super::perf_hooks::{
    api, api_member, attach_slot, call_method, class, coded_error, define_getter, define_inspect, define_methods,
    define_tag, illegal_constructor, inspect_as, milestone_ms, number_of, operation, plain_object, receiver_slot,
    slot, to_text, type_error_arg, validate_function, validate_number, validate_object, Context, Getter, Method, Outcome,
};
use quench_runtime::{RootId, RootedError, Runtime};
use std::collections::HashMap;

/// The timeline's host state.
pub(crate) struct Timeline {
    marks: Vec<RootId>,
    measures: Vec<RootId>,
    resources: Vec<RootId>,
    /// Resource entries past the limit, offered back after a `resourcetimingbufferfull`.
    overflow: Vec<RootId>,
    overflow_pending: bool,
    resource_limit: usize,
    /// The latest start time of each mark name, which measures read.
    mark_times: HashMap<String, f64>,
    observers: Vec<Observer>,
    /// The observers with entries to deliver, in the order they first had one.
    pending: Vec<RootId>,
    dispatch_queued: bool,
    /// The buffer types already warned of growing past `BUFFER_WARN_SIZE`.
    warned: Vec<String>,
}

/// `kPerformanceEntryBufferWarnSize` and Node's default resource timing buffer size.
const BUFFER_WARN_SIZE: usize = 1_000_000;
const DEFAULT_RESOURCE_LIMIT: usize = 250;

impl Default for Timeline {
    fn default() -> Self {
        Self {
            marks: Vec::new(),
            measures: Vec::new(),
            resources: Vec::new(),
            overflow: Vec::new(),
            overflow_pending: false,
            resource_limit: DEFAULT_RESOURCE_LIMIT,
            mark_times: HashMap::new(),
            observers: Vec::new(),
            pending: Vec::new(),
            dispatch_queued: false,
            warned: Vec::new(),
        }
    }
}

/// A connected observer: the types it observes and the entries it has yet to deliver.
pub(crate) struct Observer {
    object: RootId,
    types: Vec<String>,
    buffer: Vec<RootId>,
}

/// The buffers `performance` keeps, by entry type.
#[derive(Clone, Copy)]
pub(crate) enum Buffer {
    Mark,
    Measure,
    Resource,
}

impl Buffer {
    const ALL: [Self; 3] = [Self::Mark, Self::Measure, Self::Resource];

    fn entry_type(self) -> &'static str {
        match self {
            Self::Mark => "mark",
            Self::Measure => "measure",
            Self::Resource => "resource",
        }
    }

    fn of(entry_type: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|buffer| buffer.entry_type() == entry_type)
    }

    fn entries(self, timeline: &mut Timeline) -> &mut Vec<RootId> {
        match self {
            Self::Mark => &mut timeline.marks,
            Self::Measure => &mut timeline.measures,
            Self::Resource => &mut timeline.resources,
        }
    }

    /// The method that clears it, which the buffer warning names.
    fn clearer(self) -> &'static str {
        match self {
            Self::Mark => "performance.clearMarks",
            Self::Measure => "performance.clearMeasures",
            Self::Resource => "performance.clearResourceTimings",
        }
    }
}

/// `PerformanceObserver.supportedEntryTypes`.
const SUPPORTED_ENTRY_TYPES: [&str; 9] = ["dns", "function", "gc", "http", "http2", "mark", "measure", "net", "resource"];
/// The milestones `nodeTiming` exposes, which marks may not be named after and measures read.
const READ_ONLY_MARKS: [&str; 6] = ["nodeStart", "v8Start", "environment", "loopStart", "loopExit", "bootstrapComplete"];

/// An entry's fields, in a record; a resource entry adds its timing.
const ENTRY_FIELDS: [&str; 5] = ["name", "entryType", "startTime", "duration", "detail"];
const RESOURCE_FIELDS: [&str; 10] =
    ["name", "entryType", "startTime", "duration", "detail", "timingInfo", "initiatorType", "cacheMode", "deliveryType", "responseStatus"];
/// The fields each entry class's `toJSON` reports.
const ENTRY_JSON: [&str; 4] = ["name", "entryType", "startTime", "duration"];
const DETAILED_JSON: [&str; 5] = ["name", "entryType", "startTime", "duration", "detail"];

/// An observer's slot: its callback and observation kind.
const OBSERVER_FIELDS: [&str; 2] = ["callback", "kind"];
/// `kTypeSingle` and `kTypeMultiple`.
const OBSERVE_SINGLE: f64 = 0.0;
const OBSERVE_MULTIPLE: f64 = 1.0;

const ENTRY_GETTERS: [Getter; 4] =
    [("name", "perfEntryName"), ("entryType", "perfEntryType"), ("startTime", "perfEntryStartTime"), ("duration", "perfEntryDuration")];
/// `PerformanceResourceTiming.prototype`'s timing getters: (name, path in its timing info).
const RESOURCE_TIMING: [(&str, &[&str]); 15] = [
    ("workerStart", &["finalServiceWorkerStartTime"]),
    ("redirectStart", &["redirectStartTime"]),
    ("redirectEnd", &["redirectEndTime"]),
    ("fetchStart", &["postRedirectStartTime"]),
    ("domainLookupStart", &["finalConnectionTimingInfo", "domainLookupStartTime"]),
    ("domainLookupEnd", &["finalConnectionTimingInfo", "domainLookupEndTime"]),
    ("connectStart", &["finalConnectionTimingInfo", "connectionStartTime"]),
    ("connectEnd", &["finalConnectionTimingInfo", "connectionEndTime"]),
    ("secureConnectionStart", &["finalConnectionTimingInfo", "secureConnectionStartTime"]),
    ("nextHopProtocol", &["finalConnectionTimingInfo", "ALPNNegotiatedProtocol"]),
    ("requestStart", &["finalNetworkRequestStartTime"]),
    ("responseStart", &["finalNetworkResponseStartTime"]),
    ("responseEnd", &["endTime"]),
    ("encodedBodySize", &["encodedBodySize"]),
    ("decodedBodySize", &["decodedBodySize"]),
];
/// The getters before `workerStart` in the prototype's key order.
const RESOURCE_TIMING_BEFORE_WORKER: usize = 0;
/// `PerformanceResourceTiming.prototype.toJSON`'s fields.
const RESOURCE_JSON: [&str; 23] = [
    "name", "entryType", "startTime", "duration", "initiatorType", "nextHopProtocol", "workerStart", "redirectStart",
    "redirectEnd", "fetchStart", "domainLookupStart", "domainLookupEnd", "connectStart", "connectEnd",
    "secureConnectionStart", "requestStart", "responseStart", "responseEnd", "transferSize", "encodedBodySize",
    "decodedBodySize", "deliveryType", "responseStatus",
];
/// `transferSize`: the header overhead Node adds to the encoded body, by cache mode.
const TRANSFER_OVERHEAD: f64 = 300.0;
const CACHE_LOCAL: &str = "local";
const CACHE_VALIDATED: &str = "validated";

const OBSERVER_METHODS: [Method; 3] =
    [("observe", "perfObserverObserve", 0.0), ("disconnect", "perfObserverDisconnect", 0.0), ("takeRecords", "perfObserverTakeRecords", 0.0)];
const LIST_METHODS: [Method; 3] = [
    ("getEntries", "perfListGetEntries", 0.0),
    ("getEntriesByType", "perfListGetEntriesByType", 1.0),
    ("getEntriesByName", "perfListGetEntriesByName", 1.0),
];

fn timeline<T>(c: &mut Context<'_>, f: impl FnOnce(&mut Timeline) -> T) -> T {
    let shared = c.host_mut().shared_state();
    let mut state = shared.borrow_mut();
    f(&mut state.performance_native.timeline)
}

/// The entry classes, in the module's export order.
pub(super) fn build(c: &mut Context<'_>) -> Result<Vec<(&'static str, RootId)>, RootedError> {
    let entry = class(c, "PerformanceEntry", "perfEntryConstructor", 0.0, None)?;
    let entry_prototype = get(c, entry, "prototype")?;
    for (name, op) in ENTRY_GETTERS {
        let undefined = c.undefined();
        define_getter(c, entry_prototype, name, op, undefined, true)?;
    }
    json_method(c, entry_prototype, &ENTRY_JSON, true)?;
    define_inspect(c, entry_prototype, "perfEntryInspect")?;
    share(c, api::ENTRY_PROTOTYPE, entry_prototype)?;

    let node_entry = class(c, "PerformanceNodeEntry", "perfEntryConstructor", 0.0, Some(entry))?;
    let node_entry_prototype = get(c, node_entry, "prototype")?;
    detail_getter(c, node_entry_prototype, "NodePerformanceEntry")?;
    json_method(c, node_entry_prototype, &DETAILED_JSON, false)?;
    share(c, api::NODE_ENTRY_PROTOTYPE, node_entry_prototype)?;

    let mark = class(c, "PerformanceMark", "perfMarkConstructor", 1.0, Some(entry))?;
    let mark_prototype = get(c, mark, "prototype")?;
    detail_getter(c, mark_prototype, "PerformanceMark")?;
    json_method(c, mark_prototype, &DETAILED_JSON, false)?;
    define_tag(c, mark_prototype, "PerformanceMark")?;
    share(c, api::MARK_PROTOTYPE, mark_prototype)?;

    let measure = class(c, "PerformanceMeasure", "perfEntryConstructor", 0.0, Some(entry))?;
    let measure_prototype = get(c, measure, "prototype")?;
    detail_getter(c, measure_prototype, "PerformanceMeasure")?;
    json_method(c, measure_prototype, &DETAILED_JSON, false)?;
    define_tag(c, measure_prototype, "PerformanceMeasure")?;
    share(c, api::MEASURE_PROTOTYPE, measure_prototype)?;

    let resource = resource_class(c, entry)?;

    let observer = class(c, "PerformanceObserver", "perfObserverConstructor", 1.0, None)?;
    let observer_prototype = get(c, observer, "prototype")?;
    define_methods(c, observer_prototype, &OBSERVER_METHODS, true)?;
    define_tag(c, observer_prototype, "PerformanceObserver")?;
    define_inspect(c, observer_prototype, "perfObserverInspect")?;
    let types = SUPPORTED_ENTRY_TYPES.map(|name| c.string_rooted(name));
    let types = c.array_rooted(&types)?;
    let freeze = api_member(c, api::OBJECT_FREEZE)?;
    let undefined = c.undefined();
    c.call_rooted(freeze, undefined, &[types])?;
    share(c, api::SUPPORTED_ENTRY_TYPES, types)?;
    define_getter(c, observer, "supportedEntryTypes", "perfObserverSupportedTypes", undefined, false)?;

    let list = class(c, "PerformanceObserverEntryList", "perfEntryConstructor", 0.0, None)?;
    let list_prototype = get(c, list, "prototype")?;
    define_methods(c, list_prototype, &LIST_METHODS, true)?;
    define_tag(c, list_prototype, "PerformanceObserverEntryList")?;
    define_inspect(c, list_prototype, "perfListInspect")?;
    share(c, api::LIST_PROTOTYPE, list_prototype)?;

    Ok(vec![
        ("PerformanceEntry", entry),
        ("PerformanceMark", mark),
        ("PerformanceMeasure", measure),
        ("PerformanceObserver", observer),
        ("PerformanceObserverEntryList", list),
        ("PerformanceResourceTiming", resource),
    ])
}

/// `value` kept in the group's data as `name`.
fn share(c: &mut Context<'_>, name: &str, value: RootId) -> Result<(), RootedError> {
    let shared = super::perf_hooks::shared_api(c)?;
    put(c, shared, name, value)
}

fn json_method(c: &mut Context<'_>, prototype: RootId, fields: &[&str], enumerable: bool) -> Result<(), RootedError> {
    let names = fields.iter().map(|name| c.string_rooted(name)).collect::<Vec<_>>();
    let names = c.array_rooted(&names)?;
    let function = c.host_function_named_with_data(operation("perfEntryToJson"), "toJSON", names)?;
    let zero = c.number(0.0);
    c.define_named_data_property_rooted(function, "length", zero, false, false, true)?;
    c.define_named_data_property_rooted(prototype, "toJSON", function, true, enumerable, true)?;
    Ok(())
}

fn detail_getter(c: &mut Context<'_>, prototype: RootId, class: &str) -> Result<(), RootedError> {
    let class = c.string_rooted(class);
    define_getter(c, prototype, "detail", "perfEntryDetail", class, true)
}

fn resource_class(c: &mut Context<'_>, entry: RootId) -> Outcome {
    let resource = class(c, "PerformanceResourceTiming", "perfEntryConstructor", 0.0, Some(entry))?;
    let prototype = get(c, resource, "prototype")?;
    let undefined = c.undefined();
    for (name, op) in [("name", "perfResourceName"), ("startTime", "perfResourceStartTime"), ("duration", "perfResourceDuration")] {
        define_getter(c, prototype, name, op, undefined, false)?;
    }
    let initiator = c.string_rooted("initiatorType");
    define_getter(c, prototype, "initiatorType", "perfResourceField", initiator, true)?;
    let (_, timings) = RESOURCE_TIMING.split_at(RESOURCE_TIMING_BEFORE_WORKER);
    for (name, path) in timings {
        let path = path.iter().map(|step| c.string_rooted(step)).collect::<Vec<_>>();
        let path = c.array_rooted(&path)?;
        define_getter(c, prototype, name, "perfResourceTiming", path, true)?;
    }
    define_getter(c, prototype, "transferSize", "perfResourceTransferSize", undefined, true)?;
    for name in ["deliveryType", "responseStatus"] {
        let field = c.string_rooted(name);
        define_getter(c, prototype, name, "perfResourceField", field, true)?;
    }
    define_methods(c, prototype, &[("toJSON", "perfResourceToJson", 0.0)], true)?;
    define_tag(c, prototype, "PerformanceResourceTiming")?;
    share(c, api::RESOURCE_PROTOTYPE, prototype)?;
    Ok(resource)
}

/// `new PerformanceEntry()` and the other classes only the host makes.
pub(crate) fn entry_constructor(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    Err(illegal_constructor(c))
}

/// An entry of the class whose prototype `prototype` names, with `fields`.
fn create_entry(c: &mut Context<'_>, prototype: &str, names: &'static [&'static str], fields: &[RootId]) -> Outcome {
    let entry = c.object_rooted()?;
    let prototype = api_member(c, prototype)?;
    c.set_prototype_rooted(entry, prototype)?;
    let record = c.record_rooted(names, fields)?;
    attach_slot(c, entry, api::ENTRY_SLOT, record)?;
    Ok(entry)
}

/// `createPerformanceNodeEntry(name, type, start, duration, detail)`.
pub(crate) fn node_entry(c: &mut Context<'_>, name: RootId, entry_type: &str, start: f64, duration: f64, detail: RootId) -> Outcome {
    let entry_type = c.string_rooted(entry_type);
    let start = c.number(start);
    let duration = c.number(duration);
    create_entry(c, api::NODE_ENTRY_PROTOTYPE, &ENTRY_FIELDS, &[name, entry_type, start, duration, detail])
}

fn entry_record(c: &mut Context<'_>, receiver: RootId, class: &str) -> Outcome {
    receiver_slot(c, receiver, api::ENTRY_SLOT, class)
}

fn entry_field(c: &mut Context<'_>, receiver: RootId, field: &str) -> Outcome {
    let record = entry_record(c, receiver, "PerformanceEntry")?;
    get(c, record, field)
}

pub(crate) fn entry_name(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Outcome {
    entry_field(c, receiver, "name")
}

pub(crate) fn entry_type(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Outcome {
    entry_field(c, receiver, "entryType")
}

pub(crate) fn entry_start_time(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Outcome {
    entry_field(c, receiver, "startTime")
}

pub(crate) fn entry_duration(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Outcome {
    entry_field(c, receiver, "duration")
}

/// `get detail`, checked against the class the getter's data names.
pub(crate) fn entry_detail(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Outcome {
    let class = c.host_function_data()?;
    let class = c.to_string(class)?;
    let record = entry_record(c, receiver, &class)?;
    get(c, record, "detail")
}

/// `toJSON()`: the record's fields the method's data names.
pub(crate) fn entry_to_json(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Outcome {
    let names = c.host_function_data()?;
    let record = entry_record(c, receiver, "PerformanceEntry")?;
    let json = c.object_rooted()?;
    for name in c.array_like_values_rooted(names)? {
        let name = c.to_string(name)?;
        let value = get(c, record, &name)?;
        put(c, json, &name, value)?;
    }
    Ok(json)
}

pub(crate) fn entry_inspect(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    let constructor = get(c, receiver, "constructor")?;
    let name = get(c, constructor, "name")?;
    let label = c.to_string(name)?;
    inspect_as(c, receiver, args, &label, |c| call_method(c, receiver, "toJSON", &[]))
}

fn resource_record(c: &mut Context<'_>, receiver: RootId) -> Outcome {
    entry_record(c, receiver, "PerformanceResourceTiming")
}

fn timing_info(c: &mut Context<'_>, receiver: RootId) -> Outcome {
    let record = resource_record(c, receiver)?;
    get(c, record, "timingInfo")
}

pub(crate) fn resource_name(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Outcome {
    let record = resource_record(c, receiver)?;
    get(c, record, "name")
}

pub(crate) fn resource_start_time(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Outcome {
    let timing = timing_info(c, receiver)?;
    get(c, timing, "startTime")
}

pub(crate) fn resource_duration(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Outcome {
    let timing = timing_info(c, receiver)?;
    let end = get(c, timing, "endTime")?;
    let start = get(c, timing, "startTime")?;
    c.subtract_rooted(end, start)
}

/// A resource entry's own field, which the getter's data names.
pub(crate) fn resource_field(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Outcome {
    let field = c.host_function_data()?;
    let field = c.to_string(field)?;
    let record = resource_record(c, receiver)?;
    get(c, record, &field)
}

/// A timing getter: the path its data names in the timing info, undefined past a nullish step.
pub(crate) fn resource_timing(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Outcome {
    let path = c.host_function_data()?;
    let mut value = timing_info(c, receiver)?;
    for step in c.array_like_values_rooted(path)? {
        if is_nullish(c, value) {
            return Ok(c.undefined());
        }
        let step = c.to_string(step)?;
        value = get(c, value, &step)?;
    }
    Ok(value)
}

pub(crate) fn resource_transfer_size(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Outcome {
    let record = resource_record(c, receiver)?;
    let mode = get(c, record, "cacheMode")?;
    match c.string_text(mode)?.as_deref() {
        Some(CACHE_LOCAL) => return Ok(c.number(0.0)),
        Some(CACHE_VALIDATED) => return Ok(c.number(TRANSFER_OVERHEAD)),
        _ => {}
    }
    let timing = get(c, record, "timingInfo")?;
    let size = get(c, timing, "encodedBodySize")?;
    let overhead = c.number(TRANSFER_OVERHEAD);
    c.arithmetic_rooted(quench_runtime::NativeArithmetic::Add, size, overhead)
}

pub(crate) fn resource_to_json(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Outcome {
    resource_record(c, receiver)?;
    let json = c.object_rooted()?;
    for name in RESOURCE_JSON {
        let value = get(c, receiver, name)?;
        put(c, json, name, value)?;
    }
    Ok(json)
}

/// `markResourceTiming(timingInfo, requestedUrl, initiatorType, global, cacheMode,
/// bodyInfo, responseStatus, deliveryType = '')`: a resource entry, enqueued and buffered.
pub(super) fn mark_resource_timing(c: &mut Context<'_>, args: &[RootId]) -> Outcome {
    let [timing, url, initiator, _global, cache_mode, _body, status, delivery] = std::array::from_fn(|index| argument(c, args, index));
    let delivery = if is_undefined(c, delivery) { c.string_rooted("") } else { delivery };
    let entry_type = c.string_rooted("resource");
    let start = get(c, timing, "startTime")?;
    let undefined = c.undefined();
    let fields = [url, entry_type, start, undefined, undefined, timing, initiator, cache_mode, delivery, status];
    let entry = create_entry(c, api::RESOURCE_PROTOTYPE, &RESOURCE_FIELDS, &fields)?;
    enqueue(c, entry)?;
    buffer_resource(c, entry)?;
    Ok(entry)
}

fn buffer_resource(c: &mut Context<'_>, entry: RootId) -> Result<(), RootedError> {
    let retained = c.retain(entry)?;
    let schedule = timeline(c, |timeline| {
        if timeline.resources.len() < timeline.resource_limit && !timeline.overflow_pending {
            timeline.resources.push(retained);
            return false;
        }
        timeline.overflow.push(retained);
        !std::mem::replace(&mut timeline.overflow_pending, true)
    });
    if schedule {
        let callback = c.host_function(operation("perfResourceBufferFull"))?;
        let undefined = c.undefined();
        super::timers_shared_vm::set_immediate(c, undefined, &[callback])?;
    }
    Ok(())
}

/// The resource buffer's overflow: `resourcetimingbufferfull` dispatched while entries
/// remain over the limit, each round admitting what the limit then allows; a round that
/// admits none drops the rest.
pub(crate) fn resource_buffer_full(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    while timeline(c, |timeline| !timeline.overflow.is_empty()) {
        let performance = api_member(c, api::PERFORMANCE)?;
        let global = c.global_root()?;
        let event = super::perf_hooks::intrinsic(c, global, "Event")?;
        let kind = c.string_rooted(super::perf_hooks::BUFFER_FULL_EVENT);
        let event = c.construct_rooted(event, event, &[kind])?;
        call_method(c, performance, "dispatchEvent", &[event])?;
        let dropped = timeline(c, |timeline| {
            let room = timeline.resource_limit.saturating_sub(timeline.resources.len()).min(timeline.overflow.len());
            let admitted = timeline.overflow.drain(..room).collect::<Vec<_>>();
            timeline.resources.extend(admitted);
            if room == 0 { std::mem::take(&mut timeline.overflow) } else { Vec::new() }
        });
        for root in dropped {
            c.release_root(root);
        }
    }
    timeline(c, |timeline| timeline.overflow_pending = false);
    Ok(c.undefined())
}

pub(super) fn set_resource_limit(c: &mut Context<'_>, limit: usize) {
    timeline(c, |timeline| timeline.resource_limit = limit);
}

/// `getMark(name)`: a timestamp, a `nodeTiming` milestone, or the named mark's start.
fn mark_time(c: &mut Context<'_>, value: RootId) -> Result<Option<f64>, RootedError> {
    if is_undefined(c, value) {
        return Ok(None);
    }
    if let Some(time) = number_of(c, value) {
        if time < 0.0 {
            return Err(invalid_timestamp(c, value));
        }
        return Ok(Some(time));
    }
    let name = to_text(c, value)?;
    if READ_ONLY_MARKS.contains(&name.as_str()) {
        return milestone_ms(c, &name).map(Some);
    }
    if let Some(time) = timeline(c, |timeline| timeline.mark_times.get(&name).copied()) {
        return Ok(Some(time));
    }
    let message = c.string_rooted(&format!("The \"{name}\" performance mark has not been set"));
    let kind = c.string_rooted("SyntaxError");
    let error = super::dom_exception_shared_vm::create(c, message, kind)?;
    Err(c.throw(error))
}

fn invalid_timestamp(c: &mut Context<'_>, value: RootId) -> RootedError {
    let text = match c.to_string(value) {
        Ok(text) => text,
        Err(error) => return error,
    };
    coded_error(c, false, "ERR_PERFORMANCE_INVALID_TIMESTAMP", &format!("{text} is not a valid timestamp"))
}

fn invalid_name(c: &mut Context<'_>, name: &str) -> RootedError {
    let value = c.string_rooted(name);
    match super::util_shared_vm::argument_value_error(c, value, "name", "is invalid") {
        Ok(error) | Err(error) => error,
    }
}

/// `detail != null ? structuredClone(detail) : null`.
fn cloned_detail(c: &mut Context<'_>, detail: RootId) -> Outcome {
    if is_nullish(c, detail) {
        return Ok(c.null());
    }
    c.structured_clone_rooted(detail, None)
}

/// `new PerformanceMark(name, options)`: a mark whose start the mark timings keep.
pub(crate) fn mark_constructor(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    super::event_target_shared_vm::require_new(c, "PerformanceMark")?;
    let Some(&name) = args.first() else {
        return Err(missing_args(c, &["name"]));
    };
    let options = argument(c, args, 1);
    let fields = mark_fields(c, name, options)?;
    let record = c.record_rooted(&ENTRY_FIELDS, &fields)?;
    attach_slot(c, receiver, api::ENTRY_SLOT, record)?;
    Ok(c.undefined())
}

fn mark_fields(c: &mut Context<'_>, name: RootId, options: RootId) -> Result<[RootId; 5], RootedError> {
    let name = to_text(c, name)?;
    if READ_ONLY_MARKS.contains(&name.as_str()) {
        return Err(invalid_name(c, &name));
    }
    let present = !is_nullish(c, options);
    if present {
        validate_object(c, options, "options")?;
    }
    let start = if present { get(c, options, "startTime")? } else { c.undefined() };
    let start = if is_nullish(c, start) { super::perf_hooks::now(c, start, &[])? } else { start };
    let time = validate_number(c, start, "startTime")?;
    if time < 0.0 {
        return Err(invalid_timestamp(c, start));
    }
    timeline(c, |timeline| timeline.mark_times.insert(name.clone(), time));
    let detail = if present { get(c, options, "detail")? } else { c.undefined() };
    let detail = cloned_detail(c, detail)?;
    let name = c.string_rooted(&name);
    let entry_type = c.string_rooted("mark");
    let duration = c.number(0.0);
    Ok([name, entry_type, start, duration, detail])
}

/// `performance.mark(name, options)`: a mark, enqueued and buffered.
pub(super) fn mark(c: &mut Context<'_>, name: RootId, options: RootId) -> Outcome {
    let options = if is_undefined(c, options) { c.object_rooted()? } else { options };
    let fields = mark_fields(c, name, options)?;
    let entry = create_entry(c, api::MARK_PROTOTYPE, &ENTRY_FIELDS, &fields)?;
    enqueue(c, entry)?;
    buffer_user_timing(c, entry, Buffer::Mark)?;
    Ok(entry)
}

fn invalid_measure(c: &mut Context<'_>, message: &str) -> RootedError {
    coded_error(c, false, "ERR_PERFORMANCE_MEASURE_INVALID_OPTIONS", message)
}

/// `calculateStartDuration(startOrMeasureOptions, endMark)`: the measure's start and
/// duration from marks, timestamps and options.
fn start_duration(c: &mut Context<'_>, start_or_options: RootId, end_mark: RootId) -> Result<(f64, f64), RootedError> {
    let undefined = c.undefined();
    let is_options = c.is_object_rooted(start_or_options)? && !is_nullish(c, start_or_options);
    let (mut start, mut end, mut duration) = (undefined, undefined, undefined);
    let mut options_valid = false;
    if is_options {
        start = get(c, start_or_options, "start")?;
        end = get(c, start_or_options, "end")?;
        duration = get(c, start_or_options, "duration")?;
        options_valid = !is_undefined(c, start) || !is_undefined(c, end);
    }
    if options_valid {
        if !is_undefined(c, end_mark) {
            return Err(invalid_measure(c, "endMark must not be specified"));
        }
        if !is_undefined(c, start) && !is_undefined(c, end) && !is_undefined(c, duration) {
            return Err(invalid_measure(c, "Must not have options.start, options.end, and options.duration specified"));
        }
    }
    let end_time = if !is_undefined(c, end_mark) {
        mark_time(c, end_mark)?
    } else if options_valid && !is_undefined(c, end) {
        mark_time(c, end)?
    } else if options_valid && !is_undefined(c, start) && !is_undefined(c, duration) {
        let start = mark_time(c, start)?.unwrap_or(f64::NAN);
        Some(start + mark_time(c, duration)?.unwrap_or(f64::NAN))
    } else {
        Some(super::perf_hooks::elapsed_ms(c))
    };
    let end_time = end_time.unwrap_or(f64::NAN);
    let start_time = if c.string_text(start_or_options)?.is_some() {
        mark_time(c, start_or_options)?.unwrap_or(f64::NAN)
    } else if options_valid && !is_undefined(c, start) {
        mark_time(c, start)?.unwrap_or(f64::NAN)
    } else if options_valid && !is_undefined(c, duration) {
        end_time - mark_time(c, duration)?.unwrap_or(f64::NAN)
    } else {
        0.0
    };
    Ok((start_time, end_time - start_time))
}

/// `performance.measure(name, startOrMeasureOptions, endMark)`: a measure, enqueued and
/// buffered.
pub(super) fn measure(c: &mut Context<'_>, name: RootId, start_or_options: RootId, end_mark: RootId) -> Outcome {
    if c.string_text(name)?.is_none() {
        return Err(type_error_arg(c, name, "name", "string"));
    }
    let start_or_options = if is_undefined(c, start_or_options) { c.object_rooted()? } else { start_or_options };
    let (start, duration) = start_duration(c, start_or_options, end_mark)?;
    let detail = if is_nullish(c, start_or_options) { c.undefined() } else { get(c, start_or_options, "detail")? };
    let detail = cloned_detail(c, detail)?;
    let entry_type = c.string_rooted("measure");
    let start = c.number(start);
    let duration = c.number(duration);
    let entry = create_entry(c, api::MEASURE_PROTOTYPE, &ENTRY_FIELDS, &[name, entry_type, start, duration, detail])?;
    enqueue(c, entry)?;
    buffer_user_timing(c, entry, Buffer::Measure)?;
    Ok(entry)
}

/// `bufferUserTiming`: the entry kept, with one warning per type past `BUFFER_WARN_SIZE`.
fn buffer_user_timing(c: &mut Context<'_>, entry: RootId, buffer: Buffer) -> Result<(), RootedError> {
    let retained = c.retain(entry)?;
    let count = timeline(c, |timeline| {
        let entries = buffer.entries(timeline);
        entries.push(retained);
        let count = entries.len();
        let warn = count > BUFFER_WARN_SIZE && !timeline.warned.iter().any(|kind| kind == buffer.entry_type());
        if warn {
            timeline.warned.push(buffer.entry_type().to_owned());
        }
        warn.then_some(count)
    });
    let Some(count) = count else {
        return Ok(());
    };
    let kind = buffer.entry_type();
    let message = format!(
        "Possible perf_hooks memory leak detected. {count} {kind} entries added to the global performance entry buffer. Use {} to clear the buffer.",
        buffer.clearer()
    );
    let warning = c.error_rooted(&message)?;
    let name = c.string_rooted("MaxPerformanceEntryBufferExceededWarning");
    put(c, warning, "name", name)?;
    let kind = c.string_rooted(kind);
    put(c, warning, "entryType", kind)?;
    let count = c.number(count as f64);
    put(c, warning, "count", count)?;
    let global = c.global_root()?;
    let process = get(c, global, "process")?;
    call_method(c, process, "emitWarning", &[warning])?;
    Ok(())
}

/// `clearMarkTimings(name)`: one mark name, or all.
pub(super) fn clear_mark_timings(c: &mut Context<'_>, name: Option<&str>) -> Result<(), RootedError> {
    match name {
        Some(name) if READ_ONLY_MARKS.contains(&name) => Err(invalid_name(c, name)),
        Some(name) => {
            timeline(c, |timeline| timeline.mark_times.remove(name));
            Ok(())
        }
        None => {
            timeline(c, |timeline| timeline.mark_times.clear());
            Ok(())
        }
    }
}

fn record_text(c: &mut Context<'_>, entry: RootId, field: &str) -> Result<Option<String>, RootedError> {
    let Some(record) = slot(c, entry, api::ENTRY_SLOT)? else {
        return Ok(None);
    };
    let value = get(c, record, field)?;
    c.string_text(value)
}

/// `clearEntriesFromBuffer(type, name)`: the buffer's entries named `name`, or all of them.
pub(super) fn clear_buffer(c: &mut Context<'_>, buffer: Buffer, name: Option<&str>) {
    let entries = timeline(c, |timeline| std::mem::take(buffer.entries(timeline)));
    let mut kept = Vec::new();
    for entry in entries {
        let keep = match name {
            None => false,
            Some(name) => scoped_text(c, entry, "name").as_deref() != Some(name),
        };
        if keep {
            kept.push(entry);
        } else {
            c.release_root(entry);
        }
    }
    timeline(c, |timeline| *buffer.entries(timeline) = kept);
}

fn scoped_text(c: &mut Context<'_>, entry: RootId, field: &str) -> Option<String> {
    let entry = c.scope_root(entry)?;
    record_text(c, entry, field).ok().flatten()
}

fn start_of(c: &mut Context<'_>, entry: RootId) -> f64 {
    let Ok(Some(record)) = slot(c, entry, api::ENTRY_SLOT) else {
        return f64::NAN;
    };
    get(c, record, "startTime").ok().and_then(|start| number_of(c, start)).unwrap_or(f64::NAN)
}

/// `entries` in start order, ties in their given order.
fn sorted(c: &mut Context<'_>, entries: Vec<RootId>) -> Outcome {
    let mut keyed = entries.into_iter().map(|entry| (start_of(c, entry), entry)).collect::<Vec<_>>();
    keyed.sort_by(|left, right| left.0.partial_cmp(&right.0).unwrap_or(std::cmp::Ordering::Equal));
    let entries = keyed.into_iter().map(|(_, entry)| entry).collect::<Vec<_>>();
    c.array_rooted(&entries)
}

/// `filterBufferMapByNameAndType(name, type)`: the buffered entries of `type` (every buffer
/// when none) named `name`, in start order.
pub(crate) fn buffered_entries(c: &mut Context<'_>, name: Option<&str>, kind: Option<&str>) -> Outcome {
    let buffers = match kind {
        None => Buffer::ALL.to_vec(),
        Some(kind) => match Buffer::of(kind) {
            Some(buffer) => vec![buffer],
            None => return c.array_rooted(&[]),
        },
    };
    let retained = timeline(c, |timeline| buffers.iter().flat_map(|buffer| buffer.entries(timeline).clone()).collect::<Vec<_>>());
    let mut entries = Vec::new();
    for root in retained {
        let Some(entry) = c.scope_root(root) else { continue };
        if name.is_none_or(|name| record_text(c, entry, "name").ok().flatten().as_deref() == Some(name)) {
            entries.push(entry);
        }
    }
    sorted(c, entries)
}

/// `enqueue(entry)`: `entry` buffered by each connected observer of its type, delivered on a
/// later immediate.
pub(crate) fn enqueue(c: &mut Context<'_>, entry: RootId) -> Result<(), RootedError> {
    let Some(kind) = record_text(c, entry, "entryType")? else {
        return Err(type_error_arg(c, entry, "entry", "PerformanceEntry"));
    };
    let observing = timeline(c, |timeline| {
        (0..timeline.observers.len()).filter(|&index| timeline.observers[index].types.contains(&kind)).collect::<Vec<_>>()
    });
    for index in observing {
        deliver_at(c, index, entry)?;
    }
    Ok(())
}

/// `kMaybeBuffer`: the entry buffered by connected observer `index`, which becomes pending.
fn deliver_at(c: &mut Context<'_>, index: usize, entry: RootId) -> Result<(), RootedError> {
    let retained = c.retain(entry)?;
    let schedule = timeline(c, |timeline| {
        let observer = &mut timeline.observers[index];
        observer.buffer.push(retained);
        let object = observer.object;
        if !timeline.pending.contains(&object) {
            timeline.pending.push(object);
        }
        !std::mem::replace(&mut timeline.dispatch_queued, true)
    });
    if schedule {
        let dispatch = c.host_function(operation("perfObserversDispatch"))?;
        let undefined = c.undefined();
        super::timers_shared_vm::set_immediate(c, undefined, &[dispatch])?;
    }
    Ok(())
}

/// A host observation (`observerCallback`): a Node entry delivered to its type's observers.
pub(crate) fn observe_node_entry(
    c: &mut Context<'_>,
    name: RootId,
    entry_type: &str,
    start: f64,
    duration: f64,
    detail: RootId,
) -> Result<(), RootedError> {
    let entry = node_entry(c, name, entry_type, start, duration, detail)?;
    enqueue(c, entry)
}

/// The pending observers' deliveries: each callback receives an entry list of what its
/// observer buffered, and the observer.
pub(crate) fn dispatch_observers(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let pending = timeline(c, |timeline| {
        timeline.dispatch_queued = false;
        std::mem::take(&mut timeline.pending)
    });
    let observers = pending.into_iter().filter_map(|root| c.scope_root(root)).collect::<Vec<_>>();
    for observer in observers {
        let records = take_records(c, observer)?;
        let list = entry_list(c, records)?;
        let state = receiver_slot(c, observer, api::OBSERVER_SLOT, "PerformanceObserver")?;
        let callback = get(c, state, "callback")?;
        let undefined = c.undefined();
        c.call_rooted(callback, undefined, &[list, observer])?;
    }
    Ok(c.undefined())
}

fn entry_list(c: &mut Context<'_>, records: RootId) -> Outcome {
    let entries = c.array_like_values_rooted(records)?;
    let entries = sorted(c, entries)?;
    let list = c.object_rooted()?;
    let prototype = api_member(c, api::LIST_PROTOTYPE)?;
    c.set_prototype_rooted(list, prototype)?;
    attach_slot(c, list, api::LIST_SLOT, entries)?;
    Ok(list)
}

/// `new PerformanceObserver(callback)`.
pub(crate) fn observer_constructor(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    super::event_target_shared_vm::require_new(c, "PerformanceObserver")?;
    let callback = argument(c, args, 0);
    validate_function(c, callback, "callback")?;
    let undefined = c.undefined();
    let state = c.record_rooted(&OBSERVER_FIELDS, &[callback, undefined])?;
    attach_slot(c, receiver, api::OBSERVER_SLOT, state)?;
    Ok(undefined)
}

pub(crate) fn observer_supported_types(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    api_member(c, api::SUPPORTED_ENTRY_TYPES)
}

fn connected_types(c: &mut Context<'_>, object: RootId) -> Option<Vec<String>> {
    let found = timeline(c, |timeline| timeline.observers.iter().map(|observer| (observer.object, observer.types.clone())).collect::<Vec<_>>());
    found.into_iter().find_map(|(root, types)| {
        let scoped = c.scope_root(root)?;
        c.strict_equal_rooted(scoped, object).ok()?.then_some(types)
    })
}

/// The index of `object` among the connected observers.
fn connected_index(c: &mut Context<'_>, object: RootId) -> Option<usize> {
    let roots = timeline(c, |timeline| timeline.observers.iter().map(|observer| observer.object).collect::<Vec<_>>());
    roots.into_iter().position(|root| {
        c.scope_root(root).is_some_and(|scoped| c.strict_equal_rooted(scoped, object).unwrap_or(false))
    })
}

/// The binding's observer counts follow each observed type the host reports itself.
fn count_types(c: &mut Context<'_>, types: &[String], delta: f64) -> Result<(), RootedError> {
    for kind in types {
        super::performance_binding_shared_vm::adjust_observer_count(c, kind, delta)?;
    }
    Ok(())
}

/// `observe(options)`: the observer connected to `options.entryTypes`, or to
/// `options.type` (with what is buffered, when `options.buffered`).
pub(crate) fn observer_observe(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    let state = receiver_slot(c, receiver, api::OBSERVER_SLOT, "PerformanceObserver")?;
    let options = argument(c, args, 0);
    let options = if is_undefined(c, options) { c.object_rooted()? } else { options };
    validate_object(c, options, "options")?;
    let entry_types = get(c, options, "entryTypes")?;
    let single_type = get(c, options, "type")?;
    let buffered = get(c, options, "buffered")?;
    if is_undefined(c, entry_types) && is_undefined(c, single_type) {
        return Err(missing_args(c, &["options.entryTypes", "options.type"]));
    }
    if !is_nullish(c, entry_types) && !is_nullish(c, single_type) {
        return Err(match super::util_shared_vm::argument_value_error(
            c,
            entry_types,
            "options.entryTypes",
            "options.entryTypes can not set with options.type together",
        ) {
            Ok(error) | Err(error) => error,
        });
    }
    let kind = get(c, state, "kind")?;
    let kind = match number_of(c, kind) {
        None => {
            let kind = if !is_undefined(c, single_type) { OBSERVE_SINGLE } else { OBSERVE_MULTIPLE };
            let kind_root = c.number(kind);
            put(c, state, "kind", kind_root)?;
            kind
        }
        Some(OBSERVE_SINGLE) if !is_undefined(c, entry_types) => {
            return Err(modification_error(c, "PerformanceObserver can not change to multiple observations"));
        }
        Some(OBSERVE_MULTIPLE) if !is_undefined(c, single_type) => {
            return Err(modification_error(c, "PerformanceObserver can not change to single observation"));
        }
        Some(kind) => kind,
    };
    let mut types = connected_types(c, receiver).unwrap_or_default();
    if kind == OBSERVE_MULTIPLE {
        if !c.is_array_rooted(entry_types)? {
            return Err(match super::util_shared_vm::argument_type_error_with_expected(c, entry_types, "options.entryTypes", "string[]") {
                Ok(error) | Err(error) => error,
            });
        }
        count_types(c, &types, -1.0)?;
        types.clear();
        for value in c.array_like_values_rooted(entry_types)? {
            let Some(kind) = c.string_text(value)? else { continue };
            if SUPPORTED_ENTRY_TYPES.contains(&kind.as_str()) && !types.contains(&kind) {
                count_types(c, std::slice::from_ref(&kind), 1.0)?;
                types.push(kind);
            }
        }
    } else {
        let Some(kind) = c.string_text(single_type)?.filter(|kind| SUPPORTED_ENTRY_TYPES.contains(&kind.as_str())) else {
            return Ok(c.undefined());
        };
        if !types.contains(&kind) {
            types.push(kind.clone());
        }
        count_types(c, std::slice::from_ref(&kind), 1.0)?;
        if c.truthy_rooted(buffered)? {
            connect(c, receiver, types.clone())?;
            let entries = buffered_entries(c, None, Some(&kind))?;
            for entry in c.array_like_values_rooted(entries)? {
                deliver_to(c, receiver, entry)?;
            }
        }
    }
    if types.is_empty() {
        disconnect(c, receiver)?;
    } else {
        connect(c, receiver, types)?;
    }
    Ok(c.undefined())
}

fn modification_error(c: &mut Context<'_>, message: &str) -> RootedError {
    let message = c.string_rooted(message);
    let kind = c.string_rooted("InvalidModificationError");
    match super::dom_exception_shared_vm::create(c, message, kind) {
        Ok(error) => c.throw(error),
        Err(error) => error,
    }
}

/// The observer connected, observing `types`.
fn connect(c: &mut Context<'_>, object: RootId, types: Vec<String>) -> Result<(), RootedError> {
    match connected_index(c, object) {
        Some(index) => timeline(c, |timeline| timeline.observers[index].types = types),
        None => {
            let object = c.retain(object)?;
            timeline(c, |timeline| timeline.observers.push(Observer { object, types, buffer: Vec::new() }));
        }
    }
    Ok(())
}

/// One entry buffered by one observer, as `enqueue` buffers it.
fn deliver_to(c: &mut Context<'_>, object: RootId, entry: RootId) -> Result<(), RootedError> {
    match connected_index(c, object) {
        Some(index) => deliver_at(c, index, entry),
        None => Ok(()),
    }
}

/// `disconnect()`: the observer stops observing and forgets what it buffered.
fn disconnect(c: &mut Context<'_>, object: RootId) -> Result<(), RootedError> {
    if let Some(index) = connected_index(c, object) {
        let observer = timeline(c, |timeline| timeline.observers.remove(index));
        count_types(c, &observer.types, -1.0)?;
        timeline(c, |timeline| timeline.pending.retain(|&root| root != observer.object));
        for root in observer.buffer.into_iter().chain([observer.object]) {
            c.release_root(root);
        }
    }
    let state = receiver_slot(c, object, api::OBSERVER_SLOT, "PerformanceObserver")?;
    let undefined = c.undefined();
    put(c, state, "kind", undefined)
}

pub(crate) fn observer_disconnect(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Outcome {
    receiver_slot(c, receiver, api::OBSERVER_SLOT, "PerformanceObserver")?;
    disconnect(c, receiver)?;
    Ok(c.undefined())
}

/// The entries the observer buffered, which it no longer holds.
fn take_records(c: &mut Context<'_>, object: RootId) -> Outcome {
    let Some(index) = connected_index(c, object) else {
        return c.array_rooted(&[]);
    };
    let retained = timeline(c, |timeline| std::mem::take(&mut timeline.observers[index].buffer));
    let entries = retained
        .into_iter()
        .filter_map(|root| {
            let scoped = c.scope_root(root);
            c.release_root(root);
            scoped
        })
        .collect::<Vec<_>>();
    c.array_rooted(&entries)
}

pub(crate) fn observer_take_records(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Outcome {
    take_records(c, receiver)
}

pub(crate) fn observer_inspect(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    inspect_as(c, receiver, args, "PerformanceObserver", |c| {
        let index = connected_index(c, receiver);
        let connected = c.boolean(index.is_some());
        let (types, buffer, pending) = match index {
            Some(index) => timeline(c, |timeline| {
                let observer = &timeline.observers[index];
                (observer.types.clone(), observer.buffer.clone(), timeline.pending.iter().any(|&root| root == observer.object))
            }),
            None => (Vec::new(), Vec::new(), false),
        };
        let pending = c.boolean(pending);
        let types = types.iter().map(|kind| c.string_rooted(kind)).collect::<Vec<_>>();
        let types = c.array_rooted(&types)?;
        let buffer = buffer.into_iter().filter_map(|root| c.scope_root(root)).collect::<Vec<_>>();
        let buffer = c.array_rooted(&buffer)?;
        plain_object(c, &[("connected", connected), ("pending", pending), ("entryTypes", types), ("buffer", buffer)])
    })
}

fn list_entries(c: &mut Context<'_>, receiver: RootId) -> Result<Vec<RootId>, RootedError> {
    let entries = receiver_slot(c, receiver, api::LIST_SLOT, "PerformanceObserverEntryList")?;
    c.array_like_values_rooted(entries)
}

pub(crate) fn list_get_entries(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Outcome {
    let entries = list_entries(c, receiver)?;
    c.array_rooted(&entries)
}

fn filtered(c: &mut Context<'_>, entries: Vec<RootId>, name: Option<&str>, kind: Option<&str>) -> Outcome {
    let mut kept = Vec::new();
    for entry in entries {
        let name_matches = match name {
            Some(name) => {
                let value = get(c, entry, "name")?;
                c.string_text(value)?.as_deref() == Some(name)
            }
            None => true,
        };
        let kind_matches = match kind {
            Some(kind) => {
                let value = get(c, entry, "entryType")?;
                c.string_text(value)?.as_deref() == Some(kind)
            }
            None => true,
        };
        if name_matches && kind_matches {
            kept.push(entry);
        }
    }
    c.array_rooted(&kept)
}

pub(crate) fn list_get_entries_by_type(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    let entries = list_entries(c, receiver)?;
    let Some(&kind) = args.first() else {
        return Err(missing_args(c, &["type"]));
    };
    let kind = to_text(c, kind)?;
    filtered(c, entries, None, Some(&kind))
}

pub(crate) fn list_get_entries_by_name(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    let entries = list_entries(c, receiver)?;
    let Some(&name) = args.first() else {
        return Err(missing_args(c, &["name"]));
    };
    let name = to_text(c, name)?;
    let kind = argument(c, args, 1);
    let kind = if is_nullish(c, kind) { None } else { c.string_text(kind)? };
    filtered(c, entries, Some(&name), kind.as_deref())
}

pub(crate) fn list_inspect(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    inspect_as(c, receiver, args, "PerformanceObserverEntryList", |c| {
        let entries = list_entries(c, receiver)?;
        c.array_rooted(&entries)
    })
}

/// Releases the timeline's roots with the heap that held them.
pub(crate) fn release(runtime: &mut Runtime<crate::host::NodeHost>, timeline: Timeline) {
    let Timeline { marks, measures, resources, overflow, observers, .. } = timeline;
    let observed = observers.into_iter().flat_map(|observer| observer.buffer.into_iter().chain([observer.object]));
    for root in marks.into_iter().chain(measures).chain(resources).chain(overflow).chain(observed) {
        runtime.release_root(root);
    }
}

crate::host::startup_snapshot::host_snap_struct!(Observer { object, types, buffer });
crate::host::startup_snapshot::host_snap_struct!(Timeline {
    marks,
    measures,
    resources,
    overflow,
    overflow_pending,
    resource_limit,
    mark_times,
    observers,
    pending,
    dispatch_queued,
    warned,
});
