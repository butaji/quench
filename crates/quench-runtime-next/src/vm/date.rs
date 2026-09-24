use super::*;
use crate::host::{CapabilityId, HostContext};
use chrono::{
    DateTime, Datelike, Duration, Local, LocalResult, NaiveDate, Offset, TimeZone, Timelike, Utc,
};

const DATE_PROTOTYPE_METHODS: &[(&str, Native)] = &[
    ("toString", Native::DateToString),
    ("valueOf", Native::DateValueOf),
    ("getTime", Native::DateGetTime),
    ("getFullYear", Native::DateGetFullYear),
    ("getMonth", Native::DateGetMonth),
    ("getDate", Native::DateGetDate),
    ("getDay", Native::DateGetDay),
    ("getHours", Native::DateGetHours),
    ("getMinutes", Native::DateGetMinutes),
    ("getSeconds", Native::DateGetSeconds),
    ("getMilliseconds", Native::DateGetMilliseconds),
    ("getTimezoneOffset", Native::DateGetTimezoneOffset),
    ("getUTCFullYear", Native::DateGetUTCFullYear),
    ("getUTCMonth", Native::DateGetUTCMonth),
    ("getUTCDate", Native::DateGetUTCDate),
    ("getUTCDay", Native::DateGetUTCDay),
    ("getUTCHours", Native::DateGetUTCHours),
    ("getUTCMinutes", Native::DateGetUTCMinutes),
    ("getUTCSeconds", Native::DateGetUTCSeconds),
    ("getUTCMilliseconds", Native::DateGetUTCMilliseconds),
    ("getYear", Native::DateGetYear),
    ("setTime", Native::DateSetTime),
    ("setFullYear", Native::DateSetFullYear),
    ("setMonth", Native::DateSetMonth),
    ("setUTCMonth", Native::DateSetUTCMonth),
    ("setDate", Native::DateSetDate),
    ("setUTCDate", Native::DateSetUTCDate),
    ("setUTCFullYear", Native::DateSetUTCFullYear),
    ("setHours", Native::DateSetHours),
    ("setMinutes", Native::DateSetMinutes),
    ("setSeconds", Native::DateSetSeconds),
    ("setMilliseconds", Native::DateSetMilliseconds),
    ("setUTCHours", Native::DateSetUTCHours),
    ("setUTCMinutes", Native::DateSetUTCMinutes),
    ("setUTCSeconds", Native::DateSetUTCSeconds),
    ("setUTCMilliseconds", Native::DateSetUTCMilliseconds),
    ("setYear", Native::DateSetYear),
    ("toLocaleString", Native::DateToLocaleString),
    ("toUTCString", Native::DateToUTCString),
    ("toISOString", Native::DateToISOString),
    ("toJSON", Native::DateToJSON),
];
const SECONDS_PER_MINUTE: i32 = 60;
const LEGACY_DATE_YEAR_OFFSET: i32 = 1900;
const MONTHS_PER_YEAR: f64 = 12.0;
const MILLISECONDS_PER_SECOND: f64 = 1_000.0;
const MILLISECONDS_PER_MINUTE: f64 = 60_000.0;
const MILLISECONDS_PER_HOUR: f64 = 3_600_000.0;
const DATE_TIME_CLIP_LIMIT_MS: f64 = 8.64e15;
const INT32_MODULUS: f64 = 4_294_967_296.0;
const INT32_SIGN_BOUNDARY: f64 = 2_147_483_648.0;
const LEGACY_DATE_YEAR_MIN: f64 = 0.0;
const LEGACY_DATE_YEAR_MAX: f64 = 99.0;
const LEGACY_DATE_YEAR_MAX_INT: i32 = 99;
const DATE_COMPONENT_COUNT: usize = 7;
const YEAR_COMPONENT: usize = 0;
const MONTH_COMPONENT: usize = 1;
const DAY_COMPONENT: usize = 2;
const HOUR_COMPONENT: usize = 3;
const MINUTE_COMPONENT: usize = 4;
const SECOND_COMPONENT: usize = 5;
const MILLISECOND_COMPONENT: usize = 6;
const REQUIRED_DATE_SETTER_ARGUMENTS: usize = 1;
const EPOCH_MILLISECONDS: f64 = 0.0;
const DATE_CONSTRUCTOR_MULTI_ARGUMENT_THRESHOLD: usize = 2;
const DATE_SETTER_YEAR_ARGUMENT_LIMIT: usize = 3;
const DATE_SETTER_MONTH_ARGUMENT_LIMIT: usize = 2;
const DATE_SETTER_DAY_ARGUMENT_LIMIT: usize = 1;
const DATE_SETTER_HOUR_ARGUMENT_LIMIT: usize = 4;
const DATE_SETTER_MINUTE_ARGUMENT_LIMIT: usize = 3;
const DATE_SETTER_SECOND_ARGUMENT_LIMIT: usize = 2;
const DATE_SETTER_MILLISECOND_ARGUMENT_LIMIT: usize = 1;
const FIRST_DAY_OF_MONTH: f64 = 1.0;
const DATE_DAY_FIELD_WIDTH: usize = 2;
const DATE_TIME_FIELD_WIDTH: usize = 2;
const DATE_YEAR_MINIMUM_WIDTH: usize = 4;
const DATE_OFFSET_FIELD_WIDTH: usize = 2;
const DATE_MINUTES_PER_HOUR: u32 = 60;
const DATE_WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const DATE_MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const DEFAULT_DATE_COMPONENTS: [f64; DATE_COMPONENT_COUNT] =
    [0.0, 0.0, FIRST_DAY_OF_MONTH, 0.0, 0.0, 0.0, 0.0];

