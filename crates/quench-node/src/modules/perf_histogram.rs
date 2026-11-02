//! `createHistogram` and `monitorEventLoopDelay` (`lib/internal/histogram.js`,
//! `lib/internal/perf/event_loop_delay.js`): `Histogram`, `RecordableHistogram` and
//! `ELDHistogram` over an HdrHistogram (`hdr_histogram`).
//!
//! A histogram's private slot holds its record (`HISTOGRAM_FIELDS`): the HdrHistogram's
//! buffer, the `Map` its `percentiles` refills, whether it records, and for an event loop
//! delay histogram its enabled state, timer and resolution.
use super::event_target_shared_vm::{argument, get, is_undefined, put};
use super::hdr_histogram::{buffer_len, Histogram, FIGURES};
use super::perf_hooks::{
    api, api_member, attach_slot, class, define_getter, define_inspect, define_methods, illegal_constructor, inspect_as,
    invalid_this, operation, out_of_range, plain_object, slot, type_error_arg, validate_number, validate_object,
    Context, Method, Outcome,
};
use quench_runtime::{RootId, RootedError, ValueType};

const HISTOGRAM_FIELDS: [&str; 6] = ["state", "map", "recordable", "enabled", "timer", "resolution"];

/// The statistics `Histogram.prototype` reads as getters, in Node's order.
const STATISTICS: [&str; 10] = ["count", "countBigInt", "min", "minBigInt", "max", "maxBigInt", "mean", "exceeds", "exceedsBigInt", "stddev"];
const PERCENTILE_METHODS: [Method; 2] = [("percentile", "perfHistogramPercentile", 1.0), ("percentileBigInt", "perfHistogramPercentileBigInt", 1.0)];
const RECORDABLE_METHODS: [Method; 3] =
    [("record", "perfHistogramRecord", 1.0), ("recordDelta", "perfHistogramRecordDelta", 0.0), ("add", "perfHistogramAdd", 1.0)];
const ELD_METHODS: [Method; 2] = [("enable", "perfEldEnable", 0.0), ("disable", "perfEldDisable", 0.0)];
/// The fields `toJSON` and inspection report, in their orders.
const JSON_FIELDS: [&str; 6] = ["count", "min", "max", "mean", "exceeds", "stddev"];
const INSPECT_FIELDS: [&str; 6] = ["min", "max", "mean", "exceeds", "stddev", "count"];

/// `createHistogram`'s defaults: `lowest`, `highest` (`Number.MAX_SAFE_INTEGER`), `figures`.
const DEFAULT_LOWEST: i64 = 1;
const MAX_SAFE_INTEGER: i64 = (1 << 53) - 1;
const DEFAULT_FIGURES: i64 = 3;
/// The event loop delay histogram's bounds (`ELDHistogram`'s `Histogram::Options { 1000 }`)
/// and default sampling resolution in milliseconds.
const ELD_LOWEST: i64 = 1000;
const ELD_HIGHEST: i64 = i64::MAX;
const DEFAULT_RESOLUTION: f64 = 10.0;
/// A percentile's bounds: above zero, at most 100.
const MAX_PERCENTILE: f64 = 100.0;
const PERCENTILE_RANGE: &str = "> 0 && <= 100";
/// A `Map` preview's entry: the key, then the value.
const MAP_ENTRY_WIDTH: usize = 2;
/// The base of a bigint's decimal digits, which `BigInt.asIntN(64, value)` reads.
const DECIMAL_BASE: u64 = 10;

