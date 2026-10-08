use super::*;

const NANOSECONDS_PER_DAY: i128 = 86_400_000_000_000;
const NANOSECONDS_PER_HOUR: i128 = 3_600_000_000_000;
const NANOSECONDS_PER_MINUTE: i128 = 60_000_000_000;
const NANOSECONDS_PER_SECOND: i128 = 1_000_000_000;
const NANOSECONDS_PER_MILLISECOND: i128 = 1_000_000;
const NANOSECONDS_PER_MICROSECOND: i128 = 1_000;
const SUBSECOND_COMPONENTS_PER_UNIT: u32 = 1_000;
const MAX_FRACTIONAL_SECOND_DIGITS: usize = 9;
const TIME_STRING_UNITS: [(&str, i128); 5] = [
    ("minute", NANOSECONDS_PER_MINUTE),
    ("second", NANOSECONDS_PER_SECOND),
    ("millisecond", NANOSECONDS_PER_MILLISECOND),
    ("microsecond", NANOSECONDS_PER_MICROSECOND),
    ("nanosecond", 1),
];
const CALENDAR_NAME_OPTIONS: [&str; 4] = ["auto", "always", "never", "critical"];

pub(super) struct TemporalStringOptions {
    fractional_second_digits: Option<usize>,
    rounding_mode: String,
    smallest_unit: Option<String>,
    calendar_name: String,
    pub(super) time_zone: Option<String>,
}

impl TemporalStringOptions {
    fn quantum(&self) -> Option<i128> {
        self.smallest_unit
            .as_deref()
            .and_then(|unit| TIME_STRING_UNITS.iter().find(|(name, _)| *name == unit))
            .map(|(_, scale)| *scale)
            .or_else(|| {
                self.fractional_second_digits
                    .map(|digits| 10_i128.pow((MAX_FRACTIONAL_SECOND_DIGITS - digits) as u32))
            })
    }

    fn output_digits(&self) -> Option<usize> {
        self.smallest_unit
            .as_deref()
            .and_then(|unit| TIME_STRING_UNITS.iter().position(|(name, _)| *name == unit))
            .map(|index| index.saturating_sub(1) * 3)
            .or(self.fractional_second_digits)
    }
}

pub(super) fn read_options<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    value: Value,
    show_calendar: bool,
) -> Result<TemporalStringOptions, JsError> {
    if value.is_undefined() {
        return Ok(TemporalStringOptions {
            fractional_second_digits: None,
            rounding_mode: "trunc".into(),
            smallest_unit: None,
            calendar_name: "auto".into(),
            time_zone: None,
        });
    }
    if !vm.is_object_like(value) {
        return Err(vm.type_error(p, "Options must be an object".into()));
    }

    let calendar_name = if show_calendar {
        read_string_option(vm, p, value, "calendarName")?.unwrap_or_else(|| "auto".into())
    } else {
        "auto".into()
    };
    let fractional_value = option(vm, p, value, "fractionalSecondDigits")?;
    let fractional_second_digits = if fractional_value.is_undefined() {
        None
    } else if fractional_value.as_number().is_some() {
        let digits = vm.to_number(p, fractional_value)?.floor();
        if !digits.is_finite() || !(0.0..=MAX_FRACTIONAL_SECOND_DIGITS as f64).contains(&digits) {
            return Err(vm.range_error(p, "Invalid fractionalSecondDigits".into()));
        }
        Some(digits as usize)
    } else {
        let digits = vm.to_string(p, fractional_value)?.to_string();
        if digits != "auto" {
            return Err(vm.range_error(p, "Invalid fractionalSecondDigits".into()));
        }
        None
    };
    let rounding_mode =
        read_string_option(vm, p, value, "roundingMode")?.unwrap_or_else(|| "trunc".into());
    let smallest_unit = read_string_option(vm, p, value, "smallestUnit")?
        .map(|unit| unit.strip_suffix('s').unwrap_or(&unit).to_owned());
    let time_zone = if show_calendar {
        None
    } else {
        let time_zone = option(vm, p, value, "timeZone")?;
        (!time_zone.is_undefined())
            .then(|| vm.temporal_timezone_id(p, time_zone))
            .transpose()?
    };
    if !CALENDAR_NAME_OPTIONS.contains(&calendar_name.as_str()) {
        return Err(vm.range_error(p, "Invalid calendarName".into()));
    }
    if !super::temporal_instant_round::MODES.contains(&rounding_mode.as_str()) {
        return Err(vm.range_error(p, "Invalid roundingMode".into()));
    }
    if smallest_unit
        .as_deref()
        .is_some_and(|unit| !TIME_STRING_UNITS.iter().any(|(name, _)| *name == unit))
    {
        return Err(vm.range_error(p, "Invalid smallestUnit".into()));
    }
    Ok(TemporalStringOptions {
        fractional_second_digits,
        rounding_mode,
        smallest_unit,
        calendar_name,
        time_zone,
    })
}