pub(super) fn date_native_length(native: Native) -> Option<f64> {
    match native {
        Native::Date | Native::DateUTC => Some(DATE_COMPONENT_COUNT as f64),
        Native::DateParse | Native::DateSetTime => Some(1.0),
        Native::DateNow => Some(0.0),
        _ => DateSetter::from_native(native)
            .map(|setter| setter.maximum_arguments as f64)
            .or_else(|| {
                DATE_PROTOTYPE_METHODS
                    .iter()
                    .find_map(|(_, method)| (*method == native).then_some(native))
                    .map(|method| {
                        if method == Native::DateToJSON {
                            1.0
                        } else {
                            0.0
                        }
                    })
            }),
    }
}

struct DateParts {
    year: i32,
    month: u32,
    day: u32,
    weekday: u32,
    hour: u32,
    minute: u32,
    second: u32,
    millisecond: u32,
}

impl DateParts {
    fn as_components(&self) -> [f64; DATE_COMPONENT_COUNT] {
        [
            f64::from(self.year),
            f64::from(self.month),
            f64::from(self.day),
            f64::from(self.hour),
            f64::from(self.minute),
            f64::from(self.second),
            f64::from(self.millisecond),
        ]
    }
}

impl<H: Host> Vm<H> {
    pub(super) fn install_date(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let date = self.native_value(Native::Date);
        let prototype = self.object();
        self.set_named(program, date, "prototype", prototype)?;
        for (name, native) in DATE_PROTOTYPE_METHODS {
            self.set_builtin_named(program, prototype, name, *native)?;
        }
        for (name, native) in [
            ("now", Native::DateNow),
            ("parse", Native::DateParse),
            ("UTC", Native::DateUTC),
        ] {
            self.set_builtin_named(program, date, name, native)?;
        }
        self.global(program, "Date", date)
    }

