use super::*;

const NANOSECONDS_PER_DAY: i128 = 86_400_000_000_000;
const NANOSECONDS_PER_HOUR: i128 = 3_600_000_000_000;
const NANOSECONDS_PER_MINUTE: i128 = 60_000_000_000;
const NANOSECONDS_PER_SECOND: i128 = 1_000_000_000;
const NANOSECONDS_PER_MILLISECOND: i128 = 1_000_000;
const NANOSECONDS_PER_MICROSECOND: i128 = 1_000;
const DAY_ROUNDING_INCREMENT_LIMIT: f64 = 1.0;
const HOUR_ROUNDING_INCREMENT_LIMIT: f64 = 24.0;
const SEXAGESIMAL_ROUNDING_INCREMENT_LIMIT: f64 = 60.0;
const SUBSECOND_ROUNDING_INCREMENT_LIMIT: f64 = 1_000.0;
const UNITS: [(&str, i128); 7] = [
    ("day", NANOSECONDS_PER_DAY),
    ("hour", NANOSECONDS_PER_HOUR),
    ("minute", NANOSECONDS_PER_MINUTE),
    ("second", NANOSECONDS_PER_SECOND),
    ("millisecond", NANOSECONDS_PER_MILLISECOND),
    ("microsecond", NANOSECONDS_PER_MICROSECOND),
    ("nanosecond", 1),
];
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

pub(super) fn round<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    this: Value,
    args: &[Value],
) -> Result<Value, JsError> {
    let epoch = vm.temporal_instant_epoch(p, this)?;
    let options = args.first().copied().unwrap_or(Value::UNDEFINED);
    let parsed = read_options(vm, p, options)?;
    let (_, scale) = parse_unit(vm, p, parsed.smallest_unit.as_deref())?;
    let increment = validate_increment(vm, p, parsed.increment, scale)?;
    let mode = validate_mode(vm, p, parsed.rounding_mode.as_deref())?;
    let mode = match (epoch < 0, mode) {
        (true, "trunc") => "floor",
        (true, "expand") => "ceil",
        (true, "halfExpand") => "halfCeil",
        _ => mode,
    };
    let rounded =
        super::temporal_zoned_date_time::round_temporal_nanoseconds(epoch, scale * increment, mode)
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

pub(super) fn read_options<H: Host>(
    vm: &mut Vm<H>,
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

fn read_increment<H: Host>(
    vm: &mut Vm<H>,
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

fn read_string_option<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    options: Value,
    name: &str,
) -> Result<Option<String>, JsError> {
    let value = read_option(vm, p, options, name)?;
    if value.is_undefined() {
        Ok(None)
    } else {
        vm.to_string(p, value).map(Some)
    }
}

fn read_option<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    options: Value,
    name: &str,
) -> Result<Value, JsError> {
    let key = vm.intern_atom(name);
    vm.get_property(p, options, key)
}

pub(super) fn parse_unit<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    unit: Option<&str>,
) -> Result<(&'static str, i128), JsError> {
    let unit = unit.ok_or_else(|| vm.range_error(p, "Missing smallestUnit".into()))?;
    let singular = unit.strip_suffix('s').unwrap_or(unit);
    UNITS
        .iter()
        .find(|(name, _)| *name == singular)
        .copied()
        .ok_or_else(|| vm.range_error(p, "Invalid smallestUnit".into()))
}

pub(super) fn validate_increment<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    increment: Option<f64>,
    scale: i128,
) -> Result<i128, JsError> {
    let increment = increment.unwrap_or(1.0);
    let limit = match scale {
        NANOSECONDS_PER_DAY => DAY_ROUNDING_INCREMENT_LIMIT,
        NANOSECONDS_PER_HOUR => HOUR_ROUNDING_INCREMENT_LIMIT,
        NANOSECONDS_PER_MINUTE | NANOSECONDS_PER_SECOND => SEXAGESIMAL_ROUNDING_INCREMENT_LIMIT,
        NANOSECONDS_PER_MILLISECOND | NANOSECONDS_PER_MICROSECOND | 1 => {
            SUBSECOND_ROUNDING_INCREMENT_LIMIT
        }
        _ => return Err(vm.range_error(p, "Invalid rounding unit".into())),
    };
    let increment = increment.trunc();
    let at_or_above_exclusive_limit = scale != NANOSECONDS_PER_DAY && increment >= limit;
    if !increment.is_finite()
        || increment < 1.0
        || increment > limit
        || at_or_above_exclusive_limit
        || limit % increment != 0.0
    {
        return Err(vm.range_error(p, "Invalid roundingIncrement".into()));
    }
    Ok(increment as i128)
}

pub(super) fn validate_mode<'a, H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    mode: Option<&'a str>,
) -> Result<&'a str, JsError> {
    let mode = mode.unwrap_or("halfExpand");
    MODES
        .contains(&mode)
        .then_some(mode)
        .ok_or_else(|| vm.range_error(p, "Invalid roundingMode".into()))
}