fn option<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    options: Value,
    name: &str,
) -> Result<Value, JsError> {
    let key = vm.intern_atom(name);
    vm.get_property(p, options, key)
}

fn read_string_option<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    options: Value,
    name: &str,
) -> Result<Option<String>, JsError> {
    let value = option(vm, p, options, name)?;
    if value.is_undefined() {
        Ok(None)
    } else {
        vm.to_string(p, value).map(|value| Some(value.to_string()))
    }
}

pub(super) fn format_temporal_datetime(
    total_nanoseconds: i128,
    options: &TemporalStringOptions,
    calendar: Option<&str>,
    suffix: &str,
) -> Option<String> {
    let day_number = total_nanoseconds.div_euclid(NANOSECONDS_PER_DAY);
    let time = total_nanoseconds.rem_euclid(NANOSECONDS_PER_DAY);
    let rounded_time = options.quantum().map_or(time, |quantum| {
        quench_temporal::round_temporal_nanoseconds(time, quantum, &options.rounding_mode) * quantum
    });
    let (day_number, time) = if rounded_time >= NANOSECONDS_PER_DAY {
        (
            day_number.checked_add(1)?,
            rounded_time - NANOSECONDS_PER_DAY,
        )
    } else {
        (day_number, rounded_time)
    };
    let days = i64::try_from(day_number).ok()?;
    let date = quench_temporal::civil_from_days(days)?;
    super::temporal_date::checked_iso_date(date.year, date.month as i32, date.day as i32)?;
    let hour = time / NANOSECONDS_PER_HOUR;
    let minute = time / NANOSECONDS_PER_MINUTE % 60;
    let second = time / NANOSECONDS_PER_SECOND % 60;
    let nanosecond = (time % NANOSECONDS_PER_SECOND) as u32;
    if !super::temporal_plain_date_time_conversion::is_within_bounds(
        (date.year, date.month, date.day),
        [
            hour as i32,
            minute as i32,
            second as i32,
            (nanosecond / NANOSECONDS_PER_MILLISECOND as u32) as i32,
            (nanosecond / NANOSECONDS_PER_MICROSECOND as u32 % SUBSECOND_COMPONENTS_PER_UNIT)
                as i32,
            (nanosecond % NANOSECONDS_PER_MICROSECOND as u32) as i32,
        ],
    ) {
        return None;
    }
    let smallest_unit = options.smallest_unit.as_deref();
    let show_seconds = !matches!(smallest_unit, Some("minute"));
    let digits = options.output_digits();
    let fraction = if !show_seconds || matches!(smallest_unit, Some("second")) {
        String::new()
    } else if let Some(digits) = digits {
        fixed_fraction(nanosecond, digits)
    } else {
        trimmed_fraction(nanosecond)
    };
    let time = if show_seconds {
        format!("{hour:02}:{minute:02}:{second:02}{fraction}")
    } else {
        format!("{hour:02}:{minute:02}")
    };
    let calendar = calendar.filter(|_| {
        options.calendar_name == "always"
            || options.calendar_name == "critical"
            || options.calendar_name == "auto" && calendar.is_some_and(|id| id != "iso8601")
    });
    let calendar_annotation = calendar.map_or_else(String::new, |calendar| {
        let critical = if options.calendar_name == "critical" {
            "!"
        } else {
            ""
        };
        format!("[{critical}u-ca={calendar}]")
    });
    Some(format!(
        "{}T{time}{suffix}{calendar_annotation}",
        super::temporal_date::format_iso_date(date.year, date.month, date.day)
    ))
}

fn fixed_fraction(nanosecond: u32, digits: usize) -> String {
    if digits == 0 {
        return String::new();
    }
    let fraction = format!("{nanosecond:09}");
    format!(".{}", &fraction[..digits])
}

fn trimmed_fraction(nanosecond: u32) -> String {
    let fraction = format!("{nanosecond:09}");
    let fraction = fraction.trim_end_matches('0');
    if fraction.is_empty() {
        String::new()
    } else {
        format!(".{fraction}")
    }
}
