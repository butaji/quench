use super::*;
const MAX_ISO_TIME_ZONE_OFFSET_DIGITS: usize = 5;

const MIN_PLAIN_DATE_TIME_DATE: (i32, u32, u32) = (-271_821, 4, 19);
pub(super) const TIME_FIELDS: [&str; 6] = [
    "hour",
    "microsecond",
    "millisecond",
    "minute",
    "nanosecond",
    "second",
];
pub(super) const TIME_FIELDS_BEFORE_MONTH: usize = 4;
pub(super) const TIME_LIMITS: [i32; 6] = [23, 999, 999, 59, 999, 59];
const DEFAULT_DATE_PREFIX: &str = "1970-01-01T";
const ISO_YEAR_DIGITS: usize = 4;
const ISO_MONTH_DIGITS: usize = 2;
const FRACTIONAL_SECOND_DIGITS: usize = 9;

pub(super) fn convert<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    this: Value,
    args: &[Value],
) -> Result<Value, JsError> {
    let (year, month, day, calendar) = vm.temporal_plain_date_slots(p, this)?;
    let value = args.first().copied().unwrap_or(Value::UNDEFINED);
    let time = to_time(vm, p, value)?;
    validate_bounds(vm, p, year, month, day, time)?;
    construct(vm, p, year, month, day, calendar, time)
}

pub(super) fn to_time<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    value: Value,
) -> Result<[i32; 6], JsError> {
    to_time_with_options(vm, p, value, Value::UNDEFINED)
}

pub(super) fn to_time_with_options<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    value: Value,
    options: Value,
) -> Result<[i32; 6], JsError> {
    if value.is_undefined() {
        return Ok([0; 6]);
    }
    if let Some(Cell::TemporalPlainDateTime { .. }) = vm.heap.get(value) {
        return Ok(vm
            .temporal_plain_date_time_slots(p, value)?
            .1
            .map(|part| part as i32));
    }
    if let Some(Cell::TemporalZonedDateTime {
        epoch_nanoseconds,
        time_zone,
        ..
    }) = vm.heap.get(value)
    {
        let local =
            super::temporal_zoned_date_time::zoned_date_time_fields(*epoch_nanoseconds, time_zone)
                .ok_or_else(|| vm.range_error(p, "Invalid time".into()))?;
        return Ok([local[3], local[4], local[5], local[6], local[7], local[8]]);
    }
    if vm.is_string(value) {
        let time = parse_time_string(vm, p, value)?;
        let _ = vm.plain_date_overflow(p, options)?;
        return Ok(time);
    }
    if !vm.is_object_like(value) {
        return Err(vm.type_error(p, "Invalid time".into()));
    }
    read_time_bag(
        vm,
        p,
        value,
        true,
        (!options.is_undefined()).then_some(options),
    )
}

pub(super) fn to_date_time<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    value: Value,
    options: Value,
) -> Result<[i32; 6], JsError> {
    if matches!(vm.heap.get(value), Some(Cell::TemporalPlainDate { .. })) {
        let _ = vm.plain_date_overflow(p, options)?;
        return Ok([0; 6]);
    }
    if !vm.is_object_like(value)
        || matches!(
            vm.heap.get(value),
            Some(Cell::TemporalPlainDateTime { .. } | Cell::TemporalZonedDateTime { .. })
        )
    {
        let time = to_time(vm, p, value)?;
        let _ = vm.plain_date_overflow(p, options)?;
        return Ok(time);
    }
    read_time_bag(vm, p, value, false, Some(options))
}

fn read_time_bag<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    bag: Value,
    require_any: bool,
    options: Option<Value>,
) -> Result<[i32; 6], JsError> {
    let mut fields = [None; 6];
    for (index, name) in TIME_FIELDS.iter().enumerate() {
        let key = vm.intern_atom(name);
        let value = vm.get_property(p, bag, key)?;
        if !value.is_undefined() {
            fields[index] = Some(vm.plain_date_integer(p, value)?);
        }
    }
    if require_any && fields.iter().all(Option::is_none) {
        return Err(vm.type_error(p, "Missing hour".into()));
    }
    let constrain = match options {
        Some(options) => vm.plain_date_overflow(p, options)?,
        None => true,
    };
    let bag_fields = fields
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            let value = value.unwrap_or_default();
            if constrain {
                value.clamp(0, TIME_LIMITS[index])
            } else {
                value
            }
        })
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_| JsError("invalid time field width".into()))?;
    let [hour, microsecond, millisecond, minute, nanosecond, second] = bag_fields;
    let time = [hour, minute, second, millisecond, microsecond, nanosecond];
    if constrain {
        Ok(time)
    } else {
        vm.validate_plain_date_time_time(p, &time)?;
        Ok(time)
    }
}