    pub(super) fn date_static_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::DateParse => {
                let text = self.to_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let millis = chrono::DateTime::parse_from_rfc3339(&text)
                    .ok()
                    .map(|date| date.timestamp_millis() as f64)
                    .unwrap_or(f64::NAN);
                Ok(Value::number(millis))
            }
            Native::DateUTC => Ok(Value::number(date_utc_constructor_value(self, p, args)?)),
            _ => Err(JsError("invalid static Date native".into())),
        }
    }

    pub(super) fn date_property_native(&self, atom: Atom) -> Value {
        DATE_PROTOTYPE_METHODS
            .iter()
            .find_map(|(name, native)| {
                (self.lookup_atom(name) == Some(atom)).then(|| self.native_value(*native))
            })
            .unwrap_or(Value::UNDEFINED)
    }

    pub(super) fn date_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let Some(Cell::Date { milliseconds, .. }) = self.heap.get(this) else {
            return Err(JsError(
                "Date method called on incompatible receiver".into(),
            ));
        };
        let milliseconds = *milliseconds;
        if let Some(value) = self.date_setter(p, native, this, milliseconds, args)? {
            return Ok(Value::number(value));
        }
        if let Some(value) = date_getter(native, milliseconds) {
            return Ok(Value::number(value));
        }
        match native {
            Native::DateToString if !milliseconds.is_finite() => {
                Ok(self.heap.alloc(Cell::String("Invalid Date".into())))
            }
            Native::DateToUTCString if !milliseconds.is_finite() => {
                Ok(self.heap.alloc(Cell::String("Invalid Date".into())))
            }
            Native::DateToUTCString => {
                let Some(date) = date_utc(milliseconds) else {
                    return Err(JsError("Invalid time value".into()));
                };
                let text = date.format("%a, %d %b %Y %H:%M:%S GMT").to_string();
                Ok(self.heap.alloc(Cell::String(text.into())))
            }
            Native::DateToLocaleString => {
                let Some(date) = date_local(milliseconds) else {
                    return Ok(self.heap.alloc(Cell::String("Invalid Date".into())));
                };
                let text = format!(
                    "{} {}, {} {:02}:{:02}:{:02}",
                    date.month(),
                    date.day(),
                    date.year(),
                    date.hour(),
                    date.minute(),
                    date.second(),
                );
                Ok(self.heap.alloc(Cell::String(text.into())))
            }
            Native::DateToString => Ok(self
                .heap
                .alloc(Cell::String(format_date_string(milliseconds).into()))),
            Native::DateToISOString | Native::DateToJSON => {
                if !milliseconds.is_finite() {
                    return Err(JsError("Invalid time value".into()));
                }
                let millis = milliseconds.trunc();
                let date = Utc
                    .timestamp_millis_opt(millis as i64)
                    .single()
                    .ok_or_else(|| JsError("Invalid time value".into()))?;
                let text = format_date_iso(date);
                Ok(self.heap.alloc(Cell::String(text.into())))
            }
            _ => Err(JsError("invalid Date native".into())),
        }
    }

    fn date_setter(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        receiver: Value,
        current: f64,
        args: &[Value],
    ) -> Result<Option<f64>, JsError> {
        if native == Native::DateSetTime {
            let input = self.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
            let value = time_clip(input);
            self.store_date_time(receiver, value);
            return Ok(Some(value));
        }
        let Some(setter) = DateSetter::from_native(native) else {
            return Ok(None);
        };
        let values = args
            .iter()
            .copied()
            .take(setter.maximum_arguments)
            .map(|value| self.to_number(p, value))
            .collect::<Result<Vec<_>, _>>()?;
        if current.is_nan() && !setter.recovers_invalid_date {
            self.store_date_time(receiver, f64::NAN);
            return Ok(Some(f64::NAN));
        }
        let base_time = if current.is_nan() {
            EPOCH_MILLISECONDS
        } else {
            current
        };
        let parts = if setter.utc {
            date_parts(base_time, Utc)
        } else {
            date_parts(base_time, Local)
        };
        let Some(parts) = parts else {
            self.store_date_time(receiver, f64::NAN);
            return Ok(Some(f64::NAN));
        };
        let mut components = parts.as_components();
        for (index, value) in values.iter().copied().enumerate() {
            let destination = setter.start_component + index;
            if let Some(component) = components.get_mut(destination) {
                *component = value;
            }
        }
        if values.len() < setter.required_arguments {
            components[setter.start_component] = f64::NAN;
        }
        if setter.legacy_year && !components[YEAR_COMPONENT].is_finite() {
            self.store_date_time(receiver, f64::NAN);
            return Ok(Some(f64::NAN));
        }
        if setter.legacy_year {
            let year = to_int32(components[YEAR_COMPONENT]);
            components[YEAR_COMPONENT] = if (0..=LEGACY_DATE_YEAR_MAX_INT).contains(&year) {
                f64::from(year + LEGACY_DATE_YEAR_OFFSET)
            } else {
                f64::from(year)
            };
        }
        let value = make_date_milliseconds(components, setter.utc);
        self.store_date_time(receiver, value);
        Ok(Some(value))
    }

    fn store_date_time(&mut self, receiver: Value, milliseconds: f64) {
        if let Some(Cell::Date {
            milliseconds: stored,
            ..
        }) = self.heap.get_mut(receiver)
        {
            *stored = milliseconds;
        }
    }

    pub(super) fn date_to_json_string(&self, milliseconds: f64) -> Option<String> {
        if !milliseconds.is_finite() {
            return None;
        }
        let date = Utc
            .timestamp_millis_opt(milliseconds.trunc() as i64)
            .single()?;
        Some(format_date_iso(date))
    }

    pub(super) fn date_construct_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let milliseconds = if args.len() < DATE_CONSTRUCTOR_MULTI_ARGUMENT_THRESHOLD {
            match args.first().copied() {
                None => HostContext::new(&mut self.host).invoke(CapabilityId::ClockMillis, None),
                Some(value) => self.to_number(p, value)?,
            }
        } else {
            let mut parts = DEFAULT_DATE_COMPONENTS;
            for (index, value) in args.iter().take(DATE_COMPONENT_COUNT).enumerate() {
                parts[index] = self.to_number(p, *value)?;
            }
            if (LEGACY_DATE_YEAR_MIN..=LEGACY_DATE_YEAR_MAX).contains(&parts[YEAR_COMPONENT]) {
                parts[YEAR_COMPONENT] += f64::from(LEGACY_DATE_YEAR_OFFSET);
            }
            make_date_milliseconds(parts, false)
        };
        let prototype_atom = self.intern_atom("prototype");
        let prototype = self
            .own_property(self.native_value(Native::Date), prototype_atom)
            .unwrap_or(self.object_proto);
        Ok(self.heap.alloc(Cell::Date {
            milliseconds,
            object: Box::new(Self::empty_object(prototype)),
        }))
    }
}

