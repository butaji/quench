use crate::intl_prelude::*;

use quench_runtime::units::NANOSECONDS_PER_DAY;
use crate::temporal_instant_difference::NANOSECONDS_PER_HOUR;
use crate::temporal_instant_difference::NANOSECONDS_PER_MINUTE;
use quench_runtime::units::NANOSECONDS_PER_SECOND;
use quench_runtime::units::NANOSECONDS_PER_MILLISECOND;
use crate::temporal_instant_difference::NANOSECONDS_PER_MICROSECOND;
use crate::temporal_instant_difference::{read_option, read_string_option};
const UNITS: [(&str, i128); 7] = [
    ("day", NANOSECONDS_PER_DAY),
    ("hour", NANOSECONDS_PER_HOUR),
    ("minute", NANOSECONDS_PER_MINUTE),
    ("second", NANOSECONDS_PER_SECOND),
    ("millisecond", NANOSECONDS_PER_MILLISECOND),
    ("microsecond", NANOSECONDS_PER_MICROSECOND),
    ("nanosecond", 1),
];

#[derive(Clone, Copy)]
pub(super) enum RoundingDomain {
    Instant,
    PlainDateTime,
    PlainTime,
}
pub(super) const MODES: [&str; 9] = [
    "ceil",
    "floor",
    "expand",
    "trunc",
    "halfCeil",
    "halfFloor",
    "halfExpand",
    "halfTrunc",
    "halfEven",
];

pub(super) fn round(
    vm: &mut Vm,
    p: &ResidualProgram,
    this: Value,
    args: &[Value],
) -> Result<Value, JsError> {
    let epoch = vm.temporal_instant_epoch(p, this)?;
    let options = args.first().copied().unwrap_or(Value::UNDEFINED);
    let parsed = read_options(vm, p, options)?;
    let (_, scale) = parse_unit(
        vm,
        p,
        parsed.smallest_unit.as_deref(),
        RoundingDomain::Instant,
    )?;
    let increment = validate_increment(vm, p, parsed.increment, scale, RoundingDomain::Instant)?;
    let mode = validate_mode(vm, p, parsed.rounding_mode.as_deref())?;
    let mode = match (epoch < 0, mode) {
        (true, "trunc") => "floor",
        (true, "expand") => "ceil",
        (true, "halfExpand") => "halfCeil",
        _ => mode,
    };
    let rounded = quench_temporal::round_temporal_nanoseconds(epoch, scale * increment, mode)
        * scale
        * increment;
    let constructor = vm.temporal_instant_constructor(p)?;
    vm.make_temporal_instant(p, rounded, constructor)
}

pub(super) struct RoundOptions {
    pub(super) increment: Option<f64>,
    pub(super) rounding_mode: Option<String>,
    pub(super) smallest_unit: Option<String>,
}

pub(super) fn read_options(
    vm: &mut Vm,
    p: &ResidualProgram,
    options: Value,
) -> Result<RoundOptions, JsError> {
    if vm.is_string(options) {
        return Ok(RoundOptions {
            increment: None,
            rounding_mode: None,
            smallest_unit: Some(vm.to_string(p, options)?),
        });
    }
    if options.is_undefined() || options.is_null() || !vm.is_object_like(options) {
        return Err(vm.type_error(p, "Invalid options".into()));
    }
    let increment = read_increment(vm, p, options)?;
    let rounding_mode = read_string_option(vm, p, options, "roundingMode")?;
    let smallest_unit = read_string_option(vm, p, options, "smallestUnit")?;
    Ok(RoundOptions {
        increment,
        rounding_mode,
        smallest_unit,
    })
}

fn read_increment(
    vm: &mut Vm,
    p: &ResidualProgram,
    options: Value,
) -> Result<Option<f64>, JsError> {
    let value = read_option(vm, p, options, "roundingIncrement")?;
    if value.is_undefined() {
        Ok(None)
    } else {
        vm.to_number(p, value).map(Some)
    }
}

pub(super) fn parse_unit(
    vm: &mut Vm,
    p: &ResidualProgram,
    unit: Option<&str>,
    domain: RoundingDomain,
) -> Result<(&'static str, i128), JsError> {
    let unit = unit.ok_or_else(|| vm.range_error(p, "Missing smallestUnit".into()))?;
    let singular = unit.strip_suffix('s').unwrap_or(unit);
    let parsed = UNITS
        .iter()
        .find(|(name, _)| *name == singular)
        .copied()
        .ok_or_else(|| vm.range_error(p, "Invalid smallestUnit".into()))?;
    if parsed.0 == "day" && !matches!(domain, RoundingDomain::PlainDateTime) {
        return Err(vm.range_error(p, "Invalid smallestUnit".into()));
    }
    Ok(parsed)
}

pub(super) fn validate_increment(
    vm: &mut Vm,
    p: &ResidualProgram,
    increment: Option<f64>,
    scale: i128,
    domain: RoundingDomain,
) -> Result<i128, JsError> {
    let increment = increment.unwrap_or(1.0);
    let limit = match domain {
        RoundingDomain::Instant => (NANOSECONDS_PER_DAY / scale) as f64,
        RoundingDomain::PlainDateTime | RoundingDomain::PlainTime => match scale {
            NANOSECONDS_PER_DAY => 1.0,
            NANOSECONDS_PER_HOUR => 24.0,
            NANOSECONDS_PER_MINUTE | NANOSECONDS_PER_SECOND => 60.0,
            NANOSECONDS_PER_MILLISECOND | NANOSECONDS_PER_MICROSECOND | 1 => 1_000.0,
            _ => return Err(vm.range_error(p, "Invalid rounding unit".into())),
        },
    };
    let increment = increment.trunc();
    let exclusive = !matches!(domain, RoundingDomain::Instant)
        && scale != NANOSECONDS_PER_DAY
        && increment >= limit;
    if !increment.is_finite()
        || increment < 1.0
        || increment > limit
        || exclusive
        || limit % increment != 0.0
    {
        return Err(vm.range_error(p, "Invalid roundingIncrement".into()));
    }
    Ok(increment as i128)
}

pub(super) fn validate_mode<'a>(
    vm: &mut Vm,
    p: &ResidualProgram,
    mode: Option<&'a str>,
) -> Result<&'a str, JsError> {
    let mode = mode.unwrap_or("halfExpand");
    MODES
        .contains(&mode)
        .then_some(mode)
        .ok_or_else(|| vm.range_error(p, "Invalid roundingMode".into()))
}