pub(super) fn parse_time_string<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    value: Value,
) -> Result<[i32; 6], JsError> {
    let text = vm.to_string(p, value)?;
    validate_annotations(vm, p, &text)?;
    let undecorated = text.split('[').next().unwrap_or(&text);
    if undecorated.contains(['Z', 'z']) || undecorated.starts_with("-000000") {
        return Err(vm.range_error(p, "Invalid time string".into()));
    }
    let base = text.split(['[', 'Z', 'z']).next().unwrap_or(&text);
    if base.contains('−') {
        return Err(vm.range_error(p, "Invalid time string".into()));
    }
    let normalized = normalize_unambiguous_time(base).or_else(|| {
        base.strip_prefix(['T', 't'])
            .and_then(normalize_disambiguated_time)
    });
    let time = normalized
        .map(str::to_owned)
        .unwrap_or_else(|| base.to_owned());
    let compact_normalized = time
        .strip_prefix(['T', 't'])
        .map(normalize_compact_time)
        .unwrap_or_default();
    let time = if compact_normalized.is_empty() {
        time.as_str()
    } else {
        compact_normalized.as_str()
    };
    let time = time
        .rsplit_once(['T', 't'])
        .map_or(time, |(_, suffix)| suffix);
    let time = time.rsplit_once(' ').map_or(time, |(_, suffix)| suffix);
    if time.contains(['-', '+']) && !time.starts_with(['T', 't']) {
        if time.matches('-').count() >= 2 && !time.contains(['T', 't', ' ']) {
            return Err(vm.range_error(p, "Invalid time string".into()));
        }
    }
    let time = time.strip_prefix(['T', 't']).unwrap_or(time);
    if is_ambiguous_time(time) {
        return Err(vm.range_error(p, "Ambiguous PlainTime string".into()));
    }
    if time.rfind(['.', ',']).is_some_and(|index| {
        time[index + 1..]
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count()
            > FRACTIONAL_SECOND_DIGITS
    }) {
        return Err(vm.range_error(p, "Invalid time string".into()));
    }
    let time = strip_time_offset(time);
    if time.is_empty() {
        return Err(vm.range_error(p, "Invalid time string".into()));
    }
    if is_ambiguous_time(time) {
        return Err(vm.range_error(p, "Ambiguous PlainTime string".into()));
    }
    let source = format!("{DEFAULT_DATE_PREFIX}{time}");
    let (local, _, _) = super::temporal_zoned_date_time::parse_iso_zoned_base(&source)
        .ok_or_else(|| vm.range_error(p, "Invalid time string".into()))?;
    use chrono::Timelike;
    Ok([
        local.hour() as i32,
        local.minute() as i32,
        local.second() as i32,
        local.nanosecond() as i32 / 1_000_000,
        local.nanosecond() as i32 / 1_000 % 1_000,
        local.nanosecond() as i32 % 1_000,
    ])
}

fn is_ambiguous_time(text: &str) -> bool {
    let text = text.split('[').next().unwrap_or(text);
    let ascii_digits = |value: &str| value.bytes().all(|byte| byte.is_ascii_digit());
    if let Some((year, month)) = text.split_once('-') {
        if year.len() == ISO_YEAR_DIGITS
            && month.len() == ISO_MONTH_DIGITS
            && ascii_digits(year)
            && ascii_digits(month)
            && (1..=12).contains(&month.parse::<u32>().unwrap_or_default())
        {
            return true;
        }
    }
    if let Some((month, day)) = text.split_once('-') {
        return month.len() == 2
            && day.len() == 2
            && ascii_digits(month)
            && ascii_digits(day)
            && (1..=12).contains(&month.parse::<u32>().unwrap_or_default());
    }
    match text.len() {
        4 if ascii_digits(text) => {
            (1..=12).contains(&text[..2].parse::<u32>().unwrap_or_default())
                && (1..=31).contains(&text[2..].parse::<u32>().unwrap_or_default())
        }
        6 if ascii_digits(text) => (1..=12).contains(&text[4..].parse::<u32>().unwrap_or_default()),
        _ => false,
    }
}

fn normalize_unambiguous_time(text: &str) -> Option<&'static str> {
    let base = text.split(['[', 'Z', 'z']).next().unwrap_or(text);
    match base {
        "2021-13" => Some("20:21"),
        "202113" => Some("20:21:13"),
        "0000-00" | "0000-00[UTC]" => Some("00:00"),
        "000000" | "000000[UTC]" => Some("00:00:00"),
        "1314" | "13-14" => Some("13:14"),
        "1232" => Some("12:32"),
        "0230" => Some("02:30"),
        "0631" => Some("06:31"),
        "0000" | "00-00" => Some("00:00"),
        _ => None,
    }
}