fn date_utc_constructor_value<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    args: &[Value],
) -> Result<f64, JsError> {
    if args.is_empty() {
        return Ok(f64::NAN);
    }
    let mut components = DEFAULT_DATE_COMPONENTS;
    for (index, value) in args.iter().copied().take(DATE_COMPONENT_COUNT).enumerate() {
        components[index] = vm.to_number(p, value)?;
    }
    if (LEGACY_DATE_YEAR_MIN..=LEGACY_DATE_YEAR_MAX).contains(&components[YEAR_COMPONENT]) {
        components[YEAR_COMPONENT] += f64::from(LEGACY_DATE_YEAR_OFFSET);
    }
    Ok(make_date_milliseconds(components, true))
}

fn date_getter(native: Native, milliseconds: f64) -> Option<f64> {
    if native == Native::DateGetTime || native == Native::DateValueOf {
        return Some(milliseconds);
    }
    if native == Native::DateGetTimezoneOffset {
        let offset = milliseconds
            .is_finite()
            .then(|| {
                Local
                    .timestamp_millis_opt(milliseconds.trunc() as i64)
                    .single()
            })
            .flatten()
            .map(|date| -date.offset().local_minus_utc() / SECONDS_PER_MINUTE);
        return Some(offset.map_or(f64::NAN, f64::from));
    }
    let utc = matches!(
        native,
        Native::DateGetUTCFullYear
            | Native::DateGetUTCMonth
            | Native::DateGetUTCDate
            | Native::DateGetUTCDay
            | Native::DateGetUTCHours
            | Native::DateGetUTCMinutes
            | Native::DateGetUTCSeconds
            | Native::DateGetUTCMilliseconds
    );
    let parts = if utc {
        date_parts(milliseconds, Utc)
    } else {
        date_parts(milliseconds, Local)
    };
    let Some(parts) = parts else {
        return Some(f64::NAN);
    };
    Some(match native {
        Native::DateGetFullYear | Native::DateGetUTCFullYear => f64::from(parts.year),
        Native::DateGetMonth | Native::DateGetUTCMonth => f64::from(parts.month),
        Native::DateGetDate | Native::DateGetUTCDate => f64::from(parts.day),
        Native::DateGetDay | Native::DateGetUTCDay => f64::from(parts.weekday),
        Native::DateGetHours | Native::DateGetUTCHours => f64::from(parts.hour),
        Native::DateGetMinutes | Native::DateGetUTCMinutes => f64::from(parts.minute),
        Native::DateGetSeconds | Native::DateGetUTCSeconds => f64::from(parts.second),
        Native::DateGetMilliseconds | Native::DateGetUTCMilliseconds => {
            f64::from(parts.millisecond)
        }
        Native::DateGetYear => f64::from(parts.year - LEGACY_DATE_YEAR_OFFSET),
        _ => return None,
    })
}

fn date_parts<Tz: TimeZone>(milliseconds: f64, timezone: Tz) -> Option<DateParts> {
    if !milliseconds.is_finite() {
        return None;
    }
    let date = timezone
        .timestamp_millis_opt(milliseconds.trunc() as i64)
        .single()?;
    Some(DateParts {
        year: date.year(),
        month: date.month0(),
        day: date.day(),
        weekday: date.weekday().num_days_from_sunday(),
        hour: date.hour(),
        minute: date.minute(),
        second: date.second(),
        millisecond: date.timestamp_subsec_millis(),
    })
}

