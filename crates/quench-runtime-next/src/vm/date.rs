use super::*;
use crate::host::{CapabilityId, HostContext};
use chrono::{
    DateTime, Datelike, Duration, FixedOffset, Local, LocalResult, NaiveDate, Offset, TimeZone,
    Timelike, Utc,
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
    ("toDateString", Native::DateToDateString),
    ("toTimeString", Native::DateToTimeString),
    ("toLocaleDateString", Native::DateToLocaleDateString),
    ("toLocaleTimeString", Native::DateToLocaleTimeString),
    ("toUTCString", Native::DateToUTCString),
    ("toISOString", Native::DateToISOString),
    ("toJSON", Native::DateToJSON),
    ("toTemporalInstant", Native::DateToTemporalInstant),
];
const SECONDS_PER_MINUTE: i32 = 60;
const LEGACY_DATE_YEAR_OFFSET: i32 = 1900;
const MONTHS_PER_YEAR: f64 = 12.0;
const MILLISECONDS_PER_SECOND: f64 = 1_000.0;
const MILLISECONDS_PER_MINUTE: f64 = 60_000.0;
const MILLISECONDS_PER_HOUR: f64 = 3_600_000.0;
const HOURS_PER_DAY: f64 = 24.0;
const MILLISECONDS_PER_DAY: f64 = MILLISECONDS_PER_HOUR * HOURS_PER_DAY;
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
const DATE_MILLISECOND_DIGITS: usize = 3;
const DATE_OFFSET_FIELD_WIDTH: usize = 2;
const DATE_MINUTES_PER_HOUR: u32 = 60;
const DATE_MILLISECONDS_PER_DAY: i64 = 86_400_000;
const DATE_MILLISECONDS_PER_HOUR: i64 = 3_600_000;
const DATE_MILLISECONDS_PER_MINUTE: i64 = 60_000;
const DATE_MILLISECONDS_PER_SECOND: i64 = 1_000;
const DATE_DAYS_PER_CYCLE: i64 = 146_097;
const DATE_YEARS_PER_CYCLE: i64 = 400;
const DATE_QUADRENNIAL_DAYS: i64 = 1_460;
const DATE_CENTURY_DAYS: i64 = 36_524;
const DATE_DAYS_PER_YEAR: i64 = 365;
const DATE_MONTHS_PER_CYCLE_SEGMENT: i64 = 153;
const DATE_EPOCH_DAY_OFFSET: i64 = 719_468;
const ISO_YEAR_MAX: i64 = 9_999;
const ISO_EXTENDED_YEAR_WIDTH: usize = 6;
const DATE_WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const DATE_MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const DEFAULT_DATE_COMPONENTS: [f64; DATE_COMPONENT_COUNT] =
    [0.0, 0.0, FIRST_DAY_OF_MONTH, 0.0, 0.0, 0.0, 0.0];

