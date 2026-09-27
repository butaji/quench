use super::*;
use chrono::{Datelike, Duration, Offset, TimeZone, Timelike, Utc};
use std::cmp::Ordering;

const MAX_EPOCH_NANOSECONDS: i128 = 8_640_000_000_000_000_000_000;
const NANOSECONDS_PER_SECOND: i128 = 1_000_000_000;
const NANOSECONDS_PER_MILLISECOND: u32 = 1_000_000;
const NANOSECONDS_PER_MICROSECOND: u32 = 1_000;
const MICROSECONDS_PER_MILLISECOND: u32 = 1_000;
const SECONDS_PER_HOUR: i32 = 3_600;
const SECONDS_PER_MINUTE: i32 = 60;
const ISO_YEAR_DIGITS: usize = 4;
const EXTENDED_YEAR_DIGITS: usize = 6;
const MAX_BASIC_ISO_YEAR: i32 = 9_999;
const ISO_MONTH_DIGITS: usize = 2;
const ISO_DAY_DIGITS: usize = 2;
const MAX_FRACTION_DIGITS: usize = 9;
const ISO_TIME_FIELD_DIGITS: usize = 2;
const ISO_HOUR_LIMIT: u32 = 23;
const ISO_MINUTE_LIMIT: u32 = 59;
const ISO_SECOND_LIMIT: u32 = 59;
const ISO_LEAP_SECOND: u32 = ISO_SECOND_LIMIT + 1;
const NANOSECONDS_PER_MINUTE: i128 = SECONDS_PER_MINUTE as i128 * NANOSECONDS_PER_SECOND;
const NANOSECONDS_PER_HOUR: i128 = SECONDS_PER_HOUR as i128 * NANOSECONDS_PER_SECOND;
const HOURS_PER_DAY: i128 = 24;
const NANOSECONDS_PER_DAY: i128 = HOURS_PER_DAY * NANOSECONDS_PER_HOUR;
const FRACTIONAL_MILLISECOND_DIGITS: usize = 3;
const FRACTIONAL_MICROSECOND_DIGITS: usize = 6;
const SMALLEST_UNITS: [&str; 5] = [
    "minute",
    "second",
    "millisecond",
    "microsecond",
    "nanosecond",
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
const CALENDAR_NAME_OPTIONS: [&str; 4] = ["auto", "always", "never", "critical"];
const OFFSET_DISPLAY_OPTIONS: [&str; 2] = ["auto", "never"];
const TIME_ZONE_NAME_OPTIONS: [&str; 3] = ["auto", "never", "critical"];
const ROUNDING_TIE_FACTOR: i128 = 2;
const DECIMAL_RADIX: u32 = 10;
const DECIMAL_RADIX_I128: i128 = 10;

struct ZonedDateTimeStringOptions {
    calendar_name: String,
    fractional_second_digits: Option<usize>,
    offset: String,
    rounding_mode: String,
    smallest_unit: Option<String>,
    time_zone_name: String,
}

impl Default for ZonedDateTimeStringOptions {
    fn default() -> Self {
        Self {
            calendar_name: "auto".into(),
            fractional_second_digits: None,
            offset: "auto".into(),
            rounding_mode: "trunc".into(),
            smallest_unit: None,
            time_zone_name: "auto".into(),
        }
    }
}
const DISAMBIGUATION_OPTIONS: [&str; 4] = ["compatible", "earlier", "later", "reject"];
const OFFSET_OPTIONS: [&str; 4] = ["prefer", "use", "ignore", "reject"];
const OVERFLOW_OPTIONS: [&str; 2] = ["constrain", "reject"];

const ZONED_DATE_TIME_GETTERS: [(&str, Native); 12] = [
    (
        "epochNanoseconds",
        Native::TemporalZonedDateTimeEpochNanosecondsGetter,
    ),
    ("timeZoneId", Native::TemporalZonedDateTimeTimeZoneIdGetter),
    ("calendarId", Native::TemporalZonedDateTimeCalendarIdGetter),
    ("year", Native::TemporalZonedDateTimeYearGetter),
    ("month", Native::TemporalZonedDateTimeMonthGetter),
    ("day", Native::TemporalZonedDateTimeDayGetter),
    ("hour", Native::TemporalZonedDateTimeHourGetter),
    ("minute", Native::TemporalZonedDateTimeMinuteGetter),
    ("second", Native::TemporalZonedDateTimeSecondGetter),
    (
        "millisecond",
        Native::TemporalZonedDateTimeMillisecondGetter,
    ),
    (
        "microsecond",
        Native::TemporalZonedDateTimeMicrosecondGetter,
    ),
    ("nanosecond", Native::TemporalZonedDateTimeNanosecondGetter),
];
const ZONED_DATE_TIME_METHODS: [(&str, Native); 4] = [
    ("equals", Native::TemporalZonedDateTimeEquals),
    ("withTimeZone", Native::TemporalZonedDateTimeWithTimeZone),
    ("toString", Native::TemporalZonedDateTimeToString),
    ("toJSON", Native::TemporalZonedDateTimeToJSON),
];

impl<H: Host> Vm<H> {
    pub(super) fn install_temporal_zoned_date_time(
        &mut self,
        p: &ResidualProgram,
        temporal: Value,
    ) -> Result<(), JsError> {
        let constructor =
            self.native_with_realm(Native::TemporalZonedDateTime, temporal, self.realm.globals);
        self.set_builtin_function_name(constructor, "ZonedDateTime")?;
        let prototype = self.object();
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
        self.set_builtin_named(p, constructor, "from", Native::TemporalZonedDateTimeFrom)?;
        self.set_builtin_named(
            p,
            constructor,
            "compare",
            Native::TemporalZonedDateTimeCompare,
        )?;
        for (name, native) in ZONED_DATE_TIME_GETTERS {
            let getter = self.native_value(native);
            self.set_builtin_function_name(getter, &format!("get {name}"))?;
            let atom = self.intern_atom(name);
            self.set_property(prototype, atom, Value::UNDEFINED)?;
            self.set_property_attributes(
                prototype,
                PropertyKey::string(atom),
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: true,
                    accessor: true,
                    getter: Some(getter),
                    setter: None,
                },
            );
        }
        for (name, native) in ZONED_DATE_TIME_METHODS {
            self.set_builtin_named(p, prototype, name, native)?;
        }
        if let Some(symbol) = self.well_known_symbols.get("toStringTag").copied() {
            let tag = self
                .heap
                .alloc(Cell::String("Temporal.ZonedDateTime".into()));
            self.set_symbol_property(prototype, symbol, tag)?;
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
        self.set_builtin_value_named(temporal, "ZonedDateTime", constructor)
    }

    pub(super) fn temporal_zoned_date_time_construct(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        let epoch = match args.first().copied().unwrap_or(Value::UNDEFINED) {
            value if matches!(self.heap.get(value), Some(Cell::BigInt(_))) => {
                let Some(Cell::BigInt(text)) = self.heap.get(value) else {
                    unreachable!()
                };
                text.parse::<i128>()
                    .map_err(|_| self.range_error(p, "Invalid epochNanoseconds".into()))?
            }
            value if value.as_bool().is_some() => i128::from(value.as_bool().unwrap_or(false)),
            _ => return Err(self.type_error(p, "Invalid epochNanoseconds".into())),
        };
        if epoch.unsigned_abs() > MAX_EPOCH_NANOSECONDS as u128 {
            return Err(self.range_error(p, "Invalid epochNanoseconds".into()));
        }
        let timezone_value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let timezone = self.temporal_timezone_id(p, timezone_value)?;
        let calendar_value = args.get(2).copied().unwrap_or(Value::UNDEFINED);
        let calendar = if calendar_value.is_undefined() {
            "iso8601".to_owned()
        } else if matches!(self.heap.get(calendar_value), Some(Cell::String(_))) {
            let text = self.to_string(p, calendar_value)?.to_string();
            super::temporal_date_parse::parse_calendar_identifier(&text)
                .ok_or_else(|| self.range_error(p, "Invalid calendar".into()))?
        } else {
            return Err(self.type_error(p, "Invalid calendar".into()));
        };
        let prototype_atom = self.intern_atom("prototype");
        let prototype = self.get_property(p, new_target, prototype_atom)?;
        let prototype = if self.is_object_like(prototype) {
            prototype
        } else {
            self.object_proto
        };
        Ok(self.heap.alloc(Cell::TemporalZonedDateTime {
            object: Box::new(Self::empty_object(prototype)),
            epoch_nanoseconds: epoch,
            time_zone: timezone,
            calendar,
        }))
    }

    fn temporal_timezone_id(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<String, JsError> {
        let Some(Cell::String(_)) = self.heap.get(value) else {
            return Err(self.type_error(p, "Invalid time zone".into()));
        };
        let text = self.to_string(p, value)?.to_string();
        if text.eq_ignore_ascii_case("utc") {
            return Ok("UTC".into());
        }
        if text.starts_with(['+', '-']) {
            if !valid_time_zone_offset(&text) {
                return Err(self.range_error(p, "Invalid time zone".into()));
            }
            let seconds = quench_temporal::offset_seconds(&text);
            let sign = if seconds < 0 { '-' } else { '+' };
            let seconds = seconds.unsigned_abs();
            let hours = seconds / SECONDS_PER_HOUR as u32;
            let minutes = seconds / SECONDS_PER_MINUTE as u32 % SECONDS_PER_MINUTE as u32;
            return Ok(format!("{sign}{hours:02}:{minutes:02}"));
        }
        if let Some(identifier) = time_zone_from_datetime_identifier(&text) {
            return Ok(identifier);
        }
        text.parse::<chrono_tz::Tz>()
            .map(|zone| zone.to_string())
            .map_err(|_| self.range_error(p, "Invalid time zone".into()))
    }

    pub(super) fn temporal_zoned_date_time_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if native == Native::TemporalZonedDateTimeFrom {
            let options = args.get(1).copied().unwrap_or(Value::UNDEFINED);
            let input = self.temporal_zoned_date_time_record(
                p,
                args.first().copied().unwrap_or(Value::UNDEFINED),
                options,
            )?;
            return self.make_temporal_zoned_date_time(p, this, input);
        }
        if native == Native::TemporalZonedDateTimeCompare {
            let left = self.temporal_zoned_date_time_record(
                p,
                args.first().copied().unwrap_or(Value::UNDEFINED),
                Value::UNDEFINED,
            )?;
            let right = self.temporal_zoned_date_time_record(
                p,
                args.get(1).copied().unwrap_or(Value::UNDEFINED),
                Value::UNDEFINED,
            )?;
            let ordering = left.epoch_nanoseconds.cmp(&right.epoch_nanoseconds);
            return Ok(Value::number(match ordering {
                Ordering::Less => -1.0,
                Ordering::Equal => 0.0,
                Ordering::Greater => 1.0,
            }));
        }
        if native == Native::TemporalZonedDateTimeEquals {
            let Some(Cell::TemporalZonedDateTime {
                epoch_nanoseconds,
                time_zone,
                calendar,
                ..
            }) = self.heap.get(this)
            else {
                return Err(self.type_error(
                    p,
                    "Temporal.ZonedDateTime method called on incompatible receiver".into(),
                ));
            };
            let receiver = ZonedDateTimeRecord {
                epoch_nanoseconds: *epoch_nanoseconds,
                time_zone: time_zone.clone(),
                calendar: calendar.clone(),
            };
            let other = self.temporal_zoned_date_time_record(
                p,
                args.first().copied().unwrap_or(Value::UNDEFINED),
                Value::UNDEFINED,
            )?;
            return Ok(
                if receiver.epoch_nanoseconds == other.epoch_nanoseconds
                    && receiver.time_zone == other.time_zone
                    && receiver.calendar == other.calendar
                {
                    Value::TRUE
                } else {
                    Value::FALSE
                },
            );
        }
        if native == Native::TemporalZonedDateTimeWithTimeZone {
            let Some(Cell::TemporalZonedDateTime {
                object,
                epoch_nanoseconds,
                calendar,
                ..
            }) = self.heap.get(this)
            else {
                return Err(self.type_error(
                    p,
                    "Temporal.ZonedDateTime method called on incompatible receiver".into(),
                ));
            };
            let epoch_nanoseconds = *epoch_nanoseconds;
            let calendar = calendar.clone();
            let prototype = object.proto;
            let zone = args.first().copied().unwrap_or(Value::UNDEFINED);
            let time_zone = self.temporal_timezone_id(p, zone)?;
            return Ok(self.heap.alloc(Cell::TemporalZonedDateTime {
                object: Box::new(Self::empty_object(prototype)),
                epoch_nanoseconds,
                time_zone,
                calendar,
            }));
        }
        if matches!(
            native,
            Native::TemporalZonedDateTimeToString | Native::TemporalZonedDateTimeToJSON
        ) {
            let options = if native == Native::TemporalZonedDateTimeToJSON {
                ZonedDateTimeStringOptions::default()
            } else {
                self.temporal_zoned_date_time_string_options(
                    p,
                    args.first().copied().unwrap_or(Value::UNDEFINED),
                )?
            };
            return self.temporal_zoned_date_time_to_string(p, this, options);
        }
        if native == Native::TemporalZonedDateTime {
            return Err(self.type_error(p, "Temporal.ZonedDateTime requires new".into()));
        }
        let Some(Cell::TemporalZonedDateTime {
            epoch_nanoseconds,
            time_zone,
            calendar,
            ..
        }) = self.heap.get(this)
        else {
            return Err(self.type_error(
                p,
                "Temporal.ZonedDateTime method called on incompatible receiver".into(),
            ));
        };
        let (epoch, zone, calendar) = (*epoch_nanoseconds, time_zone.clone(), calendar.clone());
        match native {
            Native::TemporalZonedDateTimeEpochNanosecondsGetter => {
                Ok(self.heap.alloc(Cell::BigInt(epoch.to_string())))
            }
            Native::TemporalZonedDateTimeTimeZoneIdGetter => {
                Ok(self.heap.alloc(Cell::String(zone.into())))
            }
            Native::TemporalZonedDateTimeCalendarIdGetter => {
                Ok(self.heap.alloc(Cell::String(calendar.into())))
            }
            _ => {
                let fields = zoned_date_time_fields(epoch, &zone)
                    .ok_or_else(|| self.range_error(p, "Invalid epochNanoseconds".into()))?;
                let value = match native {
                    Native::TemporalZonedDateTimeYearGetter => fields[0],
                    Native::TemporalZonedDateTimeMonthGetter => fields[1],
                    Native::TemporalZonedDateTimeDayGetter => fields[2],
                    Native::TemporalZonedDateTimeHourGetter => fields[3],
                    Native::TemporalZonedDateTimeMinuteGetter => fields[4],
                    Native::TemporalZonedDateTimeSecondGetter => fields[5],
                    Native::TemporalZonedDateTimeMillisecondGetter => fields[6],
                    Native::TemporalZonedDateTimeMicrosecondGetter => fields[7],
                    Native::TemporalZonedDateTimeNanosecondGetter => fields[8],
                    _ => return Err(self.type_error(p, "Unsupported ZonedDateTime getter".into())),
                };
                Ok(Value::number(f64::from(value)))
            }
        }
    }

    fn temporal_zoned_date_time_record(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        options: Value,
    ) -> Result<ZonedDateTimeRecord, JsError> {
        match self.heap.get(value).cloned() {
            Some(Cell::TemporalZonedDateTime {
                epoch_nanoseconds,
                time_zone,
                calendar,
                ..
            }) => {
                self.validate_zoned_date_time_options(p, options)?;
                Ok(ZonedDateTimeRecord {
                    epoch_nanoseconds,
                    time_zone,
                    calendar,
                })
            }
            Some(Cell::String(_)) => {
                let text = self.to_string(p, value)?.to_string();
                let record = parse_zoned_date_time_string(&text)
                    .ok_or_else(|| self.range_error(p, "Invalid ZonedDateTime".into()))?;
                self.validate_zoned_date_time_options(p, options)?;
                Ok(record)
            }
            Some(Cell::Object(_)) | Some(Cell::Function { .. }) | Some(Cell::Proxy { .. }) => {
                self.temporal_zoned_date_time_property_bag(p, value, options)
            }
            _ => Err(self.type_error(p, "Invalid ZonedDateTime value".into())),
        }
    }

    fn validate_zoned_date_time_options(
        &mut self,
        p: &ResidualProgram,
        options: Value,
    ) -> Result<bool, JsError> {
        if options.is_undefined() {
            return Ok(true);
        }
        if !self.is_object_like(options) {
            return Err(self.type_error(p, "Invalid options".into()));
        }
        let mut constrain = true;
        for (name, allowed) in [
            ("disambiguation", &DISAMBIGUATION_OPTIONS[..]),
            ("offset", &OFFSET_OPTIONS[..]),
            ("overflow", &OVERFLOW_OPTIONS[..]),
        ] {
            let atom = self.intern_atom(name);
            let value = self.get_property(p, options, atom)?;
            if value.is_undefined() {
                continue;
            }
            let value = self.to_string(p, value)?.to_string();
            if !allowed.contains(&value.as_str()) {
                return Err(self.range_error(p, "Invalid Temporal option".into()));
            }
            if name == "overflow" {
                constrain = value == "constrain";
            }
        }
        Ok(constrain)
    }

    fn temporal_zoned_date_time_to_string(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        options: ZonedDateTimeStringOptions,
    ) -> Result<Value, JsError> {
        let Some(Cell::TemporalZonedDateTime {
            epoch_nanoseconds,
            time_zone,
            calendar,
            ..
        }) = self.heap.get(this)
        else {
            return Err(self.type_error(
                p,
                "Temporal.ZonedDateTime method called on incompatible receiver".into(),
            ));
        };
        let epoch = *epoch_nanoseconds;
        let time_zone = time_zone.clone();
        let calendar = calendar.clone();
        let epoch = round_zoned_date_time_epoch(epoch, &time_zone, &options)
            .ok_or_else(|| self.range_error(p, "Invalid epochNanoseconds".into()))?;
        let fields = zoned_date_time_fields(epoch, &time_zone)
            .ok_or_else(|| self.range_error(p, "Invalid epochNanoseconds".into()))?;
        let offset = timezone_offset_nanoseconds(&time_zone, epoch)
            .ok_or_else(|| self.range_error(p, "Invalid time zone".into()))?;
        let fractional = fields[6] * NANOSECONDS_PER_MILLISECOND as i32
            + fields[7] * NANOSECONDS_PER_MICROSECOND as i32
            + fields[8];
        let fraction = if let Some(unit) = options.smallest_unit.as_deref() {
            match unit {
                "hour" | "minute" | "second" => String::new(),
                "millisecond" => format_fraction(fractional, FRACTIONAL_MILLISECOND_DIGITS),
                "microsecond" => format_fraction(fractional, FRACTIONAL_MICROSECOND_DIGITS),
                "nanosecond" => format_fraction(fractional, MAX_FRACTION_DIGITS),
                _ => String::new(),
            }
        } else if let Some(digits) = options.fractional_second_digits {
            format_fraction(fractional, digits)
        } else if fractional == 0 {
            String::new()
        } else {
            let digits = format!("{fractional:09}");
            format!(".{}", digits.trim_end_matches('0'))
        };
        let calendar_annotation = format_calendar_annotation(&calendar, &options.calendar_name);
        let zone_annotation = format_time_zone_annotation(&time_zone, &options.time_zone_name);
        let time = format_zoned_time(&fields, &fraction, options.smallest_unit.as_deref());
        let offset = if options.offset == "never" {
            String::new()
        } else {
            format_offset_nanoseconds(offset)
        };
        let result = format!(
            "{}-{:02}-{:02}T{time}{offset}{zone_annotation}{calendar_annotation}",
            format_iso_year(fields[0]),
            fields[1],
            fields[2],
        );
        Ok(self.heap.alloc(Cell::String(result.into())))
    }

    fn temporal_zoned_date_time_string_options(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<ZonedDateTimeStringOptions, JsError> {
        if value.is_undefined() {
            return Ok(ZonedDateTimeStringOptions::default());
        }
        if !self.is_object_like(value) {
            return Err(self.type_error(p, "Options must be an object".into()));
        }
        let calendar_name = self.temporal_option_string(p, value, "calendarName")?;
        let fractional_value = self.get_option_property(p, value, "fractionalSecondDigits")?;
        let fractional_is_number = fractional_value.as_number().is_some();
        let fractional_text = if fractional_value.is_undefined() {
            None
        } else {
            Some(self.to_string(p, fractional_value)?.to_string())
        };
        let offset = self.temporal_option_string(p, value, "offset")?;
        let rounding_mode = self.temporal_option_string(p, value, "roundingMode")?;
        let smallest_unit = self
            .temporal_option_string(p, value, "smallestUnit")?
            .map(|unit| normalize_smallest_unit(&unit).to_owned());
        let time_zone_name = self.temporal_option_string(p, value, "timeZoneName")?;
        let fractional = self.parse_fractional_second_digits(
            p,
            fractional_text.as_deref(),
            fractional_is_number,
        )?;
        let options = ZonedDateTimeStringOptions {
            calendar_name: calendar_name.unwrap_or_else(|| "auto".into()),
            fractional_second_digits: fractional,
            offset: offset.unwrap_or_else(|| "auto".into()),
            rounding_mode: rounding_mode.unwrap_or_else(|| "trunc".into()),
            smallest_unit,
            time_zone_name: time_zone_name.unwrap_or_else(|| "auto".into()),
        };
        if !CALENDAR_NAME_OPTIONS.contains(&options.calendar_name.as_str())
            || !OFFSET_DISPLAY_OPTIONS.contains(&options.offset.as_str())
            || !ROUNDING_MODES.contains(&options.rounding_mode.as_str())
            || options
                .smallest_unit
                .as_deref()
                .is_some_and(|unit| !SMALLEST_UNITS.contains(&unit))
            || !TIME_ZONE_NAME_OPTIONS.contains(&options.time_zone_name.as_str())
        {
            return Err(self.range_error(p, "Invalid ZonedDateTime string option".into()));
        }
        Ok(options)
    }

    fn temporal_option_string(
        &mut self,
        p: &ResidualProgram,
        options: Value,
        name: &str,
    ) -> Result<Option<String>, JsError> {
        let value = self.get_option_property(p, options, name)?;
        if value.is_undefined() {
            Ok(None)
        } else {
            self.to_string(p, value)
                .map(|value| Some(value.to_string()))
        }
    }

    fn parse_fractional_second_digits(
        &mut self,
        p: &ResidualProgram,
        value: Option<&str>,
        is_number: bool,
    ) -> Result<Option<usize>, JsError> {
        let Some(value) = value else {
            return Ok(None);
        };
        if !is_number {
            if value == "auto" {
                return Ok(None);
            }
            return Err(self.range_error(p, "Invalid fractionalSecondDigits".into()));
        }
        let digits = value
            .parse::<f64>()
            .ok()
            .map(f64::floor)
            .filter(|digits| {
                digits.is_finite() && (0.0..=MAX_FRACTION_DIGITS as f64).contains(digits)
            })
            .ok_or_else(|| self.range_error(p, "Invalid fractionalSecondDigits".into()))?;
        Ok(Some(digits as usize))
    }

    fn get_option_property(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        name: &str,
    ) -> Result<Value, JsError> {
        let atom = self.intern_atom(name);
        self.get_property(p, object, atom)
    }

    fn temporal_zoned_date_time_property_bag(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        options: Value,
    ) -> Result<ZonedDateTimeRecord, JsError> {
        let calendar_atom = self.intern_atom("calendar");
        let calendar_value = self.get_property(p, value, calendar_atom)?;
        let calendar = if calendar_value.is_undefined() {
            "iso8601".to_owned()
        } else if matches!(self.heap.get(calendar_value), Some(Cell::String(_))) {
            let text = self.to_string(p, calendar_value)?.to_string();
            super::temporal_date_parse::parse_calendar_identifier(&text)
                .ok_or_else(|| self.range_error(p, "Invalid calendar".into()))?
        } else {
            return Err(self.type_error(p, "Invalid calendar".into()));
        };
        let day = self.temporal_date_bag_field(p, value, "day")?;
        let hour = self.temporal_date_bag_field(p, value, "hour")?;
        let microsecond = self.temporal_date_bag_field(p, value, "microsecond")?;
        let millisecond = self.temporal_date_bag_field(p, value, "millisecond")?;
        let minute = self.temporal_date_bag_field(p, value, "minute")?;
        let month = self.temporal_date_bag_field(p, value, "month")?;
        let month_code_atom = self.intern_atom("monthCode");
        let month_code_value = self.get_property(p, value, month_code_atom)?;
        let month_code = if month_code_value.is_undefined() {
            None
        } else {
            Some(self.to_string(p, month_code_value)?.to_string())
        };
        let nanosecond = self.temporal_date_bag_field(p, value, "nanosecond")?;
        let offset_atom = self.intern_atom("offset");
        let offset_value = self.get_property(p, value, offset_atom)?;
        let offset = if offset_value.is_undefined() {
            None
        } else {
            if !matches!(self.heap.get(offset_value), Some(Cell::String(_))) {
                return Err(self.type_error(p, "Invalid offset".into()));
            }
            let offset = self.to_string(p, offset_value)?.to_string();
            if !quench_temporal::valid_timezone_offset(&offset) {
                return Err(self.range_error(p, "Invalid offset".into()));
            }
            Some(offset)
        };
        let second = self.temporal_date_bag_field(p, value, "second")?;
        let timezone_atom = self.intern_atom("timeZone");
        let timezone_value = self.get_property(p, value, timezone_atom)?;
        let timezone = self.temporal_timezone_id(p, timezone_value)?;
        let year_atom = self.intern_atom("year");
        let year_value = self.get_property(p, value, year_atom)?;
        if year_value.is_undefined() {
            return Err(self.type_error(p, "Missing ZonedDateTime field".into()));
        }
        let month_code = month_code
            .map(|code| {
                let value = self.heap.alloc(Cell::String(code.into()));
                self.plain_date_month_code(p, value)
            })
            .transpose()?;
        let year = self.plain_date_optional_integer(p, year_value)?;
        let constrain = self.validate_zoned_date_time_options(p, options)?;
        let (Some(year), Some(day)) = (year, day) else {
            return Err(self.type_error(p, "Missing ZonedDateTime field".into()));
        };
        let Some(month) = month.or(month_code) else {
            return Err(self.type_error(p, "Missing ZonedDateTime field".into()));
        };
        if month_code.is_some_and(|month_code| month_code != month) {
            return Err(self.range_error(p, "month and monthCode must agree".into()));
        }
        let (month, day) = if constrain {
            let month = month.clamp(1, super::temporal_date::ISO_MONTHS_PER_YEAR);
            let last_day = super::temporal_date::iso_days_in_month(year, month).unwrap_or(31);
            (month, day.clamp(1, last_day))
        } else {
            (month, day)
        };
        let date = super::temporal_date::checked_iso_date(year, month, day)
            .ok_or_else(|| self.range_error(p, "Invalid ZonedDateTime".into()))?;
        let local_date = chrono::NaiveDate::from_ymd_opt(date.year, date.month, date.day)
            .ok_or_else(|| self.range_error(p, "Invalid ZonedDateTime".into()))?;
        let local = local_date
            .and_hms_nano_opt(
                hour.unwrap_or(0) as u32,
                minute.unwrap_or(0) as u32,
                second.unwrap_or(0) as u32,
                millisecond.unwrap_or(0) as u32 * NANOSECONDS_PER_MILLISECOND
                    + microsecond.unwrap_or(0) as u32 * NANOSECONDS_PER_MICROSECOND
                    + nanosecond.unwrap_or(0) as u32,
            )
            .ok_or_else(|| self.range_error(p, "Invalid ZonedDateTime".into()))?;
        let epoch_nanoseconds = zoned_local_epoch(local, &timezone)
            .ok_or_else(|| self.range_error(p, "Invalid ZonedDateTime".into()))?;
        if let Some(offset) = offset {
            if !quench_temporal::valid_timezone_offset(&offset) {
                return Err(self.range_error(p, "Invalid offset".into()));
            }
            let offset_seconds = quench_temporal::offset_seconds(&offset);
            let offset_epoch = i128::from(local.and_utc().timestamp()) * NANOSECONDS_PER_SECOND
                + i128::from(local.and_utc().timestamp_subsec_nanos())
                - i128::from(offset_seconds) * NANOSECONDS_PER_SECOND;
            if offset_epoch != epoch_nanoseconds {
                return Err(self.range_error(p, "Offset does not match time zone".into()));
            }
        }
        Ok(ZonedDateTimeRecord {
            epoch_nanoseconds,
            time_zone: timezone,
            calendar,
        })
    }

    fn temporal_date_bag_field(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        name: &str,
    ) -> Result<Option<i32>, JsError> {
        let atom = self.intern_atom(name);
        let value = self.get_property(p, value, atom)?;
        if value.is_undefined() {
            return Ok(None);
        }
        self.plain_date_optional_integer(p, value)
            .map(|value| value)
    }

    fn make_temporal_zoned_date_time(
        &mut self,
        p: &ResidualProgram,
        new_target: Value,
        record: ZonedDateTimeRecord,
    ) -> Result<Value, JsError> {
        if record.epoch_nanoseconds.unsigned_abs() > MAX_EPOCH_NANOSECONDS as u128 {
            return Err(self.range_error(p, "Invalid epochNanoseconds".into()));
        }
        let prototype_atom = self.intern_atom("prototype");
        let prototype = self.get_property(p, new_target, prototype_atom)?;
        let prototype = if self.is_object_like(prototype) {
            prototype
        } else {
            self.object_proto
        };
        Ok(self.heap.alloc(Cell::TemporalZonedDateTime {
            object: Box::new(Self::empty_object(prototype)),
            epoch_nanoseconds: record.epoch_nanoseconds,
            time_zone: record.time_zone,
            calendar: record.calendar,
        }))
    }
}

#[derive(Clone)]
struct ZonedDateTimeRecord {
    epoch_nanoseconds: i128,
    time_zone: String,
    calendar: String,
}

fn parse_zoned_date_time_string(text: &str) -> Option<ZonedDateTimeRecord> {
    let (base, annotation_text) = text.split_once('[')?;
    let mut rest = annotation_text;
    let mut time_zone = None;
    let mut calendar = None;
    loop {
        let (annotation, tail) = rest.split_once(']')?;
        let body = annotation.strip_prefix('!').unwrap_or(annotation);
        if let Some((key, value)) = body.split_once('=') {
            if key == "u-ca" {
                calendar.get_or_insert(value);
            } else if annotation.starts_with('!') {
                return None;
            }
        } else if time_zone.replace(body).is_some() {
            return None;
        }
        if tail.is_empty() {
            break;
        }
        rest = tail.strip_prefix('[')?;
    }
    let time_zone = canonical_time_zone(time_zone?)?;
    let calendar =
        super::temporal_date_parse::parse_calendar_identifier(calendar.unwrap_or("iso8601"))?;
    let (local, offset, leap_second) = parse_iso_zoned_base(base)?;
    let mut epoch_nanoseconds = match offset {
        Some(offset) => naive_epoch_nanoseconds(local)? - offset,
        None => zoned_local_epoch(local, &time_zone)?,
    };
    if leap_second {
        epoch_nanoseconds += NANOSECONDS_PER_SECOND;
    }
    if epoch_nanoseconds.unsigned_abs() > MAX_EPOCH_NANOSECONDS as u128 {
        return None;
    }
    if offset.is_some() && timezone_offset_nanoseconds(&time_zone, epoch_nanoseconds)? != offset? {
        return None;
    }
    Some(ZonedDateTimeRecord {
        epoch_nanoseconds,
        time_zone,
        calendar,
    })
}

pub(super) fn parse_iso_zoned_base(value: &str) -> Option<(chrono::NaiveDateTime, Option<i128>, bool)> {
    let Some((date, time)) = value.split_once(['T', 't', ' ']) else {
        if value.ends_with(['Z', 'z']) {
            return None;
        }
        let date = parse_iso_zoned_date(value)?;
        return Some((date.and_hms_opt(0, 0, 0)?, None, false));
    };
    if date == "-000000" || date.starts_with("-000000-") {
        return None;
    }
    let date = parse_iso_zoned_date(date)?;
    let (time, offset_text) = if let Some(time) = time.strip_suffix(['Z', 'z']) {
        (time, Some("+00:00"))
    } else if let Some(index) = time.get(1..)?.find(['+', '-']).map(|index| index + 1) {
        (&time[..index], Some(&time[index..]))
    } else {
        (time, None)
    };
    let (clock, fraction) = time
        .split_once(['.', ','])
        .map_or((time, None), |(clock, fraction)| (clock, Some(fraction)));
    if fraction.is_some_and(|fraction| {
        fraction.is_empty()
            || fraction.len() > MAX_FRACTION_DIGITS
            || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    }) {
        return None;
    }
    let fields = if clock.contains(':') {
        let parts = clock.split(':').collect::<Vec<_>>();
        match parts.as_slice() {
            [hour, minute] if fraction.is_none() => {
                [parse_two_digits(hour)?, parse_two_digits(minute)?, 0]
            }
            [hour, minute, second] => [
                parse_two_digits(hour)?,
                parse_two_digits(minute)?,
                parse_two_digits(second)?,
            ],
            _ => return None,
        }
    } else {
        let bytes = clock.as_bytes();
        let valid_length = clock.len() == ISO_TIME_FIELD_DIGITS
            || clock.len() == ISO_TIME_FIELD_DIGITS * 2
            || clock.len() == ISO_TIME_FIELD_DIGITS * 3;
        if !bytes.iter().all(u8::is_ascii_digit)
            || !valid_length
            || fraction.is_some() && clock.len() != ISO_TIME_FIELD_DIGITS * 3
        {
            return None;
        }
        [
            clock.get(..ISO_TIME_FIELD_DIGITS)?.parse().ok()?,
            clock
                .get(ISO_TIME_FIELD_DIGITS..ISO_TIME_FIELD_DIGITS * 2)
                .filter(|_| clock.len() >= ISO_TIME_FIELD_DIGITS * 2)?
                .parse()
                .ok()
                .unwrap_or(0),
            clock
                .get(ISO_TIME_FIELD_DIGITS * 2..ISO_TIME_FIELD_DIGITS * 3)
                .filter(|_| clock.len() == ISO_TIME_FIELD_DIGITS * 3)?
                .parse()
                .ok()
                .unwrap_or(0),
        ]
    };
    let [hour, minute, second] = fields;
    if hour > ISO_HOUR_LIMIT || minute > ISO_MINUTE_LIMIT || second > ISO_LEAP_SECOND {
        return None;
    }
    let leap_second = second == ISO_LEAP_SECOND;
    let second = second.min(ISO_SECOND_LIMIT);
    let nanosecond = fraction.map_or(Some(0), parse_fraction_nanoseconds)?;
    let local = date.and_hms_nano_opt(hour, minute, second, nanosecond)?;
    let offset = match offset_text {
        Some(value) => Some(parse_offset_nanoseconds(value)?),
        None => None,
    };
    Some((local, offset, leap_second))
}

fn parse_iso_zoned_date(value: &str) -> Option<chrono::NaiveDate> {
    let (year_text, month_text, day_text) = if value.starts_with(['+', '-']) {
        let sign = &value[..1];
        let body = value.get(1..)?;
        let year = body.get(..EXTENDED_YEAR_DIGITS)?;
        let remainder = body.get(EXTENDED_YEAR_DIGITS..)?;
        let (month, day) = parse_month_day(remainder)?;
        let year = format!("{sign}{year}");
        return chrono::NaiveDate::from_ymd_opt(
            year.parse().ok()?,
            month.parse().ok()?,
            day.parse().ok()?,
        );
    } else if value.contains('-') {
        let (year, remainder) = value.split_once('-')?;
        if year.len() != ISO_YEAR_DIGITS || !year.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let (month, day) = parse_extended_month_day(remainder)?;
        (year, month, day)
    } else {
        if value.len() != ISO_YEAR_DIGITS + ISO_MONTH_DIGITS + ISO_DAY_DIGITS {
            return None;
        }
        (
            value.get(..ISO_YEAR_DIGITS)?,
            value.get(ISO_YEAR_DIGITS..ISO_YEAR_DIGITS + ISO_MONTH_DIGITS)?,
            value.get(ISO_YEAR_DIGITS + ISO_MONTH_DIGITS..)?,
        )
    };
    chrono::NaiveDate::from_ymd_opt(
        year_text.parse().ok()?,
        month_text.parse().ok()?,
        day_text.parse().ok()?,
    )
}

fn parse_month_day(value: &str) -> Option<(&str, &str)> {
    if value.contains('-') {
        parse_extended_month_day(value)
    } else {
        (value.len() == ISO_MONTH_DIGITS + ISO_DAY_DIGITS).then_some((
            value.get(..ISO_MONTH_DIGITS)?,
            value.get(ISO_MONTH_DIGITS..)?,
        ))
    }
}

fn parse_extended_month_day(value: &str) -> Option<(&str, &str)> {
    let value = value.strip_prefix('-').unwrap_or(value);
    let (month, day) = value.split_once('-')?;
    (month.len() == ISO_MONTH_DIGITS && day.len() == ISO_DAY_DIGITS).then_some((month, day))
}

fn parse_two_digits(value: &str) -> Option<u32> {
    (value.len() == ISO_TIME_FIELD_DIGITS && value.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| value.parse().ok())?
}

fn parse_fraction_nanoseconds(value: &str) -> Option<u32> {
    let mut digits = value.to_owned();
    digits.extend(std::iter::repeat_n('0', MAX_FRACTION_DIGITS - value.len()));
    digits.parse().ok()
}

fn parse_offset_nanoseconds(value: &str) -> Option<i128> {
    if !quench_temporal::valid_date_time_offset(value) {
        return None;
    }
    let sign = if value.starts_with('-') { -1 } else { 1 };
    let (clock, fraction) = value
        .split_once(['.', ','])
        .map_or((value, None), |(clock, fraction)| (clock, Some(fraction)));
    let fraction = fraction
        .map(parse_fraction_nanoseconds)
        .unwrap_or(Some(0))?;
    Some(
        i128::from(quench_temporal::offset_seconds(clock)) * NANOSECONDS_PER_SECOND
            + i128::from(sign) * i128::from(fraction),
    )
}

fn naive_epoch_nanoseconds(value: chrono::NaiveDateTime) -> Option<i128> {
    let utc = value.and_utc();
    Some(
        i128::from(utc.timestamp()) * NANOSECONDS_PER_SECOND
            + i128::from(utc.timestamp_subsec_nanos()),
    )
}

fn timezone_offset_nanoseconds(zone: &str, epoch: i128) -> Option<i128> {
    if zone.starts_with(['+', '-']) {
        return Some(i128::from(quench_temporal::offset_seconds(zone)) * NANOSECONDS_PER_SECOND);
    }
    let seconds = i64::try_from(epoch.div_euclid(NANOSECONDS_PER_SECOND)).ok()?;
    let nanos = epoch.rem_euclid(NANOSECONDS_PER_SECOND) as u32;
    let utc = Utc.timestamp_opt(seconds, nanos).single()?;
    let offset = zone
        .parse::<chrono_tz::Tz>()
        .ok()?
        .offset_from_utc_datetime(&utc.naive_utc())
        .fix()
        .local_minus_utc();
    Some(i128::from(offset) * NANOSECONDS_PER_SECOND)
}

fn zoned_date_time_rounding_quantum(options: &ZonedDateTimeStringOptions) -> Option<i128> {
    if let Some(unit) = options.smallest_unit.as_deref() {
        return match unit {
            "hour" => Some(NANOSECONDS_PER_HOUR),
            "minute" => Some(NANOSECONDS_PER_MINUTE),
            "second" => Some(NANOSECONDS_PER_SECOND),
            "millisecond" => Some(NANOSECONDS_PER_MILLISECOND as i128),
            "microsecond" => Some(NANOSECONDS_PER_MICROSECOND as i128),
            "nanosecond" => Some(1),
            _ => None,
        };
    }
    options
        .fractional_second_digits
        .map(|digits| DECIMAL_RADIX_I128.pow((MAX_FRACTION_DIGITS - digits) as u32))
}

fn round_zoned_date_time_epoch(
    epoch: i128,
    time_zone: &str,
    options: &ZonedDateTimeStringOptions,
) -> Option<i128> {
    let Some(quantum) = zoned_date_time_rounding_quantum(options) else {
        return Some(epoch);
    };
    let fields = zoned_date_time_fields(epoch, time_zone)?;
    let date = chrono::NaiveDate::from_ymd_opt(fields[0], fields[1] as u32, fields[2] as u32)?;
    let nanoseconds = i128::from(fields[3]) * NANOSECONDS_PER_HOUR
        + i128::from(fields[4]) * NANOSECONDS_PER_MINUTE
        + i128::from(fields[5]) * NANOSECONDS_PER_SECOND
        + i128::from(fields[6]) * i128::from(NANOSECONDS_PER_MILLISECOND)
        + i128::from(fields[7]) * i128::from(NANOSECONDS_PER_MICROSECOND)
        + i128::from(fields[8]);
    let rounded = round_temporal_nanoseconds(nanoseconds, quantum, &options.rounding_mode) * quantum;
    let date = if rounded >= NANOSECONDS_PER_DAY {
        date.succ_opt()?
    } else {
        date
    };
    let nanoseconds = rounded % NANOSECONDS_PER_DAY;
    let hour = nanoseconds / NANOSECONDS_PER_HOUR;
    let minute = nanoseconds / NANOSECONDS_PER_MINUTE % i128::from(SECONDS_PER_MINUTE);
    let second = nanoseconds / NANOSECONDS_PER_SECOND % i128::from(SECONDS_PER_MINUTE);
    let subsecond = nanoseconds % NANOSECONDS_PER_SECOND;
    let local = date.and_hms_nano_opt(
        u32::try_from(hour).ok()?,
        u32::try_from(minute).ok()?,
        u32::try_from(second).ok()?,
        u32::try_from(subsecond).ok()?,
    )?;
    zoned_local_epoch(local, time_zone)
}

fn normalize_smallest_unit(unit: &str) -> &str {
    unit.strip_suffix('s')
        .filter(|singular| SMALLEST_UNITS.contains(singular))
        .unwrap_or(unit)
}

pub(super) fn round_temporal_nanoseconds(value: i128, quantum: i128, mode: &str) -> i128 {
    let quotient = value / quantum;
    let remainder = value % quantum;
    if remainder == 0 {
        return quotient;
    }
    let sign = value.signum();
    let distance = remainder.abs();
    let tie = distance * ROUNDING_TIE_FACTOR == quantum;
    let above_tie = distance * ROUNDING_TIE_FACTOR > quantum;
    let adjust = match mode {
        "trunc" => false,
        "floor" => sign < 0,
        "ceil" => sign > 0,
        "expand" => true,
        "halfTrunc" => above_tie,
        "halfExpand" => above_tie || tie,
        "halfFloor" => above_tie || tie && sign < 0,
        "halfCeil" => above_tie || tie && sign > 0,
        "halfEven" => above_tie || tie && quotient % ROUNDING_TIE_FACTOR != 0,
        _ => false,
    };
    quotient + if adjust { sign } else { 0 }
}

fn format_fraction(nanoseconds: i32, digits: usize) -> String {
    if digits == 0 {
        return String::new();
    }
    let scale = DECIMAL_RADIX.pow((MAX_FRACTION_DIGITS - digits) as u32);
    format!(".{:0digits$}", nanoseconds as u32 / scale)
}

fn format_zoned_time(fields: &[i32; 9], fraction: &str, smallest_unit: Option<&str>) -> String {
    let hour = format!("{:02}", fields[3]);
    match smallest_unit {
        Some("hour") => hour,
        Some("minute") => format!("{hour}:{:02}", fields[4]),
        _ => format!("{hour}:{:02}:{:02}{fraction}", fields[4], fields[5]),
    }
}

fn format_calendar_annotation(calendar: &str, calendar_name: &str) -> String {
    match calendar_name {
        "never" => String::new(),
        "auto" if calendar == "iso8601" => String::new(),
        "critical" => format!("[!u-ca={calendar}]"),
        _ => format!("[u-ca={calendar}]"),
    }
}

fn format_time_zone_annotation(time_zone: &str, time_zone_name: &str) -> String {
    match time_zone_name {
        "never" => String::new(),
        "critical" => format!("[!{time_zone}]"),
        _ => format!("[{time_zone}]"),
    }
}

fn format_iso_year(year: i32) -> String {
    if (0..=MAX_BASIC_ISO_YEAR).contains(&year) {
        format!("{year:0width$}", width = ISO_YEAR_DIGITS)
    } else if year < 0 {
        format!(
            "-{year_abs:0width$}",
            year_abs = year.unsigned_abs(),
            width = EXTENDED_YEAR_DIGITS
        )
    } else {
        format!("+{year:0width$}", width = EXTENDED_YEAR_DIGITS)
    }
}

fn format_offset_nanoseconds(offset: i128) -> String {
    let sign = if offset < 0 { '-' } else { '+' };
    let seconds = offset.unsigned_abs() / NANOSECONDS_PER_SECOND as u128;
    let hours = seconds / SECONDS_PER_HOUR as u128;
    let minutes = seconds / SECONDS_PER_MINUTE as u128 % SECONDS_PER_MINUTE as u128;
    let seconds = seconds % SECONDS_PER_MINUTE as u128;
    if seconds == 0 {
        format!("{sign}{hours:02}:{minutes:02}")
    } else {
        format!("{sign}{hours:02}:{minutes:02}:{seconds:02}")
    }
}

fn canonical_time_zone(value: &str) -> Option<String> {
    if value.eq_ignore_ascii_case("utc") {
        return Some("UTC".into());
    }
    if value.starts_with(['+', '-']) {
        if !valid_time_zone_offset(value) {
            return None;
        }
        let seconds = quench_temporal::offset_seconds(value);
        let sign = if seconds < 0 { '-' } else { '+' };
        let seconds = seconds.unsigned_abs();
        let hours = seconds / SECONDS_PER_HOUR as u32;
        let minutes = seconds / SECONDS_PER_MINUTE as u32 % SECONDS_PER_MINUTE as u32;
        return Some(format!("{sign}{hours:02}:{minutes:02}"));
    }
    value
        .parse::<chrono_tz::Tz>()
        .ok()
        .map(|zone| zone.to_string())
}

fn valid_time_zone_offset(value: &str) -> bool {
    if quench_temporal::valid_timezone_offset(value) {
        return true;
    }
    value.strip_prefix(['+', '-']).is_some_and(|hours| {
        hours.len() == ISO_TIME_FIELD_DIGITS
            && hours.bytes().all(|byte| byte.is_ascii_digit())
            && quench_temporal::valid_date_time_offset(value)
    })
}

fn time_zone_from_datetime_identifier(value: &str) -> Option<String> {
    let (date_time, annotations) = value
        .split_once('[')
        .map_or((value, None), |(base, rest)| (base, Some(rest)));
    if !date_time.contains(['T', 't', ' ']) {
        return None;
    }
    if let Some(annotations) = annotations {
        let annotation = annotations.split(']').next()?;
        let zone = annotation.strip_prefix('!').unwrap_or(annotation);
        if !zone.contains('=') {
            return canonical_time_zone(zone);
        }
    }
    if date_time.ends_with(['Z', 'z']) {
        return Some("UTC".into());
    }
    let time_start = date_time.find(['T', 't', ' '])? + 1;
    let offset_start = date_time.get(time_start..)?.find(['+', '-'])? + time_start;
    canonical_time_zone(date_time.get(offset_start..)?)
}

fn zoned_local_epoch(local: chrono::NaiveDateTime, zone: &str) -> Option<i128> {
    let instant = if zone.starts_with(['+', '-']) {
        let offset_seconds = quench_temporal::offset_seconds(zone);
        let utc = local
            .and_utc()
            .checked_sub_signed(Duration::seconds(i64::from(offset_seconds)))?;
        i128::from(utc.timestamp()) * NANOSECONDS_PER_SECOND
            + i128::from(utc.timestamp_subsec_nanos())
    } else {
        let zone = zone.parse::<chrono_tz::Tz>().ok()?;
        let instant = match zone.from_local_datetime(&local) {
            chrono::LocalResult::Single(instant) => instant,
            chrono::LocalResult::Ambiguous(first, second) => {
                if first.timestamp() <= second.timestamp() {
                    first
                } else {
                    second
                }
            }
            chrono::LocalResult::None => return None,
        };
        i128::from(instant.timestamp()) * NANOSECONDS_PER_SECOND
            + i128::from(instant.timestamp_subsec_nanos())
    };
    (instant.unsigned_abs() <= MAX_EPOCH_NANOSECONDS as u128).then_some(instant)
}

fn zoned_date_time_fields(epoch: i128, zone: &str) -> Option<[i32; 9]> {
    let seconds = epoch.div_euclid(NANOSECONDS_PER_SECOND);
    let nanoseconds = epoch.rem_euclid(NANOSECONDS_PER_SECOND) as u32;
    let utc = Utc
        .timestamp_opt(i64::try_from(seconds).ok()?, nanoseconds)
        .single()?;
    let offset = if zone.starts_with(['+', '-']) {
        quench_temporal::offset_seconds(zone)
    } else {
        zone.parse::<chrono_tz::Tz>()
            .ok()?
            .offset_from_utc_datetime(&utc.naive_utc())
            .fix()
            .local_minus_utc()
    };
    let local = utc
        .naive_utc()
        .checked_add_signed(Duration::seconds(i64::from(offset)))?;
    let subsecond = local.and_utc().timestamp_subsec_nanos();
    Some([
        local.year(),
        i32::try_from(local.month()).ok()?,
        i32::try_from(local.day()).ok()?,
        i32::try_from(local.hour()).ok()?,
        i32::try_from(local.minute()).ok()?,
        i32::try_from(local.second()).ok()?,
        i32::try_from(subsecond / NANOSECONDS_PER_MILLISECOND).ok()?,
        i32::try_from(subsecond / NANOSECONDS_PER_MICROSECOND % MICROSECONDS_PER_MILLISECOND)
            .ok()?,
        i32::try_from(subsecond % NANOSECONDS_PER_MICROSECOND).ok()?,
    ])
}