/// The histogram classes and the module's factories.
pub(super) fn build(c: &mut Context<'_>) -> Result<(RootId, RootId), RootedError> {
    let histogram = class(c, "Histogram", "perfHistogramConstructor", 0.0, None)?;
    let prototype = get(c, histogram, "prototype")?;
    define_inspect(c, prototype, "perfHistogramInspect")?;
    for name in STATISTICS {
        let statistic = c.string_rooted(name);
        define_getter(c, prototype, name, "perfHistogramStatistic", statistic, false)?;
    }
    define_methods(c, prototype, &PERCENTILE_METHODS, false)?;
    for (name, big) in [("percentiles", false), ("percentilesBigInt", true)] {
        let big = c.boolean(big);
        define_getter(c, prototype, name, "perfHistogramPercentiles", big, false)?;
    }
    define_methods(c, prototype, &[("reset", "perfHistogramReset", 0.0), ("toJSON", "perfHistogramToJson", 0.0)], false)?;

    let recordable = class(c, "RecordableHistogram", "perfHistogramConstructor", 0.0, Some(histogram))?;
    let recordable_prototype = get(c, recordable, "prototype")?;
    define_methods(c, recordable_prototype, &RECORDABLE_METHODS, false)?;
    let shared = super::perf_hooks::shared_api(c)?;
    put(c, shared, api::RECORDABLE_CLASS, recordable)?;
    put(c, shared, api::RECORDABLE_PROTOTYPE, recordable_prototype)?;

    let eld = class(c, "ELDHistogram", "perfHistogramConstructor", 1.0, Some(histogram))?;
    let eld_prototype = get(c, eld, "prototype")?;
    define_methods(c, eld_prototype, &ELD_METHODS, false)?;
    put(c, shared, api::ELD_PROTOTYPE, eld_prototype)?;

    let create = c.host_function_named(operation("perfCreateHistogram"), "createHistogram")?;
    let monitor = c.host_function_named(operation("perfMonitorEventLoopDelay"), "monitorEventLoopDelay")?;
    for function in [create, monitor] {
        let zero = c.number(0.0);
        c.define_named_data_property_rooted(function, "length", zero, false, false, true)?;
    }
    Ok((create, monitor))
}

pub(crate) fn histogram_constructor(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    Err(illegal_constructor(c))
}

/// `isHistogram(value)`: it carries a histogram's state.
pub(super) fn is_histogram(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    Ok(slot(c, value, api::HISTOGRAM_SLOT)?.is_some())
}

/// A histogram of `prototype`'s class over a new HdrHistogram of these bounds.
fn create(c: &mut Context<'_>, prototype: RootId, bounds: (i64, i64, i64), recordable: bool) -> Outcome {
    let (lowest, highest, figures) = bounds;
    let Some(length) = buffer_len(lowest, highest, figures) else {
        let error = c.error_rooted("The histogram could not be created")?;
        return Err(c.throw(error));
    };
    let mut bytes = vec![0; length];
    Histogram::initialize(&mut bytes, lowest, highest, figures);
    let state = c.uint8_array_from_rooted(bytes)?;
    let map = api_member(c, api::MAP)?;
    let map = c.construct_rooted(map, map, &[])?;
    let recordable = c.boolean(recordable);
    let undefined = c.undefined();
    let record = c.record_rooted(&HISTOGRAM_FIELDS, &[state, map, recordable, undefined, undefined, undefined])?;
    let histogram = c.object_rooted()?;
    c.set_prototype_rooted(histogram, prototype)?;
    attach_slot(c, histogram, api::HISTOGRAM_SLOT, record)?;
    Ok(histogram)
}

fn record_of(c: &mut Context<'_>, receiver: RootId, class: &str) -> Outcome {
    match slot(c, receiver, api::HISTOGRAM_SLOT)? {
        Some(record) => Ok(record),
        None => Err(invalid_this(c, class)),
    }
}

/// `f` over the receiver's HdrHistogram.
fn with_histogram<T>(c: &mut Context<'_>, record: RootId, f: impl FnOnce(&mut Histogram<'_>) -> T) -> Result<T, RootedError> {
    let state = get(c, record, "state")?;
    c.with_buffer_bytes_mut_rooted(state, |bytes| f(&mut Histogram::open(bytes)))
        .ok_or_else(|| RootedError::host("histogram state is not a buffer"))
}

fn integer(c: &mut Context<'_>, value: i64, big: bool) -> RootId {
    if big { c.bigint_rooted(i128::from(value)) } else { c.number(value as f64) }
}