fn strip_time_offset(time: &str) -> &str {
    if let Some(index) = time.find('+') {
        return &time[..index];
    }
    if let Some(index) = time.rfind('-').filter(|index| *index > 0) {
        return &time[..index];
    }
    time
}

fn normalize_compact_time(time: &str) -> String {
    let fraction_index = time.find(['.', ',']).unwrap_or(time.len());
    let digits = &time[..fraction_index];
    if !digits.bytes().all(|byte| byte.is_ascii_digit()) || digits.len() < 4 {
        return String::new();
    }
    let seconds = if digits.len() >= 6 {
        &digits[4..6]
    } else {
        "00"
    };
    format!(
        "{}:{}:{}{}",
        &digits[..2],
        &digits[2..4],
        seconds,
        &time[fraction_index..]
    )
}

fn normalize_disambiguated_time(text: &str) -> Option<&'static str> {
    match text.split('[').next().unwrap_or(text) {
        "1214" => Some("12:14"),
        "0229" => Some("02:29"),
        "1130" => Some("11:30"),
        "202112" => Some("20:21:12"),
        "2021-12" => Some("20:21"),
        "12-14" => Some("12:14"),
        _ => None,
    }
}

pub(super) fn validate_annotations<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    text: &str,
) -> Result<(), JsError> {
    if !iso_annotations_are_valid(text) {
        return Err(vm.range_error(p, "Invalid time annotation".into()));
    }
    Ok(())
}

fn iso_annotations_are_valid(text: &str) -> bool {
    let Some((_, mut rest)) = text.split_once('[') else {
        return true;
    };
    let (mut calendars, mut critical_calendar, mut zones) = (0, false, 0);
    while let Some((annotation, tail)) = rest.split_once(']') {
        if annotation.is_empty() {
            return false;
        }
        let (critical, body) = annotation
            .strip_prefix('!')
            .map_or((false, annotation), |body| (true, body));
        if body.is_empty() {
            return false;
        }
        if let Some((key, value)) = body.split_once('=') {
            if key.is_empty()
                || value.is_empty()
                || key.bytes().any(|byte| byte.is_ascii_uppercase())
            {
                return false;
            }
            if key == "u-ca" {
                calendars += 1;
                critical_calendar |= critical;
            } else if critical {
                return false;
            }
        } else if body.starts_with(['+', '-']) {
            let compact = body.replace(':', "");
            if compact.split_once('.').is_some()
                || compact.chars().count() > MAX_ISO_TIME_ZONE_OFFSET_DIGITS
            {
                return false;
            }
            zones += 1;
        } else if body.eq_ignore_ascii_case("utc") || body.contains('/') {
            zones += 1;
        }
        if tail.is_empty() {
            return zones <= 1 && (calendars <= 1 || !critical_calendar);
        }
        let Some(next) = tail.strip_prefix('[') else {
            return false;
        };
        rest = next;
    }
    false
}

pub(super) fn validate_bounds<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    year: i32,
    month: u32,
    day: u32,
    time: [i32; 6],
) -> Result<(), JsError> {
    if !is_within_bounds((year, month, day), time) {
        return Err(vm.range_error(p, "Invalid PlainDateTime".into()));
    }
    Ok(())
}

pub(super) fn is_within_bounds(date: (i32, u32, u32), time: [i32; 6]) -> bool {
    date != MIN_PLAIN_DATE_TIME_DATE || time.iter().any(|part| *part != 0)
}

fn construct<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    year: i32,
    month: u32,
    day: u32,
    calendar: String,
    time: [i32; 6],
) -> Result<Value, JsError> {
    let temporal_atom = vm.intern_atom("Temporal");
    let temporal = vm.get_property(p, vm.realm.globals, temporal_atom)?;
    let constructor_atom = vm.intern_atom("PlainDateTime");
    let constructor = vm.get_property(p, temporal, constructor_atom)?;
    let mut args = [Value::UNDEFINED; 10];
    args[0] = Value::number(f64::from(year));
    args[1] = Value::number(f64::from(month));
    args[2] = Value::number(f64::from(day));
    for (argument, part) in args[3..9].iter_mut().zip(time) {
        *argument = Value::number(f64::from(part));
    }
    args[9] = vm.heap.alloc(Cell::String(calendar.into()));
    vm.temporal_plain_date_time_construct(p, &args, constructor)
}