pub(super) fn date_native_length(native: Native) -> Option<f64> {
    match native {
        Native::Date | Native::DateUTC => Some(DATE_COMPONENT_COUNT as f64),
        Native::DateParse | Native::DateSetTime | Native::DateToPrimitive => Some(1.0),
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
        let prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.object_proto)));
        self.install_date_for_realm(program, self.realm.globals, date, prototype)
    }

    pub(super) fn install_date_for_realm(
        &mut self,
        program: &ResidualProgram,
        global: Value,
        constructor: Value,
        prototype: Value,
    ) -> Result<(), JsError> {
        self.set_builtin_function_name(constructor, "Date")?;
        self.set_builtin_value_named(constructor, "prototype", prototype)?;
        let prototype_atom = self.intern_atom("prototype");
        self.set_property_attributes(
            constructor,
            PropertyKey::string(prototype_atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: false,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        self.set_builtin_value_named(prototype, "constructor", constructor)?;
        for (name, native) in DATE_PROTOTYPE_METHODS {
            let method = self.native_with_realm(*native, global, global);
            self.set_builtin_function_name(method, name)?;
            self.set_builtin_value_named(prototype, name, method)?;
        }
        for (name, native) in [
            ("now", Native::DateNow),
            ("parse", Native::DateParse),
            ("UTC", Native::DateUTC),
        ] {
            let method = self.native_with_realm(native, global, global);
            self.set_builtin_function_name(method, name)?;
            self.set_builtin_value_named(constructor, name, method)?;
        }
        if let Some(symbol) = self.well_known_symbols.get("toPrimitive").copied() {
            let method = self.native_with_realm(Native::DateToPrimitive, global, global);
            self.set_builtin_function_name(method, "[Symbol.toPrimitive]")?;
            self.set_symbol_property(prototype, symbol, method)?;
            self.set_property_attributes(
                prototype,
                PropertyKey::symbol(symbol),
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: true,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
        }
        if self.well_known_symbols.contains_key("toStringTag") {
            self.install_builtin_to_string_tag(prototype, "Date")?;
        }
        if global == self.realm.globals {
            self.global(program, "Date", constructor)
        } else {
            let atom = self.intern_atom("Date");
            self.set_property(global, atom, constructor)?;
            self.set_property_attributes(
                global,
                PropertyKey::string(atom),
                PropertyAttributes {
                    writable: true,
                    enumerable: false,
                    configurable: true,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
            Ok(())
        }
    }

    pub(super) fn date_call(&mut self) -> Result<Value, JsError> {
        let milliseconds = HostContext::new(&mut self.host).invoke(CapabilityId::ClockMillis, None);
        Ok(self
            .heap
            .alloc(Cell::String(format_date_string(milliseconds).into())))
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
                let millis = parse_date_string(&text);
                Ok(Value::number(millis))
            }
            Native::DateUTC => Ok(Value::number(date_utc_constructor_value(self, p, args)?)),
            _ => Err(JsError("invalid static Date native".into())),
        }
    }

    pub(super) fn date_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if native == Native::DateToJSON {
            return self.date_to_json(p, this);
        }
        if native == Native::DateToPrimitive {
            return self.date_to_primitive(p, this, args);
        }
        let Some(Cell::Date { milliseconds, .. }) = self.heap.get(this) else {
            return Err(self.type_error(p, "Date method called on incompatible receiver".into()));
        };
        let milliseconds = *milliseconds;
        if let Some(value) = self.date_setter(p, native, this, args)? {
            return Ok(Value::number(value));
        }
        if let Some(value) = date_getter(native, milliseconds) {
            return Ok(Value::number(value));
        }
        match native {
            Native::DateToString if !milliseconds.is_finite() => {
                Ok(self.heap.alloc(Cell::String("Invalid Date".into())))
            }
            Native::DateToDateString => Ok(self
                .heap
                .alloc(Cell::String(format_date_date_string(milliseconds).into()))),
            Native::DateToTimeString => Ok(self
                .heap
                .alloc(Cell::String(format_date_time_string(milliseconds).into()))),
            Native::DateToLocaleDateString | Native::DateToLocaleTimeString => {
                if !milliseconds.is_finite() {
                    return Ok(self.heap.alloc(Cell::String("Invalid Date".into())));
                }
                let Some(date) = date_local(milliseconds) else {
                    return Ok(self.heap.alloc(Cell::String("Invalid Date".into())));
                };
                let text = if native == Native::DateToLocaleDateString {
                    format!("{} {}, {}", date.month(), date.day(), date.year())
                } else {
                    format!(
                        "{:02}:{:02}:{:02}",
                        date.hour(),
                        date.minute(),
                        date.second()
                    )
                };
                Ok(self.heap.alloc(Cell::String(text.into())))
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
                    return Err(self.range_error(p, "Invalid time value".into()));
                }
                let text = format_date_iso(milliseconds)
                    .ok_or_else(|| self.range_error(p, "Invalid time value".into()))?;
                Ok(self.heap.alloc(Cell::String(text.into())))
            }
            Native::DateToTemporalInstant => {
                if !milliseconds.is_finite() {
                    return Err(self.range_error(p, "Invalid time value".into()));
                }
                let nanoseconds = format!("{:.0}", milliseconds.trunc() * 1_000_000.0);
                let value = self.heap.alloc(Cell::BigInt(nanoseconds.into()));
                let object = self
                    .heap
                    .alloc(Cell::Object(Self::empty_object(self.object_proto)));
                let key = self.intern_atom("epochNanoseconds");
                self.set_property(object, key, value)?;
                Ok(object)
            }
            _ => Err(JsError("invalid Date native".into())),
        }
    }

    fn date_to_json(&mut self, p: &ResidualProgram, receiver: Value) -> Result<Value, JsError> {
        if receiver.is_null() || receiver.is_undefined() {
            return Err(self.type_error(p, "Cannot convert undefined or null to object".into()));
        }
        let primitive = self.to_primitive(p, receiver, "number")?;
        if matches!(primitive.as_number(), Some(number) if !number.is_finite()) {
            return Ok(Value::NULL);
        }
        let key = self.intern_atom("toISOString");
        let method = self.get_property(p, receiver, key)?;
        self.call_value(p, method, receiver, &[])
    }

    fn date_to_primitive(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if !self.is_object_like(receiver) {
            return Err(self.type_error(
                p,
                "Date.prototype[Symbol.toPrimitive] requires an object".into(),
            ));
        }
        let hint = match args.first().and_then(|value| self.heap.get(*value)) {
            Some(Cell::String(hint))
                if hint.host_string() == "string" || hint.host_string() == "default" =>
            {
                "string"
            }
            Some(Cell::String(hint)) if hint.host_string() == "number" => "number",
            _ => return Err(self.type_error(p, "Invalid hint".into())),
        };
        for name in if hint == "string" {
            ["toString", "valueOf"]
        } else {
            ["valueOf", "toString"]
        } {
            let atom = self.intern_atom(name);
            let method = self.get_property(p, receiver, atom)?;
            if self.is_function(method) {
                let value = self.call_value(p, method, receiver, &[])?;
                if !self.is_object_like(value) {
                    return Ok(value);
                }
            }
        }
        Err(self.type_error(p, "Cannot convert object to primitive value".into()))
    }

    fn date_setter(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        receiver: Value,
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
        let current = match self.heap.get(receiver) {
            Some(Cell::Date { milliseconds, .. }) => *milliseconds,
            _ => {
                return Err(
                    self.type_error(p, "Date method called on incompatible receiver".into())
                );
            }
        };
        let values = args
            .iter()
            .copied()
            .take(setter.maximum_arguments)
            .map(|value| self.to_number(p, value))
            .collect::<Result<Vec<_>, _>>()?;
        if current.is_nan() && !setter.recovers_invalid_date {
            return Ok(Some(f64::NAN));
        }
        let base_time = if current.is_nan() {
            EPOCH_MILLISECONDS
        } else {
            current
        };
        let parts = if setter.utc || current.is_nan() {
            date_parts(base_time, Utc)
        } else {
            date_parts_local(base_time)
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

    pub(super) fn date_construct_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let milliseconds = if args.len() < DATE_CONSTRUCTOR_MULTI_ARGUMENT_THRESHOLD {
            match args.first().copied() {
                None => HostContext::new(&mut self.host).invoke(CapabilityId::ClockMillis, None),
                Some(value) if matches!(self.heap.get(value), Some(Cell::Date { .. })) => {
                    match self.heap.get(value) {
                        Some(Cell::Date { milliseconds, .. }) => *milliseconds,
                        _ => unreachable!(),
                    }
                }
                Some(value) => {
                    let primitive = self.to_primitive(p, value, "default")?;
                    match self.heap.get(primitive) {
                        Some(Cell::String(text)) => parse_date_string(text.host_string()),
                        _ => time_clip(self.to_number(p, primitive)?),
                    }
                }
            }
        } else {
            let mut parts = DEFAULT_DATE_COMPONENTS;
            for (index, value) in args.iter().take(DATE_COMPONENT_COUNT).enumerate() {
                parts[index] = self.to_number(p, *value)?;
            }
            parts[YEAR_COMPONENT] = parts[YEAR_COMPONENT].trunc();
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
    components[YEAR_COMPONENT] = components[YEAR_COMPONENT].trunc();
    if (LEGACY_DATE_YEAR_MIN..=LEGACY_DATE_YEAR_MAX).contains(&components[YEAR_COMPONENT]) {
        components[YEAR_COMPONENT] += f64::from(LEGACY_DATE_YEAR_OFFSET);
    }
    Ok(make_date_milliseconds(components, true))
}

fn date_getter(native: Native, milliseconds: f64) -> Option<f64> {
    if !matches!(
        native,
        Native::DateGetTime
            | Native::DateValueOf
            | Native::DateGetTimezoneOffset
            | Native::DateGetFullYear
            | Native::DateGetMonth
            | Native::DateGetDate
            | Native::DateGetDay
            | Native::DateGetHours
            | Native::DateGetMinutes
            | Native::DateGetSeconds
            | Native::DateGetMilliseconds
            | Native::DateGetUTCFullYear
            | Native::DateGetUTCMonth
            | Native::DateGetUTCDate
            | Native::DateGetUTCDay
            | Native::DateGetUTCHours
            | Native::DateGetUTCMinutes
            | Native::DateGetUTCSeconds
            | Native::DateGetUTCMilliseconds
            | Native::DateGetYear
    ) {
        return None;
    }
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
        date_parts_local(milliseconds)
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

fn date_parts_local(milliseconds: f64) -> Option<DateParts> {
    let date = date_local(milliseconds)?;
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

fn date_local(milliseconds: f64) -> Option<DateTime<FixedOffset>> {
    if !milliseconds.is_finite() || milliseconds.abs() > DATE_TIME_CLIP_LIMIT_MS {
        return None;
    }
    let timestamp = milliseconds.trunc() as i64;
    let offset_seconds = Local
        .timestamp_millis_opt(timestamp)
        .single()
        .map(|date| date.offset().fix().local_minus_utc())
        .unwrap_or_else(|| Local::now().offset().fix().local_minus_utc());
    let offset_seconds = offset_seconds / SECONDS_PER_MINUTE * SECONDS_PER_MINUTE;
    let offset = FixedOffset::east_opt(offset_seconds)?;
    Utc.timestamp_millis_opt(timestamp)
        .single()
        .map(|date| date.with_timezone(&offset))
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
    let year = year.trunc();
    let month = month.trunc();
    let day = day.trunc();
    let hour = hour.trunc();
    let minute = minute.trunc();
    let second = second.trunc();
    let millisecond = millisecond.trunc();
    let normalized_year = year + (month / MONTHS_PER_YEAR).floor();
    if normalized_year < f64::from(i32::MIN) || normalized_year > f64::from(i32::MAX) {
        return f64::NAN;
    }
    let year = normalized_year as i32;
    let month = month.rem_euclid(MONTHS_PER_YEAR) as u32 + 1;
    if utc {
        let day_ms = days_from_civil(f64::from(year), f64::from(month), 1.0) * MILLISECONDS_PER_DAY
            + (day - 1.0) * MILLISECONDS_PER_DAY;
        let time_ms = ((hour * MILLISECONDS_PER_HOUR + minute * MILLISECONDS_PER_MINUTE)
            + second * MILLISECONDS_PER_SECOND)
            + millisecond;
        return time_clip(day_ms + time_ms);
    }
    let day_ms = days_from_civil(f64::from(year), f64::from(month), 1.0) * MILLISECONDS_PER_DAY
        + (day - 1.0) * MILLISECONDS_PER_DAY;
    let local_wall_time_ms = ((hour * MILLISECONDS_PER_HOUR + minute * MILLISECONDS_PER_MINUTE)
        + second * MILLISECONDS_PER_SECOND)
        + millisecond;
    let Some(date) = NaiveDate::from_ymd_opt(year, month, 1) else {
        let offset =
            Local::now().offset().fix().local_minus_utc() / SECONDS_PER_MINUTE * SECONDS_PER_MINUTE;
        return time_clip(
            day_ms + local_wall_time_ms - f64::from(offset) * MILLISECONDS_PER_SECOND,
        );
    };
    let Some(day_offset) = finite_i64(day - 1.0) else {
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
        let offset =
            Local::now().offset().fix().local_minus_utc() / SECONDS_PER_MINUTE * SECONDS_PER_MINUTE;
        return time_clip(
            day_ms + local_wall_time_ms - f64::from(offset) * MILLISECONDS_PER_SECOND,
        );
    };
    let milliseconds = if utc {
        date_time.and_utc().timestamp_millis() as f64
    } else {
        match Local.from_local_datetime(&date_time) {
            LocalResult::Single(date) => {
                let offset =
                    date.offset().fix().local_minus_utc() / SECONDS_PER_MINUTE * SECONDS_PER_MINUTE;
                date_time.and_utc().timestamp_millis() as f64
                    - f64::from(offset) * MILLISECONDS_PER_SECOND
            }
            LocalResult::Ambiguous(first, second) => {
                let wall_time = date_time.and_utc().timestamp_millis() as f64;
                let first_offset = first.offset().fix().local_minus_utc() / SECONDS_PER_MINUTE
                    * SECONDS_PER_MINUTE;
                let second_offset = second.offset().fix().local_minus_utc() / SECONDS_PER_MINUTE
                    * SECONDS_PER_MINUTE;
                (wall_time - f64::from(first_offset) * MILLISECONDS_PER_SECOND)
                    .min(wall_time - f64::from(second_offset) * MILLISECONDS_PER_SECOND)
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
        let clipped = value.trunc();
        if clipped == 0.0 {
            EPOCH_MILLISECONDS
        } else {
            clipped
        }
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

fn format_date_iso(milliseconds: f64) -> Option<String> {
    if !milliseconds.is_finite() || milliseconds.abs() > DATE_TIME_CLIP_LIMIT_MS {
        return None;
    }
    let whole = milliseconds.trunc() as i64;
    let days = whole.div_euclid(DATE_MILLISECONDS_PER_DAY);
    let time = whole.rem_euclid(DATE_MILLISECONDS_PER_DAY);
    let (year, month, day) = civil_from_days(days);
    let hour = time / DATE_MILLISECONDS_PER_HOUR;
    let minute = (time / DATE_MILLISECONDS_PER_MINUTE) % i64::from(DATE_MINUTES_PER_HOUR);
    let second = (time / DATE_MILLISECONDS_PER_SECOND) % i64::from(DATE_MINUTES_PER_HOUR);
    let millisecond = time % DATE_MILLISECONDS_PER_SECOND;
    Some(format!(
        "{}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millisecond:03}Z",
        format_iso_year(year),
    ))
}

fn format_iso_year(year: i64) -> String {
    if (0..=ISO_YEAR_MAX).contains(&year) {
        format!("{year:04}")
    } else if year < 0 {
        format!(
            "-{:0width$}",
            year.unsigned_abs(),
            width = ISO_EXTENDED_YEAR_WIDTH
        )
    } else {
        format!("+{year:0width$}", width = ISO_EXTENDED_YEAR_WIDTH)
    }
}

fn days_from_civil(year: f64, month: f64, day: f64) -> f64 {
    let year = year - f64::from(month <= 2.0);
    let years_per_cycle = DATE_YEARS_PER_CYCLE as f64;
    let days_per_cycle = DATE_DAYS_PER_CYCLE as f64;
    let era = (year / years_per_cycle).floor();
    let year_of_era = year - era * years_per_cycle;
    let month = month + if month > 2.0 { -3.0 } else { 9.0 };
    let day_of_year =
        ((DATE_MONTHS_PER_CYCLE_SEGMENT as f64 * month + 2.0) / 5.0).floor() + day - 1.0;
    era * days_per_cycle + year_of_era * DATE_DAYS_PER_YEAR as f64 + (year_of_era / 4.0).floor()
        - (year_of_era / 100.0).floor()
        + day_of_year
        - DATE_EPOCH_DAY_OFFSET as f64
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + DATE_EPOCH_DAY_OFFSET;
    let era = if days >= 0 {
        days
    } else {
        days - (DATE_DAYS_PER_CYCLE - 1)
    } / DATE_DAYS_PER_CYCLE;
    let day_of_era = days - era * DATE_DAYS_PER_CYCLE;
    let year_of_era = (day_of_era - day_of_era / DATE_QUADRENNIAL_DAYS
        + day_of_era / DATE_CENTURY_DAYS
        - day_of_era / (DATE_DAYS_PER_CYCLE - 1))
        / DATE_DAYS_PER_YEAR;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / DATE_MONTHS_PER_CYCLE_SEGMENT;
    (
        year + i64::from(month_part >= 10),
        month_part + if month_part < 10 { 3 } else { -9 },
        day_of_year - (DATE_MONTHS_PER_CYCLE_SEGMENT * month_part + 2) / 5 + 1,
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

fn format_date_date_string(milliseconds: f64) -> String {
    let Some(date) = date_local(milliseconds) else {
        return "Invalid Date".into();
    };
    format!(
        "{} {} {:02} {}",
        DATE_WEEKDAYS[date.weekday().num_days_from_sunday() as usize],
        DATE_MONTHS[date.month0() as usize],
        date.day(),
        display_date_year(date.year()),
    )
}

fn format_date_time_string(milliseconds: f64) -> String {
    let Some(date) = date_local(milliseconds) else {
        return "Invalid Date".into();
    };
    let offset = date.offset().fix().local_minus_utc() / SECONDS_PER_MINUTE;
    let sign = if offset < 0 { '-' } else { '+' };
    format!(
        "{:02}:{:02}:{:02} GMT{sign}{:02}{:02}",
        date.hour(),
        date.minute(),
        date.second(),
        offset.unsigned_abs() / DATE_MINUTES_PER_HOUR,
        offset.unsigned_abs() % DATE_MINUTES_PER_HOUR,
    )
}

fn parse_date_string(text: &str) -> f64 {
    if let Some(milliseconds) = parse_iso_date_string(text) {
        return milliseconds;
    }
    if text.len() == DATE_YEAR_MINIMUM_WIDTH && text.bytes().all(|byte| byte.is_ascii_digit()) {
        if let Ok(year) = text.parse::<i32>() {
            return make_date_milliseconds(
                [f64::from(year), 0.0, FIRST_DAY_OF_MONTH, 0.0, 0.0, 0.0, 0.0],
                true,
            );
        }
    }
    chrono::DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|date| time_clip(date.timestamp_millis() as f64))
        .or_else(|| {
            chrono::DateTime::parse_from_str(text, "%a %b %e %Y %H:%M:%S GMT%z")
                .ok()
                .map(|date| time_clip(date.timestamp_millis() as f64))
        })
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(text, "%a, %d %b %Y %H:%M:%S GMT")
                .ok()
                .map(|date| time_clip(date.and_utc().timestamp_millis() as f64))
        })
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f")
                .ok()
                .and_then(|local| match Local.from_local_datetime(&local) {
                    LocalResult::Single(date) => Some(time_clip(date.timestamp_millis() as f64)),
                    LocalResult::Ambiguous(first, second) => Some(time_clip(
                        first.timestamp_millis().min(second.timestamp_millis()) as f64,
                    )),
                    LocalResult::None => None,
                })
        })
        .unwrap_or(f64::NAN)
}

fn parse_iso_date_string(text: &str) -> Option<f64> {
    let (date, time) = text.split_once('T').unwrap_or((text, ""));
    let year_width = if date.starts_with('+') || date.starts_with('-') {
        DATE_YEAR_MINIMUM_WIDTH + 3
    } else {
        DATE_YEAR_MINIMUM_WIDTH
    };
    let year = date.get(..year_width)?.parse::<i32>().ok()?;
    if date.get(..year_width)? == "-000000" {
        return None;
    }
    let date_tail = date.get(year_width..)?.strip_prefix('-')?;
    let (month, day) = date_tail.split_once('-')?;
    let month = month.parse::<f64>().ok()?;
    let day = day.parse::<f64>().ok()?;
    if time.is_empty() {
        return Some(make_date_milliseconds(
            [f64::from(year), month - 1.0, day, 0.0, 0.0, 0.0, 0.0],
            true,
        ));
    }
    let (time, offset_minutes) = if let Some(time) = time.strip_suffix('Z') {
        (time, 0.0)
    } else if let Some(index) = time.rfind(|character| character == '+' || character == '-') {
        let (clock, offset) = time.split_at(index);
        let (hours, minutes) = offset[1..].split_once(':')?;
        let sign = if offset.starts_with('-') { -1.0 } else { 1.0 };
        (
            clock,
            sign * (hours.parse::<f64>().ok()? * f64::from(DATE_MINUTES_PER_HOUR)
                + minutes.parse::<f64>().ok()?),
        )
    } else {
        (time, f64::NAN)
    };
    let (hour, rest) = time.split_once(':')?;
    let (minute, rest) = rest.split_once(':').unwrap_or((rest, "0"));
    let (second, fraction) = rest.split_once('.').unwrap_or((rest, "0"));
    let millisecond = fraction
        .chars()
        .take(DATE_MILLISECOND_DIGITS)
        .collect::<String>()
        .chars()
        .chain(std::iter::repeat('0'))
        .take(DATE_MILLISECOND_DIGITS)
        .collect::<String>()
        .parse::<f64>()
        .ok()?;
    let components = [
        f64::from(year),
        month - 1.0,
        day,
        hour.parse().ok()?,
        minute.parse().ok()?,
        second.parse().ok()?,
        millisecond,
    ];
    let milliseconds = if offset_minutes.is_nan() {
        make_date_milliseconds(components, false)
    } else {
        make_date_milliseconds(components, true) - offset_minutes * MILLISECONDS_PER_MINUTE
    };
    Some(time_clip(milliseconds))
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