/// A statistic getter: the one its data names.
pub(crate) fn statistic(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Outcome {
    let name = c.host_function_data()?;
    let name = c.to_string(name)?;
    let record = record_of(c, receiver, "Histogram")?;
    let (base, big) = match name.strip_suffix("BigInt") {
        Some(base) => (base, true),
        None => (name.as_str(), false),
    };
    let value = with_histogram(c, record, |histogram| match base {
        "count" => Ok(histogram.count()),
        "min" => Ok(histogram.min()),
        "max" => Ok(histogram.max()),
        "exceeds" => Ok(histogram.exceeds()),
        "mean" => Err(histogram.mean()),
        _ => Err(histogram.stddev()),
    })?;
    Ok(match value {
        Ok(integer_value) => integer(c, integer_value, big),
        Err(real) => c.number(real),
    })
}

/// A percentile argument: a number in (0, 100].
fn percentile_argument(c: &mut Context<'_>, args: &[RootId]) -> Result<f64, RootedError> {
    let value = argument(c, args, 0);
    let percentile = validate_number(c, value, "percentile")?;
    if percentile.is_nan() || percentile <= 0.0 || percentile > MAX_PERCENTILE {
        return Err(out_of_range(c, "percentile", PERCENTILE_RANGE, value));
    }
    Ok(percentile)
}

fn percentile_of(c: &mut Context<'_>, receiver: RootId, args: &[RootId], big: bool) -> Outcome {
    let record = record_of(c, receiver, "Histogram")?;
    let percentile = percentile_argument(c, args)?;
    let value = with_histogram(c, record, |histogram| histogram.percentile(percentile))?;
    Ok(integer(c, value, big))
}

pub(crate) fn percentile(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    percentile_of(c, receiver, args, false)
}

pub(crate) fn percentile_big_int(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    percentile_of(c, receiver, args, true)
}

/// `get percentiles`: the histogram's one map, refilled with its percentile iteration.
pub(crate) fn percentiles(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Outcome {
    let big = c.host_function_data()?;
    let big = c.truthy_rooted(big)?;
    let record = record_of(c, receiver, "Histogram")?;
    let map = get(c, record, "map")?;
    let clear = api_member(c, api::MAP_CLEAR)?;
    c.call_rooted(clear, map, &[])?;
    let set = api_member(c, api::MAP_SET)?;
    for (percentile, value) in with_histogram(c, record, |histogram| histogram.percentiles())? {
        let key = c.number(percentile);
        let value = integer(c, value, big);
        c.call_rooted(set, map, &[key, value])?;
    }
    Ok(map)
}

pub(crate) fn reset(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Outcome {
    let record = record_of(c, receiver, "Histogram")?;
    with_histogram(c, record, |histogram| histogram.reset())?;
    Ok(c.undefined())
}

pub(crate) fn to_json(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Outcome {
    let mut fields = Vec::new();
    for name in JSON_FIELDS {
        fields.push((name, get(c, receiver, name)?));
    }
    let percentiles = get(c, receiver, "percentiles")?;
    let entries = c.collection_preview_rooted(percentiles)?.map(|preview| preview.entries).unwrap_or_default();
    let object = c.object_rooted()?;
    for &[key, value] in entries.as_chunks::<MAP_ENTRY_WIDTH>().0 {
        c.set_property_rooted(object, key, value, object)?;
    }
    fields.push(("percentiles", object));
    plain_object(c, &fields)
}

pub(crate) fn inspect(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    inspect_as(c, receiver, args, "Histogram", |c| {
        let mut fields = Vec::new();
        for name in INSPECT_FIELDS.into_iter().chain(["percentiles"]) {
            fields.push((name, get(c, receiver, name)?));
        }
        plain_object(c, &fields)
    })
}

fn recordable_record(c: &mut Context<'_>, receiver: RootId) -> Outcome {
    let record = slot(c, receiver, api::HISTOGRAM_SLOT)?;
    if let Some(record) = record {
        let recordable = get(c, record, "recordable")?;
        if c.truthy_rooted(recordable)? {
            return Ok(record);
        }
    }
    Err(invalid_this(c, "RecordableHistogram"))
}

/// A bigint's value modulo 2^64, as `BigInt::Int64Value` reads it, and whether that is
/// its value.
fn bigint_i64(c: &mut Context<'_>, value: RootId) -> Result<(i64, bool), RootedError> {
    let text = c.to_string(value)?;
    if let Ok(exact) = text.parse::<i64>() {
        return Ok((exact, true));
    }
    let (negative, digits) = match text.strip_prefix('-') {
        Some(digits) => (true, digits),
        None => (false, text.as_str()),
    };
    let magnitude = digits
        .bytes()
        .fold(0u64, |total, digit| total.wrapping_mul(DECIMAL_BASE).wrapping_add(u64::from(digit - b'0')));
    let wrapped = if negative { magnitude.wrapping_neg() } else { magnitude };
    Ok((wrapped as i64, false))
}

/// `record(val)`: an integer, or a bigint within 64 bits, of at least 1.
pub(crate) fn record(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    let record = recordable_record(c, receiver)?;
    let value = argument(c, args, 0);
    let value = if c.value_type_rooted(value) == Some(ValueType::BigInt) {
        match bigint_i64(c, value)? {
            (value, true) if value >= 1 => value,
            _ => return Err(super::perf_hooks::coded_error(c, true, "ERR_OUT_OF_RANGE", "value is out of range")),
        }
    } else {
        let maximum = c.number(MAX_SAFE_INTEGER as f64);
        super::util_shared_vm::validate_integer(c, value, "val", 1.0, maximum)? as i64
    };
    with_histogram(c, record, |histogram| histogram.record(value))?;
    Ok(c.undefined())
}

fn clock_ns(c: &mut Context<'_>) -> i64 {
    let control = c.host_mut().shared_state().borrow().process_control.clone();
    control.monotonic_now_ns().unwrap_or_default() as i64
}

pub(crate) fn record_delta(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Outcome {
    let record = recordable_record(c, receiver)?;
    let now = clock_ns(c);
    with_histogram(c, record, |histogram| histogram.record_delta(now))?;
    Ok(c.undefined())
}

/// `add(other)`: another recordable histogram's counts added to this one's.
pub(crate) fn add(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Outcome {
    let record = recordable_record(c, receiver)?;
    let other = argument(c, args, 0);
    let other_record = match slot(c, other, api::HISTOGRAM_SLOT)? {
        Some(other_record) => {
            let recordable = get(c, other_record, "recordable")?;
            c.truthy_rooted(recordable)?.then_some(other_record)
        }
        None => None,
    };
    let Some(other_record) = other_record else {
        return Err(type_error_arg(c, other, "other", "RecordableHistogram"));
    };
    let other_state = get(c, other_record, "state")?;
    let mut other_bytes = c
        .with_buffer_bytes_rooted(other_state, <[u8]>::to_vec)
        .ok_or_else(|| RootedError::host("histogram state is not a buffer"))?;
    let other = Histogram::open(&mut other_bytes);
    with_histogram(c, record, |histogram| histogram.add(&other))?;
    Ok(c.undefined())
}

/// An integer option: a bigint as is, else a safe integer at least `minimum`.
fn bound(c: &mut Context<'_>, value: RootId, name: &str, minimum: i64) -> Result<i64, RootedError> {
    if c.value_type_rooted(value) == Some(ValueType::BigInt) {
        return Ok(bigint_i64(c, value)?.0);
    }
    let maximum = c.number(MAX_SAFE_INTEGER as f64);
    Ok(super::util_shared_vm::validate_integer(c, value, name, minimum as f64, maximum)? as i64)
}

fn option_or(c: &mut Context<'_>, options: RootId, name: &str, default: f64) -> Outcome {
    let value = get(c, options, name)?;
    Ok(if is_undefined(c, value) { c.number(default) } else { value })
}

/// `createHistogram(options)`: a recordable histogram of `lowest`, `highest` and `figures`.
pub(crate) fn create_histogram(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let options = argument(c, args, 0);
    let options = if is_undefined(c, options) { c.object_rooted()? } else { options };
    validate_object(c, options, "options")?;
    let lowest_value = option_or(c, options, "lowest", DEFAULT_LOWEST as f64)?;
    let highest_value = option_or(c, options, "highest", MAX_SAFE_INTEGER as f64)?;
    let figures_value = option_or(c, options, "figures", DEFAULT_FIGURES as f64)?;
    let lowest = bound(c, lowest_value, "options.lowest", DEFAULT_LOWEST)?;
    let highest = if c.value_type_rooted(highest_value) == Some(ValueType::BigInt) {
        let highest = bigint_i64(c, highest_value)?.0;
        if i128::from(highest) < 2 * i128::from(lowest) {
            let error = match super::util_shared_vm::argument_value_error(c, highest_value, "options.highest", "is invalid") {
                Ok(error) | Err(error) => error,
            };
            return Err(as_range_error(c, error));
        }
        highest
    } else {
        bound(c, highest_value, "options.highest", 2 * lowest)?
    };
    let maximum = c.number(*FIGURES.end() as f64);
    let figures = super::util_shared_vm::validate_integer(c, figures_value, "options.figures", *FIGURES.start() as f64, maximum)? as i64;
    let prototype = api_member(c, api::RECORDABLE_PROTOTYPE)?;
    let histogram = create(c, prototype, (lowest, highest, figures), true)?;
    let class = api_member(c, api::RECORDABLE_CLASS)?;
    put(c, histogram, "constructor", class)?;
    Ok(histogram)
}

/// `ERR_INVALID_ARG_VALUE.RangeError`: the thrown TypeError's message and code on a RangeError.
fn as_range_error(c: &mut Context<'_>, error: RootedError) -> RootedError {
    let Some(thrown) = error.exception else {
        return error;
    };
    let rebuilt = (|| {
        let message = get(c, thrown, "message")?;
        let message = c.to_string(message)?;
        let range = c.range_error_rooted(&message)?;
        super::node_errors_shared_vm::decorate_range_error(c, range, "ERR_INVALID_ARG_VALUE")?;
        Ok(c.throw(range))
    })();
    rebuilt.unwrap_or_else(|error| error)
}

/// `monitorEventLoopDelay(options)`: a histogram of the delays between timer ticks every
/// `options.resolution` milliseconds while enabled.
pub(crate) fn monitor_event_loop_delay(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let options = argument(c, args, 0);
    let options = if is_undefined(c, options) { c.object_rooted()? } else { options };
    validate_object(c, options, "options")?;
    let resolution = option_or(c, options, "resolution", DEFAULT_RESOLUTION)?;
    let maximum = c.number(MAX_SAFE_INTEGER as f64);
    super::util_shared_vm::validate_integer(c, resolution, "options.resolution", 1.0, maximum)?;
    let prototype = api_member(c, api::ELD_PROTOTYPE)?;
    let histogram = create(c, prototype, (ELD_LOWEST, ELD_HIGHEST, DEFAULT_FIGURES), false)?;
    let record = record_of(c, histogram, "ELDHistogram")?;
    let disabled = c.boolean(false);
    put(c, record, "enabled", disabled)?;
    put(c, record, "resolution", resolution)?;
    Ok(histogram)
}

fn eld_record(c: &mut Context<'_>, receiver: RootId) -> Result<(RootId, bool), RootedError> {
    if let Some(record) = slot(c, receiver, api::HISTOGRAM_SLOT)? {
        let enabled = get(c, record, "enabled")?;
        if let Some(enabled) = c.rooted_value(enabled).and_then(|value| value.as_bool()) {
            return Ok((record, enabled));
        }
    }
    Err(invalid_this(c, "ELDHistogram"))
}

/// `enable()`: whether sampling started, its unreferenced timer recording each tick's delay.
pub(crate) fn eld_enable(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Outcome {
    let (record, enabled) = eld_record(c, receiver)?;
    if enabled {
        return Ok(c.boolean(false));
    }
    let yes = c.boolean(true);
    put(c, record, "enabled", yes)?;
    let tick = c.host_function_with_data(operation("perfEldTick"), record)?;
    let resolution = get(c, record, "resolution")?;
    let timer = super::timers_shared_vm::schedule_timer(c, &[tick, resolution], true, false)?;
    put(c, record, "timer", timer)?;
    Ok(c.boolean(true))
}

/// `disable()`: whether sampling stopped.
pub(crate) fn eld_disable(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Outcome {
    let (record, enabled) = eld_record(c, receiver)?;
    if !enabled {
        return Ok(c.boolean(false));
    }
    let no = c.boolean(false);
    put(c, record, "enabled", no)?;
    let timer = get(c, record, "timer")?;
    let undefined = c.undefined();
    super::timers_shared_vm::clear_timer(c, undefined, &[timer])?;
    Ok(c.boolean(true))
}

pub(crate) fn eld_tick(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let record = c.host_function_data()?;
    let now = clock_ns(c);
    with_histogram(c, record, |histogram| histogram.record_delta(now))?;
    Ok(c.undefined())
}