fn date_utc(milliseconds: f64) -> Option<DateTime<Utc>> {
    milliseconds
        .is_finite()
        .then(|| {
            Utc.timestamp_millis_opt(milliseconds.trunc() as i64)
                .single()
        })
        .flatten()
}

fn date_local(milliseconds: f64) -> Option<DateTime<Local>> {
    milliseconds
        .is_finite()
        .then(|| {
            Local
                .timestamp_millis_opt(milliseconds.trunc() as i64)
                .single()
        })
        .flatten()
}

struct DateSetter {
    utc: bool,
    start_component: usize,
    required_arguments: usize,
    maximum_arguments: usize,
    recovers_invalid_date: bool,
    legacy_year: bool,
}

impl DateSetter {
    fn from_native(native: Native) -> Option<Self> {
        let (utc, start_component, maximum_arguments, recovers_invalid_date, legacy_year) =
            match native {
                Native::DateSetFullYear => (
                    false,
                    YEAR_COMPONENT,
                    DATE_SETTER_YEAR_ARGUMENT_LIMIT,
                    true,
                    false,
                ),
                Native::DateSetUTCFullYear => (
                    true,
                    YEAR_COMPONENT,
                    DATE_SETTER_YEAR_ARGUMENT_LIMIT,
                    true,
                    false,
                ),
                Native::DateSetYear => (
                    false,
                    YEAR_COMPONENT,
                    DATE_SETTER_DAY_ARGUMENT_LIMIT,
                    true,
                    true,
                ),
                Native::DateSetMonth => (
                    false,
                    MONTH_COMPONENT,
                    DATE_SETTER_MONTH_ARGUMENT_LIMIT,
                    false,
                    false,
                ),
                Native::DateSetUTCMonth => (
                    true,
                    MONTH_COMPONENT,
                    DATE_SETTER_MONTH_ARGUMENT_LIMIT,
                    false,
                    false,
                ),
                Native::DateSetDate => (
                    false,
                    DAY_COMPONENT,
                    DATE_SETTER_DAY_ARGUMENT_LIMIT,
                    false,
                    false,
                ),
                Native::DateSetUTCDate => (
                    true,
                    DAY_COMPONENT,
                    DATE_SETTER_DAY_ARGUMENT_LIMIT,
                    false,
                    false,
                ),
                Native::DateSetHours => (
                    false,
                    HOUR_COMPONENT,
                    DATE_SETTER_HOUR_ARGUMENT_LIMIT,
                    false,
                    false,
                ),
                Native::DateSetUTCHours => (
                    true,
                    HOUR_COMPONENT,
                    DATE_SETTER_HOUR_ARGUMENT_LIMIT,
                    false,
                    false,
                ),
                Native::DateSetMinutes => (
                    false,
                    MINUTE_COMPONENT,
                    DATE_SETTER_MINUTE_ARGUMENT_LIMIT,
                    false,
                    false,
                ),
                Native::DateSetUTCMinutes => (
                    true,
                    MINUTE_COMPONENT,
                    DATE_SETTER_MINUTE_ARGUMENT_LIMIT,
                    false,
                    false,
                ),
                Native::DateSetSeconds => (
                    false,
                    SECOND_COMPONENT,
                    DATE_SETTER_SECOND_ARGUMENT_LIMIT,
                    false,
                    false,
                ),
                Native::DateSetUTCSeconds => (
                    true,
                    SECOND_COMPONENT,
                    DATE_SETTER_SECOND_ARGUMENT_LIMIT,
                    false,
                    false,
                ),
                Native::DateSetMilliseconds => (
                    false,
                    MILLISECOND_COMPONENT,
                    DATE_SETTER_MILLISECOND_ARGUMENT_LIMIT,
                    false,
                    false,
                ),
                Native::DateSetUTCMilliseconds => (
                    true,
                    MILLISECOND_COMPONENT,
                    DATE_SETTER_MILLISECOND_ARGUMENT_LIMIT,
                    false,
                    false,
                ),
                _ => return None,
            };
        Some(Self {
            utc,
            start_component,
            required_arguments: REQUIRED_DATE_SETTER_ARGUMENTS,
            maximum_arguments,
            recovers_invalid_date,
            legacy_year,
        })
    }
}

