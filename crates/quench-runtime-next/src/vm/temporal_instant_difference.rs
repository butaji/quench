use super::*;

const NANOSECONDS_PER_HOUR: i128 = 3_600_000_000_000;
const NANOSECONDS_PER_MINUTE: i128 = 60_000_000_000;
const NANOSECONDS_PER_SECOND: i128 = 1_000_000_000;
const NANOSECONDS_PER_MILLISECOND: i128 = 1_000_000;
const NANOSECONDS_PER_MICROSECOND: i128 = 1_000;
const UNITS: [(&str, i128, usize); 6] = [
    ("hour", NANOSECONDS_PER_HOUR, 4),
    ("minute", NANOSECONDS_PER_MINUTE, 5),
    ("second", NANOSECONDS_PER_SECOND, 6),
    ("millisecond", NANOSECONDS_PER_MILLISECOND, 7),
    ("microsecond", NANOSECONDS_PER_MICROSECOND, 8),
    ("nanosecond", 1, 9),
];
const ROUNDING_MODES: [&str; 9] = [
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
const DEFAULT_ROUNDING_MODE: &str = "trunc";
const DEFAULT_SMALLEST_UNIT: &str = "nanosecond";
const DEFAULT_LARGEST_UNIT: &str = "second";

pub(super) fn difference<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    native: Native,
    this: Value,
    args: &[Value],
) -> Result<Value, JsError> {
    let left = vm.temporal_instant_epoch(p, this)?;
    let right = vm.temporal_instant_input(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
    let delta = difference_delta(left, right, native);
    let options = read_options(vm, p, args.get(1).copied())?;
    let (smallest, largest, increment, mode) = normalize_options(options);
    let smallest = unit(vm, p, &smallest)?;
    let largest = unit(vm, p, &largest)?;
    validate_units(vm, p, smallest, largest, increment, &mode)?;
    let quantum = smallest.1 * increment as i128;
    let rounded =
        super::temporal_zoned_date_time::round_temporal_nanoseconds(delta, quantum, &mode)
            * quantum;
    let fields = decompose(rounded, smallest, largest);
    vm.make_temporal_duration(p, fields)
}

fn difference_delta(left: i128, right: i128, native: Native) -> i128 {
    match native {
        Native::TemporalInstantUntil => right - left,
        Native::TemporalInstantSince => left - right,
        _ => unreachable!("not an Instant difference native"),
    }
}

#[derive(Default)]
struct DifferenceOptions {
    largest_unit: Option<String>,
    increment: Option<f64>,
    rounding_mode: Option<String>,
    smallest_unit: Option<String>,
}

fn read_options<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    options: Option<Value>,
) -> Result<DifferenceOptions, JsError> {
    let Some(options) = options.filter(|value| !value.is_undefined()) else {
        return Ok(DifferenceOptions::default());
    };
    if !vm.is_object_like(options) {
        return Err(vm.type_error(p, "Invalid options".into()));
    }
    let largest_unit = read_string_option(vm, p, options, "largestUnit")?;
    let increment = read_number_option(vm, p, options, "roundingIncrement")?;
    let rounding_mode = read_string_option(vm, p, options, "roundingMode")?;
    let smallest_unit = read_string_option(vm, p, options, "smallestUnit")?;
    Ok(DifferenceOptions {
        largest_unit,
        increment,
        rounding_mode,
        smallest_unit,
    })
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

fn read_number_option<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    options: Value,
    name: &str,
) -> Result<Option<f64>, JsError> {
    let value = read_option(vm, p, options, name)?;
    if value.is_undefined() {
        Ok(None)
    } else {
        vm.to_number(p, value).map(|number| Some(number.trunc()))
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

fn normalize_options(mut options: DifferenceOptions) -> (String, String, f64, String) {
    let smallest = options
        .smallest_unit
        .take()
        .unwrap_or_else(|| DEFAULT_SMALLEST_UNIT.into());
    let largest = options.largest_unit.take().unwrap_or_else(|| {
        if matches!(smallest.as_str(), "hour" | "hours" | "minute" | "minutes") {
            smallest.clone()
        } else {
            DEFAULT_LARGEST_UNIT.into()
        }
    });
    (
        smallest.strip_suffix('s').unwrap_or(&smallest).into(),
        largest.strip_suffix('s').unwrap_or(&largest).into(),
        options.increment.unwrap_or(1.0),
        options
            .rounding_mode
            .take()
            .unwrap_or_else(|| DEFAULT_ROUNDING_MODE.into()),
    )
}

fn unit<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    value: &str,
) -> Result<(&'static str, i128, usize), JsError> {
    UNITS
        .iter()
        .find(|(name, _, _)| *name == value)
        .copied()
        .ok_or_else(|| vm.range_error(p, "Invalid time unit".into()))
}

fn validate_units<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    smallest: (&str, i128, usize),
    largest: (&str, i128, usize),
    increment: f64,
    mode: &str,
) -> Result<(), JsError> {
    let maximum = increment_limit(smallest.0);
    if !increment.is_finite() || increment < 1.0 || increment as i128 >= maximum {
        return Err(vm.range_error(p, "Invalid roundingIncrement".into()));
    }
    if maximum % increment as i128 != 0 {
        return Err(vm.range_error(p, "Invalid roundingIncrement".into()));
    }
    if largest.1 < smallest.1 {
        return Err(vm.range_error(p, "Invalid unit relationship".into()));
    }
    if !ROUNDING_MODES.contains(&mode) {
        return Err(vm.range_error(p, "Invalid roundingMode".into()));
    }
    Ok(())
}

fn increment_limit(unit: &str) -> i128 {
    match unit {
        "hour" => 24,
        "minute" | "second" => 60,
        "millisecond" | "microsecond" | "nanosecond" => 1_000,
        _ => 0,
    }
}

fn decompose(
    mut delta: i128,
    smallest: (&str, i128, usize),
    largest: (&str, i128, usize),
) -> [f64; 10] {
    let mut fields = [0.0; 10];
    for (_, scale, index) in UNITS
        .iter()
        .copied()
        .filter(|unit| unit.1 <= largest.1 && unit.1 >= smallest.1)
    {
        fields[index] = (delta / scale) as f64;
        delta %= scale;
        if scale == smallest.1 {
            break;
        }
    }
    fields
}