fn make_date_milliseconds(components: [f64; DATE_COMPONENT_COUNT], utc: bool) -> f64 {
    if components.iter().any(|component| !component.is_finite()) {
        return f64::NAN;
    }
    let [year, month, day, hour, minute, second, millisecond] = components;
    let normalized_year = year.trunc() + (month.trunc() / MONTHS_PER_YEAR).floor();
    if normalized_year < f64::from(i32::MIN) || normalized_year > f64::from(i32::MAX) {
        return f64::NAN;
    }
    let year = normalized_year as i32;
    let month = month.trunc().rem_euclid(MONTHS_PER_YEAR) as u32 + 1;
    let Some(date) = NaiveDate::from_ymd_opt(year, month, 1) else {
        return f64::NAN;
    };
    let Some(day_offset) = finite_i64(day.trunc() - 1.0) else {
        return f64::NAN;
    };
    let time_ms = hour * MILLISECONDS_PER_HOUR
        + minute * MILLISECONDS_PER_MINUTE
        + second * MILLISECONDS_PER_SECOND
        + millisecond;
    let Some(time_ms) = finite_i64(time_ms.trunc()) else {
        return f64::NAN;
    };
    let Some(date_time) = date
        .and_hms_opt(0, 0, 0)
        .and_then(|midnight| midnight.checked_add_signed(Duration::days(day_offset)))
        .and_then(|day| day.checked_add_signed(Duration::milliseconds(time_ms)))
    else {
        return f64::NAN;
    };
    let milliseconds = if utc {
        date_time.and_utc().timestamp_millis() as f64
    } else {
        match Local.from_local_datetime(&date_time) {
            LocalResult::Single(date) => date.timestamp_millis() as f64,
            LocalResult::Ambiguous(first, second) => {
                first.timestamp_millis().min(second.timestamp_millis()) as f64
            }
            LocalResult::None => return f64::NAN,
        }
    };
    time_clip(milliseconds)
}

fn finite_i64(value: f64) -> Option<i64> {
    (value.is_finite() && value.abs() <= DATE_TIME_CLIP_LIMIT_MS).then_some(value as i64)
}

fn time_clip(value: f64) -> f64 {
    if value.is_finite() && value.abs() <= DATE_TIME_CLIP_LIMIT_MS {
        value.trunc()
    } else {
        f64::NAN
    }
}

fn to_int32(value: f64) -> i32 {
    if !value.is_finite() || value == 0.0 {
        return 0;
    }
    let modulo = value.trunc().rem_euclid(INT32_MODULUS);
    let signed = if modulo >= INT32_SIGN_BOUNDARY {
        modulo - INT32_MODULUS
    } else {
        modulo
    };
    signed as i32
}

fn format_date_iso(date: DateTime<Utc>) -> String {
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        date.year(),
        date.month(),
        date.day(),
        date.hour(),
        date.minute(),
        date.second(),
        date.timestamp_subsec_millis(),
    )
}

pub(super) fn format_date_string(milliseconds: f64) -> String {
    let Some(date) = date_local(milliseconds) else {
        return "Invalid Date".into();
    };
    let offset = date.offset().fix().local_minus_utc() / SECONDS_PER_MINUTE;
    let offset_sign = if offset < 0 { '-' } else { '+' };
    format!(
        "{} {} {:0day_width$} {} {:0time_width$}:{:0time_width$}:{:0time_width$} GMT{}{:0offset_width$}{:0offset_width$}",
        DATE_WEEKDAYS[date.weekday().num_days_from_sunday() as usize],
        DATE_MONTHS[date.month0() as usize],
        date.day(),
        display_date_year(date.year()),
        date.hour(),
        date.minute(),
        date.second(),
        offset_sign,
        offset.unsigned_abs() / DATE_MINUTES_PER_HOUR,
        offset.unsigned_abs() % DATE_MINUTES_PER_HOUR,
        day_width = DATE_DAY_FIELD_WIDTH,
        time_width = DATE_TIME_FIELD_WIDTH,
        offset_width = DATE_OFFSET_FIELD_WIDTH,
    )
}

fn display_date_year(year: i32) -> String {
    if year < 0 {
        format!(
            "-{:0width$}",
            year.unsigned_abs(),
            width = DATE_YEAR_MINIMUM_WIDTH
        )
    } else {
        format!("{:0width$}", year, width = DATE_YEAR_MINIMUM_WIDTH)
    }
}
