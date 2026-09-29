use super::*;
const ZERO_OFFSET_TIME_ZONES: [&str; 5] = ["UTC", "+00", "-00", "+00:00", "-00:00"];
use chrono::{Datelike, Duration, Offset, TimeZone, Timelike, Utc};
use std::cmp::Ordering;

pub(super) const MAX_EPOCH_NANOSECONDS: i128 = 8_640_000_000_000_000_000_000;
const NANOSECONDS_PER_SECOND: i128 = 1_000_000_000;
const NANOSECOND: i128 = 1;
const MIN_ROUNDING_INCREMENT: i128 = NANOSECOND;
const MAX_SUBSECOND_ROUNDING_INCREMENT: i128 = 1_000;
const NANOSECONDS_PER_MILLISECOND: u32 = 1_000_000;
const NANOSECONDS_PER_MICROSECOND: u32 = 1_000;
const MICROSECONDS_PER_MILLISECOND: u32 = 1_000;
const SECONDS_PER_HOUR: i32 = 3_600;
const SECONDS_PER_MINUTE: i32 = 60;
const ISO_YEAR_DIGITS: usize = 4;
const EXTENDED_YEAR_DIGITS: usize = 6;
const ZONED_DATE_TIME_TIME_FIELD_LIMITS: [i32; 6] = [23, 59, 59, 999, 999, 999];
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
const TIME_ZONE_TRANSITION_SEARCH_LIMIT: usize = 200_000;
const ZONED_DATE_TIME_ROUND_UNITS: [(&str, i128, i128); 7] = [
    ("day", NANOSECONDS_PER_DAY, MIN_ROUNDING_INCREMENT),
    ("hour", NANOSECONDS_PER_HOUR, HOURS_PER_DAY),
    ("minute", NANOSECONDS_PER_MINUTE, SECONDS_PER_MINUTE as i128),
    ("second", NANOSECONDS_PER_SECOND, SECONDS_PER_MINUTE as i128),
    (
        "millisecond",
        NANOSECONDS_PER_MILLISECOND as i128,
        MAX_SUBSECOND_ROUNDING_INCREMENT,
    ),
    (
        "microsecond",
        NANOSECONDS_PER_MICROSECOND as i128,
        MAX_SUBSECOND_ROUNDING_INCREMENT,
    ),
    ("nanosecond", NANOSECOND, MAX_SUBSECOND_ROUNDING_INCREMENT),
];
const ZONED_DATE_TIME_DIFFERENCE_UNITS: [&str; 10] = [
    "year",
    "month",
    "week",
    "day",
    "hour",
    "minute",
    "second",
    "millisecond",
    "microsecond",
    "nanosecond",
];
pub(super) const NANOSECONDS_PER_DAY: i128 = HOURS_PER_DAY * NANOSECONDS_PER_HOUR;
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

struct ZonedDateTimeDifferenceOptions {
    largest: &'static str,
    smallest: &'static str,
    increment: i128,
    rounding_mode: String,
}

struct ZonedDateTimeOptions {
    disambiguation: String,
    offset: String,
    overflow: String,
}

impl Default for ZonedDateTimeOptions {
    fn default() -> Self {
        Self {
            disambiguation: "compatible".into(),
            offset: "reject".into(),
            overflow: "constrain".into(),
        }
    }
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
pub(super) const DISAMBIGUATION_OPTIONS: [&str; 4] = ["compatible", "earlier", "later", "reject"];
const OFFSET_OPTIONS: [&str; 4] = ["prefer", "use", "ignore", "reject"];
const OVERFLOW_OPTIONS: [&str; 2] = ["constrain", "reject"];

const ZONED_DATE_TIME_GETTERS: [(&str, Native); 28] = [
    (
        "epochNanoseconds",
        Native::TemporalZonedDateTimeEpochNanosecondsGetter,
    ),
    (
        "epochMilliseconds",
        Native::TemporalZonedDateTimeEpochMillisecondsGetter,
    ),
    ("timeZoneId", Native::TemporalZonedDateTimeTimeZoneIdGetter),
    ("offset", Native::TemporalZonedDateTimeOffsetGetter),
    (
        "offsetNanoseconds",
        Native::TemporalZonedDateTimeOffsetNanosecondsGetter,
    ),
    ("calendarId", Native::TemporalZonedDateTimeCalendarIdGetter),
    ("year", Native::TemporalZonedDateTimeYearGetter),
    ("month", Native::TemporalZonedDateTimeMonthGetter),
    ("monthCode", Native::TemporalZonedDateTimeMonthCodeGetter),
    ("day", Native::TemporalZonedDateTimeDayGetter),
    ("era", Native::TemporalZonedDateTimeEraGetter),
    ("eraYear", Native::TemporalZonedDateTimeEraYearGetter),
    ("dayOfWeek", Native::TemporalZonedDateTimeDayOfWeekGetter),
    ("dayOfYear", Native::TemporalZonedDateTimeDayOfYearGetter),
    ("weekOfYear", Native::TemporalZonedDateTimeWeekOfYearGetter),
    ("yearOfWeek", Native::TemporalZonedDateTimeYearOfWeekGetter),
    ("daysInWeek", Native::TemporalZonedDateTimeDaysInWeekGetter),
    (
        "daysInMonth",
        Native::TemporalZonedDateTimeDaysInMonthGetter,
    ),
    ("daysInYear", Native::TemporalZonedDateTimeDaysInYearGetter),
    (
        "monthsInYear",
        Native::TemporalZonedDateTimeMonthsInYearGetter,
    ),
    ("inLeapYear", Native::TemporalZonedDateTimeInLeapYearGetter),
    ("hoursInDay", Native::TemporalZonedDateTimeHoursInDayGetter),
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
const ZONED_DATE_TIME_METHODS: &[(&str, Native)] = &[
    ("equals", Native::TemporalZonedDateTimeEquals),
    ("with", Native::TemporalZonedDateTimeWith),
    ("withCalendar", Native::TemporalZonedDateTimeWithCalendar),
    ("withPlainTime", Native::TemporalZonedDateTimeWithPlainTime),
    ("withTimeZone", Native::TemporalZonedDateTimeWithTimeZone),
    ("add", Native::TemporalZonedDateTimeAdd),
    ("subtract", Native::TemporalZonedDateTimeSubtract),
    (
        "getTimeZoneTransition",
        Native::TemporalZonedDateTimeGetTimeZoneTransition,
    ),
    ("startOfDay", Native::TemporalZonedDateTimeStartOfDay),
    ("round", Native::TemporalZonedDateTimeRound),
    ("until", Native::TemporalZonedDateTimeUntil),
    ("since", Native::TemporalZonedDateTimeSince),
    ("toInstant", Native::TemporalZonedDateTimeToInstant),
    ("toPlainDate", Native::TemporalZonedDateTimeToPlainDate),
    (
        "toPlainDateTime",
        Native::TemporalZonedDateTimeToPlainDateTime,
    ),
    ("toPlainTime", Native::TemporalZonedDateTimeToPlainTime),
    ("toString", Native::TemporalZonedDateTimeToString),
    (
        "toLocaleString",
        Native::TemporalZonedDateTimeToLocaleString,
    ),
    ("toJSON", Native::TemporalZonedDateTimeToJSON),
    ("valueOf", Native::TemporalZonedDateTimeValueOf),
];

impl<H: Host> Vm<H> {
    pub(super) fn temporal_instant_to_zoned_date_time_iso(
        &mut self,
        p: &ResidualProgram,
        epoch_nanoseconds: i128,
        time_zone_value: Value,
    ) -> Result<Value, JsError> {
        let time_zone = self.temporal_timezone_id(p, time_zone_value)?;
        let temporal_atom = self.intern_atom("Temporal");
        let temporal = self.get_property(p, self.realm.globals, temporal_atom)?;
        let constructor_atom = self.intern_atom("ZonedDateTime");
        let constructor = self.get_property(p, temporal, constructor_atom)?;
        self.make_temporal_zoned_date_time(
            p,
            constructor,
            ZonedDateTimeRecord {
                epoch_nanoseconds,
                time_zone,
                calendar: "iso8601".into(),
            },
        )
    }

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
        for (name, native) in ZONED_DATE_TIME_METHODS.iter().copied() {
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
        if self.is_string(timezone_value) {
            let timezone = self.to_string(p, timezone_value)?.to_string();
            if time_zone_from_datetime_identifier(&timezone).is_some() {
                return Err(self.range_error(p, "Invalid time zone".into()));
            }
        }
        let timezone = self.temporal_timezone_id(p, timezone_value)?;
        let calendar_value = args.get(2).copied().unwrap_or(Value::UNDEFINED);
        let calendar = if calendar_value.is_undefined() {
            "iso8601".to_owned()
        } else if matches!(self.heap.get(calendar_value), Some(Cell::String(_))) {
            let text = self.to_string(p, calendar_value)?.to_string();
            super::temporal_date_parse::parse_calendar_identifier_name(&text)
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

    pub(super) fn temporal_timezone_id(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<String, JsError> {
        if let Some(Cell::TemporalZonedDateTime { time_zone, .. }) = self.heap.get(value) {
            return Ok(time_zone.clone());
        }
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

    fn temporal_zoned_date_time_intrinsic_constructor(
        &mut self,
        p: &ResidualProgram,
    ) -> Result<Value, JsError> {
        let temporal_atom = self.intern_atom("Temporal");
        let temporal = self.get_property(p, self.realm.globals, temporal_atom)?;
        let constructor_atom = self.intern_atom("ZonedDateTime");
        self.get_property(p, temporal, constructor_atom)
    }

    pub(super) fn temporal_zoned_date_time_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if matches!(
            native,
            Native::TemporalZonedDateTimeAdd | Native::TemporalZonedDateTimeSubtract
        ) {
            return self.temporal_zoned_date_time_arithmetic(p, native, this, args);
        }
        if native == Native::TemporalZonedDateTimeGetTimeZoneTransition {
            return self.temporal_zoned_date_time_transition(p, this, args);
        }
        if native == Native::TemporalZonedDateTimeStartOfDay {
            return self.temporal_zoned_date_time_start_of_day(p, this);
        }
        if native == Native::TemporalZonedDateTimeRound {
            return self.temporal_zoned_date_time_round(p, this, args);
        }
        if matches!(
            native,
            Native::TemporalZonedDateTimeUntil | Native::TemporalZonedDateTimeSince
        ) {
            return self.temporal_zoned_date_time_difference(p, native, this, args);
        }
        if native == Native::TemporalZonedDateTimeFrom {
            let options = args.get(1).copied().unwrap_or(Value::UNDEFINED);
            let input = self.temporal_zoned_date_time_record(
                p,
                args.first().copied().unwrap_or(Value::UNDEFINED),
                options,
            )?;
            let temporal_atom = self.intern_atom("Temporal");
            let temporal = self.get_property(p, self.realm.globals, temporal_atom)?;
            let constructor_atom = self.intern_atom("ZonedDateTime");
            let constructor = self.get_property(p, temporal, constructor_atom)?;
            return self.make_temporal_zoned_date_time(p, constructor, input);
        }
        if native == Native::TemporalZonedDateTimeCompare {
            let left = self.temporal_zoned_date_time_compare_record(
                p,
                args.first().copied().unwrap_or(Value::UNDEFINED),
            )?;
            let right = self.temporal_zoned_date_time_compare_record(
                p,
                args.get(1).copied().unwrap_or(Value::UNDEFINED),
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
        if native == Native::TemporalZonedDateTimeWith {
            return self.temporal_zoned_date_time_with(p, this, args);
        }
        if native == Native::TemporalZonedDateTimeWithCalendar {
            return self.temporal_zoned_date_time_with_calendar(p, this, args);
        }
        if native == Native::TemporalZonedDateTimeWithPlainTime {
            return self.temporal_zoned_date_time_with_plain_time(p, this, args);
        }
        if native == Native::TemporalZonedDateTimeWithTimeZone {
            let Some(Cell::TemporalZonedDateTime {
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
            let zone = args.first().copied().unwrap_or(Value::UNDEFINED);
            let time_zone = self.temporal_timezone_id(p, zone)?;
            let constructor = self.temporal_zoned_date_time_intrinsic_constructor(p)?;
            return self.make_temporal_zoned_date_time(
                p,
                constructor,
                ZonedDateTimeRecord {
                    epoch_nanoseconds,
                    time_zone,
                    calendar,
                },
            );
        }
        if native == Native::TemporalZonedDateTimeToLocaleString {
            return self.temporal_to_locale_string(p, this, args);
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
        if matches!(
            native,
            Native::TemporalZonedDateTimeToInstant
                | Native::TemporalZonedDateTimeToPlainDate
                | Native::TemporalZonedDateTimeToPlainDateTime
                | Native::TemporalZonedDateTimeToPlainTime
                | Native::TemporalZonedDateTimeValueOf
        ) {
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
            let epoch_nanoseconds = *epoch_nanoseconds;
            let time_zone = time_zone.clone();
            let calendar = calendar.clone();
            if native == Native::TemporalZonedDateTimeValueOf {
                return Err(self.type_error(p, "Cannot convert ZonedDateTime to a number".into()));
            }
            if native == Native::TemporalZonedDateTimeToInstant {
                let constructor = self.temporal_instant_constructor(p)?;
                return self.make_temporal_instant(p, epoch_nanoseconds, constructor);
            }
            let fields = zoned_date_time_fields(epoch_nanoseconds, &time_zone)
                .ok_or_else(|| self.range_error(p, "Invalid epochNanoseconds".into()))?;
            if native == Native::TemporalZonedDateTimeToPlainDate {
                let constructor = self.temporal_plain_date_constructor(p)?;
                let date = super::temporal_date::checked_iso_date(fields[0], fields[1], fields[2])
                    .ok_or_else(|| self.range_error(p, "Invalid PlainDate".into()))?;
                return self.make_temporal_plain_date(p, date, calendar, constructor);
            }
            if native == Native::TemporalZonedDateTimeToPlainTime {
                let time = [
                    fields[3], fields[4], fields[5], fields[6], fields[7], fields[8],
                ];
                return self.temporal_plain_time_object(time);
            }
            let constructor = self.temporal_plain_date_time_constructor(p)?;
            let args = [
                Value::number(f64::from(fields[0])),
                Value::number(f64::from(fields[1])),
                Value::number(f64::from(fields[2])),
                Value::number(f64::from(fields[3])),
                Value::number(f64::from(fields[4])),
                Value::number(f64::from(fields[5])),
                Value::number(f64::from(fields[6])),
                Value::number(f64::from(fields[7])),
                Value::number(f64::from(fields[8])),
                self.heap.alloc(Cell::String(calendar.into())),
            ];
            return self.temporal_plain_date_time_construct(p, &args, constructor);
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
            Native::TemporalZonedDateTimeEpochMillisecondsGetter => Ok(Value::number(
                epoch.div_euclid(i128::from(NANOSECONDS_PER_MILLISECOND)) as f64,
            )),
            Native::TemporalZonedDateTimeHoursInDayGetter => {
                let (start, next_start) =
                    zoned_date_time_day_bounds(epoch, &zone).ok_or_else(|| {
                        self.range_error(p, "ZonedDateTime day boundary is out of range".into())
                    })?;
                if next_start.unsigned_abs() > MAX_EPOCH_NANOSECONDS as u128 {
                    return Err(
                        self.range_error(p, "ZonedDateTime day boundary is out of range".into())
                    );
                }
                Ok(Value::number(
                    (next_start - start) as f64 / NANOSECONDS_PER_HOUR as f64,
                ))
            }
            Native::TemporalZonedDateTimeOffsetGetter
            | Native::TemporalZonedDateTimeOffsetNanosecondsGetter => {
                let offset = timezone_offset_nanoseconds(&zone, epoch)
                    .ok_or_else(|| self.range_error(p, "Invalid time-zone offset".into()))?;
                if native == Native::TemporalZonedDateTimeOffsetGetter {
                    Ok(self
                        .heap
                        .alloc(Cell::String(format_offset_nanoseconds(offset).into())))
                } else {
                    Ok(Value::number(offset as f64))
                }
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
                let calendar_fields = quench_intl::calendar_fields_from_iso(
                    fields[0],
                    fields[1] as u32,
                    fields[2] as u32,
                    &calendar,
                );
                if native == Native::TemporalZonedDateTimeMonthCodeGetter {
                    return Ok(self.heap.alloc(Cell::String(calendar_fields.map_or_else(
                        || format!("M{:0width$}", fields[1], width = ISO_MONTH_DIGITS),
                        |fields| fields.month_code,
                    ).into())));
                }
                let value = match native {
                    Native::TemporalZonedDateTimeYearGetter => calendar_fields.as_ref().map_or(fields[0], |fields| fields.year),
                    Native::TemporalZonedDateTimeMonthGetter => calendar_fields.as_ref().map_or(fields[1], |fields| fields.month as i32),
                    Native::TemporalZonedDateTimeDayGetter => calendar_fields.as_ref().map_or(fields[2], |fields| fields.day as i32),
                    Native::TemporalZonedDateTimeHourGetter => fields[3],
                    Native::TemporalZonedDateTimeMinuteGetter => fields[4],
                    Native::TemporalZonedDateTimeSecondGetter => fields[5],
                    Native::TemporalZonedDateTimeMillisecondGetter => fields[6],
                    Native::TemporalZonedDateTimeMicrosecondGetter => fields[7],
                    Native::TemporalZonedDateTimeNanosecondGetter => fields[8],
                    Native::TemporalZonedDateTimeEraGetter
                    | Native::TemporalZonedDateTimeEraYearGetter => {
                        return Ok(match native {
                            Native::TemporalZonedDateTimeEraGetter => calendar_fields
                                .and_then(|fields| fields.era)
                                .map_or(Value::UNDEFINED, |era| self.heap.alloc(Cell::String(era.into()))),
                            _ => calendar_fields
                                .and_then(|fields| fields.era_year)
                                .map_or(Value::UNDEFINED, |year| Value::number(f64::from(year))),
                        });
                    }
                    Native::TemporalZonedDateTimeDayOfWeekGetter => i32::try_from(
                        super::temporal_date::iso_day_of_week(super::temporal_date::IsoDate {
                            year: fields[0],
                            month: fields[1] as u32,
                            day: fields[2] as u32,
                        }),
                    )
                    .unwrap_or_default(),
                    Native::TemporalZonedDateTimeDayOfYearGetter => i32::try_from(
                        calendar_fields.as_ref().map_or_else(
                            || {
                                super::temporal_date::iso_day_of_year(
                                    super::temporal_date::IsoDate {
                                        year: fields[0],
                                        month: fields[1] as u32,
                                        day: fields[2] as u32,
                                    },
                                )
                            },
                            |fields| fields.day_of_year,
                        ),
                    )
                    .unwrap_or_default(),
                    Native::TemporalZonedDateTimeWeekOfYearGetter => {
                        return Ok(super::temporal_date::temporal_iso_week(
                            super::temporal_date::IsoDate {
                                year: fields[0],
                                month: fields[1] as u32,
                                day: fields[2] as u32,
                            },
                            &calendar,
                        )
                        .map_or(Value::UNDEFINED, |(week, _)| Value::number(f64::from(week))));
                    }
                    Native::TemporalZonedDateTimeYearOfWeekGetter => {
                        return Ok(super::temporal_date::temporal_iso_week(
                            super::temporal_date::IsoDate {
                                year: fields[0],
                                month: fields[1] as u32,
                                day: fields[2] as u32,
                            },
                            &calendar,
                        )
                        .map_or(Value::UNDEFINED, |(_, year)| Value::number(f64::from(year))));
                    }
                    Native::TemporalZonedDateTimeDaysInWeekGetter => {
                        super::temporal_date::ISO_DAYS_PER_WEEK as i32
                    }
                    Native::TemporalZonedDateTimeDaysInMonthGetter => {
                        calendar_fields.as_ref().map_or_else(
                            || super::temporal_date::iso_days_in_month(fields[0], fields[1]).unwrap_or_default() as i32,
                            |fields| fields.days_in_month as i32,
                        )
                    }
                    Native::TemporalZonedDateTimeDaysInYearGetter => {
                        calendar_fields.as_ref().map_or_else(
                            || super::temporal_date::iso_days_in_year(fields[0]) as i32,
                            |fields| fields.days_in_year as i32,
                        )
                    }
                    Native::TemporalZonedDateTimeMonthsInYearGetter => {
                        calendar_fields.as_ref().map_or(
                            super::temporal_date::ISO_MONTHS_PER_YEAR,
                            |fields| fields.months_in_year as i32,
                        )
                    }
                    Native::TemporalZonedDateTimeInLeapYearGetter => {
                        return Ok(if calendar_fields.map_or_else(
                            || super::temporal_date::iso_is_leap_year(fields[0]),
                            |fields| fields.is_leap_year,
                        ) {
                            Value::TRUE
                        } else {
                            Value::FALSE
                        });
                    }
                    _ => return Err(self.type_error(p, "Unsupported ZonedDateTime getter".into())),
                };
                Ok(Value::number(f64::from(value)))
            }
        }
    }

    pub(super) fn temporal_zoned_date_time_arithmetic(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
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
        let epoch_nanoseconds = *epoch_nanoseconds;
        let time_zone = time_zone.clone();
        let calendar = calendar.clone();
        let mut duration =
            self.duration_record(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let options = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let constrain = self.plain_date_overflow(p, options)?;
        self.validate_duration_fields(p, &duration)?;
        if native == Native::TemporalZonedDateTimeSubtract {
            duration.iter_mut().for_each(|field| *field = -*field);
        }
        let fields = zoned_date_time_fields(epoch_nanoseconds, &time_zone)
            .ok_or_else(|| self.range_error(p, "Invalid epochNanoseconds".into()))?;
        let date_time_constructor = self.temporal_plain_date_time_constructor(p)?;
        let date_time_args = [
            Value::number(f64::from(fields[0])),
            Value::number(f64::from(fields[1])),
            Value::number(f64::from(fields[2])),
            Value::number(f64::from(fields[3])),
            Value::number(f64::from(fields[4])),
            Value::number(f64::from(fields[5])),
            Value::number(f64::from(fields[6])),
            Value::number(f64::from(fields[7])),
            Value::number(f64::from(fields[8])),
            self.heap.alloc(Cell::String(calendar.clone().into())),
        ];
        let local_date_time =
            self.temporal_plain_date_time_construct(p, &date_time_args, date_time_constructor)?;
        let mut date_duration = duration;
        date_duration[super::temporal_date_arithmetic::DURATION_HOURS_FIELD..].fill(0.0);
        let date_duration_args = date_duration.map(Value::number);
        let date_duration = self.temporal_duration_construct(p, &date_duration_args)?;
        let internal_options = self.object();
        let overflow_key = self.intern_atom("overflow");
        let overflow = if constrain { "constrain" } else { "reject" };
        let overflow = self.heap.alloc(Cell::String(overflow.into()));
        self.set_property(internal_options, overflow_key, overflow)?;
        let local_date_time = self.temporal_plain_date_time_arithmetic(
            p,
            Native::TemporalPlainDateTimeAdd,
            local_date_time,
            &[date_duration, internal_options],
        )?;
        let (date, time, _) = self.temporal_plain_date_time_slots(p, local_date_time)?;
        let date_epoch = zoned_local_epoch_from_iso_fields(date, time, &time_zone, "compatible")
            .ok_or_else(|| self.range_error(p, "Invalid local date-time".into()))?;
        let time_delta = duration[super::temporal_date_arithmetic::DURATION_HOURS_FIELD..]
            .iter()
            .zip(super::temporal_date_arithmetic::TIME_UNIT_NANOSECOND_SCALES)
            .map(|(value, scale)| *value as i128 * scale)
            .sum::<i128>();
        let epoch_nanoseconds = date_epoch + time_delta;
        let temporal_key = self.intern_atom("Temporal");
        let temporal = self.get_property(p, self.realm.globals, temporal_key)?;
        let constructor_key = self.intern_atom("ZonedDateTime");
        let constructor = self.get_property(p, temporal, constructor_key)?;
        self.make_temporal_zoned_date_time(
            p,
            constructor,
            ZonedDateTimeRecord {
                epoch_nanoseconds,
                time_zone,
                calendar,
            },
        )
    }

    fn temporal_zoned_date_time_transition(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
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
        let epoch_nanoseconds = *epoch_nanoseconds;
        let time_zone = time_zone.clone();
        let calendar = calendar.clone();
        let options = args
            .first()
            .copied()
            .ok_or_else(|| self.type_error(p, "Missing options".into()))?;
        if matches!(self.heap.get(options), Some(Cell::Symbol(_))) {
            return Err(self.type_error(p, "Invalid options".into()));
        }
        let direction = if let Some(Cell::String(value)) = self.heap.get(options) {
            value.to_string()
        } else {
            if !self.is_object_like(options) {
                return Err(self.type_error(p, "Invalid options".into()));
            }
            let direction_atom = self.intern_atom("direction");
            let direction = self.get_property(p, options, direction_atom)?;
            if matches!(self.heap.get(direction), Some(Cell::Symbol(_))) {
                return Err(self.type_error(p, "Invalid direction".into()));
            }
            self.to_string(p, direction)?.to_string()
        };
        if direction != "next" && direction != "previous" {
            return Err(self.range_error(p, "Invalid direction".into()));
        }
        let Some(transition) = find_time_zone_transition(&time_zone, epoch_nanoseconds, &direction)
        else {
            return Ok(Value::NULL);
        };
        let temporal_atom = self.intern_atom("Temporal");
        let temporal = self.get_property(p, self.realm.globals, temporal_atom)?;
        let constructor_atom = self.intern_atom("ZonedDateTime");
        let constructor = self.get_property(p, temporal, constructor_atom)?;
        self.make_temporal_zoned_date_time(
            p,
            constructor,
            ZonedDateTimeRecord {
                epoch_nanoseconds: transition,
                time_zone,
                calendar,
            },
        )
    }

    fn temporal_zoned_date_time_start_of_day(
        &mut self,
        p: &ResidualProgram,
        this: Value,
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
        let epoch_nanoseconds = *epoch_nanoseconds;
        let time_zone = time_zone.clone();
        let calendar = calendar.clone();
        let at_epoch_limit = epoch_nanoseconds.unsigned_abs() >= MAX_EPOCH_NANOSECONDS as u128;
        if at_epoch_limit && !ZERO_OFFSET_TIME_ZONES.contains(&time_zone.as_str()) {
            return Err(self.range_error(p, "Invalid epochNanoseconds".into()));
        }
        if at_epoch_limit {
            return Ok(this);
        }
        let midnight = round_zoned_date_time_day(epoch_nanoseconds, &time_zone, "trunc")
            .ok_or_else(|| self.range_error(p, "Invalid epochNanoseconds".into()))?;
        let temporal_atom = self.intern_atom("Temporal");
        let temporal = self.get_property(p, self.realm.globals, temporal_atom)?;
        let constructor_atom = self.intern_atom("ZonedDateTime");
        let constructor = self.get_property(p, temporal, constructor_atom)?;
        self.make_temporal_zoned_date_time(
            p,
            constructor,
            ZonedDateTimeRecord {
                epoch_nanoseconds: midnight,
                time_zone,
                calendar,
            },
        )
    }

    fn temporal_zoned_date_time_round(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
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
        let epoch_nanoseconds = *epoch_nanoseconds;
        let time_zone = time_zone.clone();
        let calendar = calendar.clone();
        let options = args
            .first()
            .copied()
            .ok_or_else(|| self.type_error(p, "Missing rounding options".into()))?;
        if options.is_null() || matches!(self.heap.get(options), Some(Cell::Symbol(_))) {
            return Err(self.type_error(p, "Invalid rounding options".into()));
        }
        let (smallest_unit, increment, rounding_mode) =
            if let Some(Cell::String(value)) = self.heap.get(options) {
                (
                    value.to_string(),
                    MIN_ROUNDING_INCREMENT,
                    "halfExpand".to_owned(),
                )
            } else {
                if !self.is_object_like(options) {
                    return Err(self.type_error(p, "Invalid rounding options".into()));
                }
                let increment_value = self.get_option_property(p, options, "roundingIncrement")?;
                let increment = if increment_value.is_undefined() {
                    MIN_ROUNDING_INCREMENT
                } else {
                    let increment = self.to_number(p, increment_value)?;
                    if !increment.is_finite() || increment <= 0.0 {
                        return Err(self.range_error(p, "Invalid roundingIncrement".into()));
                    }
                    increment as i128
                };
                let mode_value = self.get_option_property(p, options, "roundingMode")?;
                let rounding_mode = if mode_value.is_undefined() {
                    "halfExpand".to_owned()
                } else {
                    self.to_string(p, mode_value)?.to_string()
                };
                let unit = self.get_option_property(p, options, "smallestUnit")?;
                if unit.is_undefined() {
                    return Err(self.range_error(p, "smallestUnit required".into()));
                }
                (
                    self.to_string(p, unit)?.to_string(),
                    increment,
                    rounding_mode,
                )
            };
        let smallest_unit = smallest_unit.strip_suffix('s').unwrap_or(&smallest_unit);
        let Some((_, unit_nanoseconds, increment_limit)) = ZONED_DATE_TIME_ROUND_UNITS
            .iter()
            .find(|(unit, _, _)| *unit == smallest_unit)
        else {
            return Err(self.range_error(p, "Invalid smallestUnit".into()));
        };
        if !zoned_date_time_increment_is_valid(increment, *increment_limit) {
            return Err(self.range_error(p, "Invalid roundingIncrement".into()));
        }
        if smallest_unit == "day"
            && epoch_nanoseconds.unsigned_abs() >= MAX_EPOCH_NANOSECONDS as u128
        {
            return Err(self.range_error(p, "Invalid epochNanoseconds".into()));
        }
        let quantum = unit_nanoseconds
            .checked_mul(increment)
            .ok_or_else(|| self.range_error(p, "Invalid roundingIncrement".into()))?;
        let rounded = if smallest_unit == "day" {
            round_zoned_date_time_day(epoch_nanoseconds, &time_zone, &rounding_mode)
        } else {
            let offset = timezone_offset_nanoseconds(&time_zone, epoch_nanoseconds)
                .ok_or_else(|| self.range_error(p, "Invalid time zone".into()))?;
            round_zoned_local_nanoseconds(epoch_nanoseconds, offset, quantum, &rounding_mode)
        }
        .ok_or_else(|| self.range_error(p, "Invalid epochNanoseconds".into()))?;
        let temporal_atom = self.intern_atom("Temporal");
        let temporal = self.get_property(p, self.realm.globals, temporal_atom)?;
        let constructor_atom = self.intern_atom("ZonedDateTime");
        let constructor = self.get_property(p, temporal, constructor_atom)?;
        self.make_temporal_zoned_date_time(
            p,
            constructor,
            ZonedDateTimeRecord {
                epoch_nanoseconds: rounded,
                time_zone,
                calendar,
            },
        )
    }

    fn temporal_zoned_date_time_with_calendar(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let Some(Cell::TemporalZonedDateTime {
            epoch_nanoseconds,
            time_zone,
            ..
        }) = self.heap.get(this)
        else {
            return Err(self.type_error(
                p,
                "Temporal.ZonedDateTime method called on incompatible receiver".into(),
            ));
        };
        let record_epoch = *epoch_nanoseconds;
        let record_time_zone = time_zone.clone();
        let calendar_like = args.first().copied().unwrap_or(Value::UNDEFINED);
        let calendar = match self.heap.get(calendar_like) {
            Some(Cell::String(_)) => {
                let text = self.to_string(p, calendar_like)?.to_string();
                super::temporal_date_parse::calendar_identifier_from_string(&text)
                    .ok_or_else(|| self.range_error(p, "Invalid calendar".into()))?
            }
            Some(Cell::TemporalPlainDate { calendar, .. })
            | Some(Cell::TemporalPlainDateTime { calendar, .. })
            | Some(Cell::TemporalPlainMonthDay { calendar, .. })
            | Some(Cell::TemporalPlainYearMonth { calendar, .. }) => calendar.clone(),
            Some(Cell::TemporalZonedDateTime { .. }) => "iso8601".to_owned(),
            _ => return Err(self.type_error(p, "Invalid calendar".into())),
        };
        let temporal_atom = self.intern_atom("Temporal");
        let temporal = self.get_property(p, self.realm.globals, temporal_atom)?;
        let constructor_atom = self.intern_atom("ZonedDateTime");
        let constructor = self.get_property(p, temporal, constructor_atom)?;
        self.make_temporal_zoned_date_time(
            p,
            constructor,
            ZonedDateTimeRecord {
                epoch_nanoseconds: record_epoch,
                time_zone: record_time_zone,
                calendar,
            },
        )
    }

    fn temporal_zoned_date_time_with_plain_time(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
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
        let (epoch_nanoseconds, time_zone, calendar) =
            (*epoch_nanoseconds, time_zone.clone(), calendar.clone());
        let argument = args.first().copied().unwrap_or(Value::UNDEFINED);
        let epoch = if argument.is_undefined() {
            round_zoned_date_time_day(epoch_nanoseconds, &time_zone, "trunc")
                .ok_or_else(|| self.range_error(p, "Invalid epochNanoseconds".into()))?
        } else {
            if matches!(self.heap.get(argument), Some(Cell::Symbol(_))) {
                return Err(self.type_error(p, "Invalid time".into()));
            }
            if !self.is_string(argument) && !self.is_object_like(argument) {
                return Err(self.type_error(p, "Invalid time".into()));
            }
            let fields = zoned_date_time_fields(epoch_nanoseconds, &time_zone)
                .ok_or_else(|| self.range_error(p, "Invalid epochNanoseconds".into()))?;
            let time = if self.is_string(argument) {
                super::temporal_plain_date_time_conversion::parse_time_string(self, p, argument)
                    .map_err(|_| self.range_error(p, "Invalid time".into()))?
            } else {
                super::temporal_plain_date_time_conversion::to_time(self, p, argument)?
            };
            let resolved = self.make_zoned_date_time_from_local(
                p,
                super::temporal_date::IsoDate {
                    year: fields[0],
                    month: fields[1] as u32,
                    day: fields[2] as u32,
                },
                time.map(|field| field as u32),
                calendar.clone(),
                time_zone.clone(),
                "compatible",
            )?;
            let Some(Cell::TemporalZonedDateTime {
                epoch_nanoseconds, ..
            }) = self.heap.get(resolved)
            else {
                return Err(self.type_error(p, "Invalid ZonedDateTime".into()));
            };
            *epoch_nanoseconds
        };
        if epoch.unsigned_abs() > MAX_EPOCH_NANOSECONDS as u128 {
            return Err(self.range_error(p, "Invalid epochNanoseconds".into()));
        }
        let temporal_atom = self.intern_atom("Temporal");
        let temporal = self.get_property(p, self.realm.globals, temporal_atom)?;
        let constructor_atom = self.intern_atom("ZonedDateTime");
        let constructor = self.get_property(p, temporal, constructor_atom)?;
        self.make_temporal_zoned_date_time(
            p,
            constructor,
            ZonedDateTimeRecord {
                epoch_nanoseconds: epoch,
                time_zone,
                calendar,
            },
        )
    }

    fn temporal_zoned_date_time_with(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        enum FieldValue {
            Undefined,
            Number(f64),
            String(String),
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
        let (epoch_nanoseconds, time_zone, calendar) =
            (*epoch_nanoseconds, time_zone.clone(), calendar.clone());
        let partial = args.first().copied().unwrap_or(Value::UNDEFINED);
        let is_plain_time = matches!(
            self.heap.get(partial),
            Some(Cell::Object(object)) if object.proto == self.temporal_plain_time_proto
        );
        if !self.is_object_like(partial)
            || is_plain_time
            || matches!(
                self.heap.get(partial),
                Some(
                    Cell::Array { .. }
                        | Cell::TemporalPlainDate { .. }
                        | Cell::TemporalPlainDateTime { .. }
                        | Cell::TemporalPlainMonthDay { .. }
                        | Cell::TemporalPlainYearMonth { .. }
                        | Cell::TemporalZonedDateTime { .. }
                )
            )
        {
            return Err(self.type_error(p, "Invalid date-time-like".into()));
        }
        let key = self.intern_atom("calendar");
        if !self.get_property(p, partial, key)?.is_undefined() {
            return Err(self.type_error(p, "Invalid calendar".into()));
        }
        let key = self.intern_atom("timeZone");
        if !self.get_property(p, partial, key)?.is_undefined() {
            return Err(self.type_error(p, "Invalid time zone".into()));
        }

        // Preserve the old core's observable property-read ordering.
        let names = [
            "day",
            "hour",
            "microsecond",
            "millisecond",
            "minute",
            "month",
            "monthCode",
            "nanosecond",
            "offset",
            "second",
            "year",
            "era",
            "eraYear",
        ];
        let mut supplied = Vec::with_capacity(names.len());
        for name in names {
            let key = self.intern_atom(name);
            let value = self.get_property(p, partial, key)?;
            let value = if value.is_undefined() {
                FieldValue::Undefined
            } else if matches!(name, "monthCode" | "offset" | "era") {
                if name == "offset"
                    && !matches!(
                        self.heap.get(value),
                        Some(Cell::String(_) | Cell::Object(_))
                    )
                {
                    return Err(self.type_error(p, "Invalid offset".into()));
                }
                let text = self.to_string(p, value)?.to_string();
                FieldValue::String(text)
            } else {
                FieldValue::Number(self.to_number(p, value)?)
            };
            supplied.push((name, value));
        }
        let options = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let primitive_options = !options.is_undefined() && !self.is_object_like(options);
        let option = |vm: &mut Self, name: &str, allowed: &[&str], default: &str| {
            if options.is_undefined() || primitive_options {
                return Ok(default.to_owned());
            }
            let key = vm.intern_atom(name);
            let value = vm.get_property(p, options, key)?;
            if value.is_undefined() {
                return Ok(default.to_owned());
            }
            let value = vm.to_string(p, value)?.to_string();
            if !allowed.contains(&value.as_str()) {
                return Err(vm.range_error(p, "Invalid Temporal option".into()));
            }
            Ok(value)
        };
        let disambiguation = option(
            self,
            "disambiguation",
            &DISAMBIGUATION_OPTIONS,
            "compatible",
        )?;
        let offset_mode = option(self, "offset", &OFFSET_OPTIONS, "prefer")?;
        let overflow = option(self, "overflow", &OVERFLOW_OPTIONS, "constrain")?;
        if supplied
            .iter()
            .all(|(_, value)| matches!(value, FieldValue::Undefined))
        {
            return Err(self.type_error(p, "Insufficient date-time data".into()));
        }
        if let Some((_, FieldValue::String(offset))) =
            supplied.iter().find(|(name, _)| *name == "offset")
        {
            if parse_offset_nanoseconds(offset).is_none() {
                return Err(self.range_error(p, "Invalid offset".into()));
            }
        }

        let fields = zoned_date_time_fields(epoch_nanoseconds, &time_zone)
            .ok_or_else(|| self.range_error(p, "Invalid epochNanoseconds".into()))?;
        let [
            year,
            month,
            day,
            hour,
            minute,
            second,
            millisecond,
            microsecond,
            nanosecond,
        ] = fields;
        let base_date = self.heap.alloc(Cell::TemporalPlainDate {
            object: Box::new(Self::empty_object(self.temporal_plain_date_proto)),
            year,
            month: month as u32,
            day: day as u32,
            calendar: calendar.clone(),
        });
        let changes = self.object();
        let mut has_date_change = false;
        for (name, value) in &supplied {
            if matches!(
                *name,
                "year" | "month" | "monthCode" | "day" | "era" | "eraYear"
            )
                && !matches!(value, FieldValue::Undefined)
            {
                let key = self.intern_atom(name);
                let value = match value {
                    FieldValue::Number(value) => Value::number(*value),
                    FieldValue::String(value) => {
                        self.heap.alloc(Cell::String(value.clone().into()))
                    }
                    FieldValue::Undefined => Value::UNDEFINED,
                };
                self.set_property(changes, key, value)?;
                has_date_change = true;
            }
        }
        let internal_options = self.object();
        let overflow_key = self.intern_atom("overflow");
        let overflow_value = self.heap.alloc(Cell::String(overflow.clone().into()));
        self.set_property(internal_options, overflow_key, overflow_value)?;
        let date = if has_date_change {
            self.temporal_plain_date_with(p, base_date, &[changes, internal_options])?
        } else {
            base_date
        };
        let (year, month, day, calendar) = self.temporal_plain_date_slots(p, date)?;
        let mut time = [hour, minute, second, millisecond, microsecond, nanosecond];
        for (name, value) in &supplied {
            let index = match *name {
                "hour" => Some(0),
                "minute" => Some(1),
                "second" => Some(2),
                "millisecond" => Some(3),
                "microsecond" => Some(4),
                "nanosecond" => Some(5),
                _ => None,
            };
            let Some(index) = index else { continue };
            let FieldValue::Number(number) = value else {
                continue;
            };
            if !number.is_finite() {
                return Err(self.range_error(p, "Invalid time".into()));
            }
            let limit = ZONED_DATE_TIME_TIME_FIELD_LIMITS[index];
            let integer = number.trunc() as i32;
            time[index] = if overflow == "constrain" {
                integer.clamp(0, limit)
            } else if integer < 0 || integer > limit {
                return Err(self.range_error(p, "Invalid time".into()));
            } else {
                integer
            };
        }
        if primitive_options {
            return Err(self.type_error(p, "Invalid options".into()));
        }
        let date = super::temporal_date::IsoDate { year, month, day };
        let time = time.map(|field| field as u32);
        let offset = supplied
            .iter()
            .find_map(|(name, value)| match (name, value) {
                (&"offset", FieldValue::String(value)) => Some(value),
                _ => None,
            })
            .map(|value| {
                parse_offset_nanoseconds(value)
                    .ok_or_else(|| self.range_error(p, "Invalid offset".into()))
            })
            .transpose()?;
        if offset.is_none() && offset_mode != "ignore" {
            return self.make_zoned_date_time_from_local(
                p,
                date,
                time,
                calendar,
                time_zone,
                &disambiguation,
            );
        }
        let local_epoch = local_epoch_from_iso_fields(date, time);
        let epoch_nanoseconds = resolve_zoned_local_epoch(
            date,
            time,
            &time_zone,
            local_epoch,
            offset,
            false,
            &offset_mode,
            &disambiguation,
        )
        .ok_or_else(|| self.range_error(p, "Invalid ZonedDateTime".into()))?;
        let temporal_atom = self.intern_atom("Temporal");
        let temporal = self.get_property(p, self.realm.globals, temporal_atom)?;
        let constructor_atom = self.intern_atom("ZonedDateTime");
        let constructor = self.get_property(p, temporal, constructor_atom)?;
        self.make_temporal_zoned_date_time(
            p,
            constructor,
            ZonedDateTimeRecord {
                epoch_nanoseconds,
                time_zone,
                calendar,
            },
        )
    }

    fn temporal_zoned_date_time_difference(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let Some(Cell::TemporalZonedDateTime {
            epoch_nanoseconds: left_epoch,
            time_zone: left_time_zone,
            calendar: left_calendar,
            ..
        }) = self.heap.get(this)
        else {
            return Err(self.type_error(
                p,
                "Temporal.ZonedDateTime method called on incompatible receiver".into(),
            ));
        };
        let left = ZonedDateTimeRecord {
            epoch_nanoseconds: *left_epoch,
            time_zone: left_time_zone.clone(),
            calendar: left_calendar.clone(),
        };
        let other = self.temporal_zoned_date_time_compare_record(
            p,
            args.first().copied().unwrap_or(Value::UNDEFINED),
        )?;
        let options = self.temporal_zoned_date_time_difference_options(
            p,
            args.get(1).copied().unwrap_or(Value::UNDEFINED),
        )?;
        if left.calendar != other.calendar {
            return Err(self.range_error(p, "ZonedDateTime calendars do not match".into()));
        }
        if !zoned_time_zones_equivalent(&left.time_zone, &other.time_zone) {
            return Err(self.range_error(p, "ZonedDateTime time zones do not match".into()));
        }
        if zoned_date_time_unit_rank(options.largest) >= zoned_date_time_unit_rank("hour") {
            return self.zoned_date_time_time_difference(p, native, &left, &other, &options);
        }
        let date_time_constructor = self.temporal_plain_date_time_constructor(p)?;
        let left_date_time =
            self.zoned_date_time_local_plain_date_time(p, date_time_constructor, &left)?;
        let other_date_time =
            self.zoned_date_time_local_plain_date_time(p, date_time_constructor, &other)?;
        let internal_options = self.object();
        for (name, value) in [
            ("largestUnit", options.largest),
            ("smallestUnit", options.smallest),
            ("roundingMode", options.rounding_mode.as_str()),
        ] {
            let atom = self.intern_atom(name);
            let value = self.heap.alloc(Cell::String(value.into()));
            self.set_property(internal_options, atom, value)?;
        }
        let increment_atom = self.intern_atom("roundingIncrement");
        self.set_property(
            internal_options,
            increment_atom,
            Value::number(options.increment as f64),
        )?;
        let date_time_native = if native == Native::TemporalZonedDateTimeSince {
            Native::TemporalPlainDateTimeSince
        } else {
            Native::TemporalPlainDateTimeUntil
        };
        self.temporal_plain_date_time_difference(
            p,
            date_time_native,
            left_date_time,
            &[other_date_time, internal_options],
        )
    }

    fn temporal_zoned_date_time_difference_options(
        &mut self,
        p: &ResidualProgram,
        options: Value,
    ) -> Result<ZonedDateTimeDifferenceOptions, JsError> {
        if !options.is_undefined() && !self.is_object_like(options) {
            return Err(self.type_error(p, "Invalid options".into()));
        }
        let largest_value = if options.is_undefined() {
            Value::UNDEFINED
        } else {
            self.get_option_property(p, options, "largestUnit")?
        };
        let largest_was_default = largest_value.is_undefined();
        let largest_value = if largest_was_default {
            None
        } else {
            Some(self.to_string(p, largest_value)?.to_string())
        };
        let increment_value = if options.is_undefined() {
            Value::UNDEFINED
        } else {
            self.get_option_property(p, options, "roundingIncrement")?
        };
        let increment = if increment_value.is_undefined() {
            MIN_ROUNDING_INCREMENT
        } else {
            let increment = self.to_number(p, increment_value)?;
            if !increment.is_finite()
                || increment <= 0.0
                || increment
                    > super::temporal_date_time_difference::MAX_CALENDAR_DIFFERENCE_ROUNDING_INCREMENT
                        as f64
            {
                return Err(self.range_error(p, "Invalid roundingIncrement".into()));
            }
            increment.trunc() as i128
        };
        let mode_value = if options.is_undefined() {
            Value::UNDEFINED
        } else {
            self.get_option_property(p, options, "roundingMode")?
        };
        let rounding_mode = if mode_value.is_undefined() {
            "trunc".to_owned()
        } else {
            self.to_string(p, mode_value)?.to_string()
        };
        let smallest_value = if options.is_undefined() {
            Value::UNDEFINED
        } else {
            self.get_option_property(p, options, "smallestUnit")?
        };
        let smallest_was_default = smallest_value.is_undefined();
        let smallest_value = if smallest_was_default {
            "nanosecond".to_owned()
        } else {
            self.to_string(p, smallest_value)?.to_string()
        };
        let smallest = canonical_zoned_difference_unit(&smallest_value)
            .ok_or_else(|| self.range_error(p, "Invalid smallestUnit".into()))?;
        let largest_value = largest_value.unwrap_or_else(|| {
            if largest_was_default
                && zoned_date_time_unit_rank(smallest) < zoned_date_time_unit_rank("hour")
            {
                smallest.to_owned()
            } else {
                "hour".to_owned()
            }
        });
        let largest = if normalize_zoned_difference_unit(&largest_value) == "auto" {
            "hour"
        } else {
            canonical_zoned_difference_unit(&largest_value)
                .ok_or_else(|| self.range_error(p, "Invalid largestUnit".into()))?
        };
        if zoned_date_time_unit_rank(smallest) < zoned_date_time_unit_rank(largest) {
            return Err(self.range_error(p, "smallestUnit larger than largestUnit".into()));
        }
        if !ROUNDING_MODES.contains(&rounding_mode.as_str()) {
            return Err(self.range_error(p, "Invalid roundingMode".into()));
        }
        if !super::temporal_date_time_difference::difference_increment_is_valid(
            increment as f64,
            smallest,
            super::temporal_date_time_difference::DifferenceDomain::ZonedDateTime,
        ) {
            return Err(self.range_error(p, "Invalid roundingIncrement".into()));
        }
        Ok(ZonedDateTimeDifferenceOptions {
            largest,
            smallest,
            increment,
            rounding_mode,
        })
    }

    fn zoned_date_time_local_plain_date_time(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
        value: &ZonedDateTimeRecord,
    ) -> Result<Value, JsError> {
        let fields = zoned_date_time_fields(value.epoch_nanoseconds, &value.time_zone)
            .ok_or_else(|| self.range_error(p, "Invalid epochNanoseconds".into()))?;
        let args = [
            Value::number(f64::from(fields[0])),
            Value::number(f64::from(fields[1])),
            Value::number(f64::from(fields[2])),
            Value::number(f64::from(fields[3])),
            Value::number(f64::from(fields[4])),
            Value::number(f64::from(fields[5])),
            Value::number(f64::from(fields[6])),
            Value::number(f64::from(fields[7])),
            Value::number(f64::from(fields[8])),
            self.heap.alloc(Cell::String(value.calendar.clone().into())),
        ];
        self.temporal_plain_date_time_construct(p, &args, constructor)
    }

    fn zoned_date_time_time_difference(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        left: &ZonedDateTimeRecord,
        right: &ZonedDateTimeRecord,
        options: &ZonedDateTimeDifferenceOptions,
    ) -> Result<Value, JsError> {
        let direction = if native == Native::TemporalZonedDateTimeSince {
            -1_i128
        } else {
            1_i128
        };
        let difference = (right.epoch_nanoseconds - left.epoch_nanoseconds) * direction;
        let quantum = zoned_date_time_unit_nanoseconds(options.smallest)
            .ok_or_else(|| self.range_error(p, "Invalid smallestUnit".into()))?
            * options.increment;
        let rounded = super::temporal_zoned_date_time::round_temporal_nanoseconds(
            difference,
            quantum,
            &options.rounding_mode,
        ) * quantum;
        let sign = rounded.signum();
        let mut remainder = rounded.unsigned_abs() as i128;
        let mut time = [0_i128; 6];
        let scales = super::temporal_date_arithmetic::TIME_UNIT_NANOSECOND_SCALES;
        for (index, scale) in scales.into_iter().enumerate() {
            time[index] = remainder / scale;
            remainder %= scale;
        }
        let first_unit =
            zoned_date_time_unit_rank(options.largest) - zoned_date_time_unit_rank("hour");
        if first_unit != 0 {
            time[first_unit] += time[..first_unit]
                .iter()
                .zip(scales.iter().take(first_unit))
                .map(|(value, scale)| value * scale)
                .sum::<i128>()
                / scales[first_unit];
            time[..first_unit].fill(0);
        }
        let mut fields = [0.0; 10];
        for (index, value) in time.into_iter().enumerate() {
            fields[index + super::temporal_date_arithmetic::DURATION_HOURS_FIELD] =
                (value * sign) as f64;
        }
        self.make_temporal_duration(p, fields)
    }

    fn temporal_zoned_date_time_compare_record(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<ZonedDateTimeRecord, JsError> {
        if matches!(self.heap.get(value), Some(Cell::String(_))) {
            let text = self.to_string(p, value)?.to_string();
            super::temporal_plain_date_time_conversion::validate_annotations(self, p, &text)?;
            let parsed = parse_zoned_date_time_string(&text)
                .ok_or_else(|| self.range_error(p, "Invalid ZonedDateTime".into()))?;
            return resolve_zoned_date_time_string(parsed, "reject")
                .ok_or_else(|| self.range_error(p, "Invalid ZonedDateTime".into()));
        }
        self.temporal_zoned_date_time_record(p, value, Value::UNDEFINED)
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
                self.temporal_zoned_date_time_options(p, options)?;
                Ok(ZonedDateTimeRecord {
                    epoch_nanoseconds,
                    time_zone,
                    calendar,
                })
            }
            Some(Cell::String(_)) => {
                let text = self.to_string(p, value)?.to_string();
                super::temporal_plain_date_time_conversion::validate_annotations(self, p, &text)?;
                let parsed = parse_zoned_date_time_string(&text)
                    .ok_or_else(|| self.range_error(p, "Invalid ZonedDateTime".into()))?;
                let options = self.temporal_zoned_date_time_options(p, options)?;
                resolve_zoned_date_time_string(parsed, &options.offset)
                    .ok_or_else(|| self.range_error(p, "Invalid ZonedDateTime".into()))
            }
            Some(Cell::Object(_)) | Some(Cell::Function { .. }) | Some(Cell::Proxy { .. }) => {
                self.temporal_zoned_date_time_property_bag(p, value, options)
            }
            _ => Err(self.type_error(p, "Invalid ZonedDateTime value".into())),
        }
    }

    fn temporal_zoned_date_time_options(
        &mut self,
        p: &ResidualProgram,
        options: Value,
    ) -> Result<ZonedDateTimeOptions, JsError> {
        if options.is_undefined() {
            return Ok(ZonedDateTimeOptions::default());
        }
        if !self.is_object_like(options) {
            return Err(self.type_error(p, "Invalid options".into()));
        }
        Ok(ZonedDateTimeOptions {
            disambiguation: self.temporal_zoned_date_time_option(
                p,
                options,
                "disambiguation",
                "compatible",
                &DISAMBIGUATION_OPTIONS,
            )?,
            offset: self.temporal_zoned_date_time_option(
                p,
                options,
                "offset",
                "reject",
                &OFFSET_OPTIONS,
            )?,
            overflow: self.temporal_zoned_date_time_option(
                p,
                options,
                "overflow",
                "constrain",
                &OVERFLOW_OPTIONS,
            )?,
        })
    }

    fn temporal_zoned_date_time_option(
        &mut self,
        p: &ResidualProgram,
        options: Value,
        name: &str,
        default: &str,
        allowed: &[&str],
    ) -> Result<String, JsError> {
        let atom = self.intern_atom(name);
        let value = self.get_property(p, options, atom)?;
        if value.is_undefined() {
            return Ok(default.to_owned());
        }
        let value = self.to_string(p, value)?.to_string();
        if !allowed.contains(&value.as_str()) {
            return Err(self.range_error(p, "Invalid Temporal option".into()));
        }
        Ok(value)
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
        let calendar = match self.heap.get(calendar_value) {
            None if calendar_value.is_undefined() => "iso8601".to_owned(),
            Some(Cell::String(value)) => {
                super::temporal_date_parse::calendar_identifier_from_string(value.host_string())
                    .ok_or_else(|| self.range_error(p, "Invalid calendar".into()))?
            }
            Some(Cell::TemporalPlainDate { calendar, .. })
            | Some(Cell::TemporalPlainDateTime { calendar, .. })
            | Some(Cell::TemporalPlainMonthDay { calendar, .. })
            | Some(Cell::TemporalPlainYearMonth { calendar, .. })
            | Some(Cell::TemporalZonedDateTime { calendar, .. }) => calendar.clone(),
            _ => return Err(self.type_error(p, "Invalid calendar".into())),
        };
        let day = self.temporal_date_bag_field(p, value, "day")?;
        let hour = self.temporal_date_bag_field(p, value, "hour")?;
        let microsecond = self.temporal_date_bag_field(p, value, "microsecond")?;
        let millisecond = self.temporal_date_bag_field(p, value, "millisecond")?;
        let minute = self.temporal_date_bag_field(p, value, "minute")?;
        let month = self.temporal_date_bag_field(p, value, "month")?;
        let month_code = self.plain_date_time_month_code_field(p, value)?;
        let nanosecond = self.temporal_date_bag_field(p, value, "nanosecond")?;
        let offset_atom = self.intern_atom("offset");
        let offset_value = self.get_property(p, value, offset_atom)?;
        let offset = if offset_value.is_undefined() {
            None
        } else {
            if !matches!(self.heap.get(offset_value), Some(Cell::String(_)))
                && !self.is_object_like(offset_value)
            {
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
        let year = self.plain_date_optional_integer(p, year_value)?;
        let era_atom = self.intern_atom("era");
        let era_value = self.get_property(p, value, era_atom)?;
        let era = if era_value.is_undefined() {
            None
        } else {
            Some(self.to_string(p, era_value)?.to_string())
        };
        let era_year_atom = self.intern_atom("eraYear");
        let era_year_value = self.get_property(p, value, era_year_atom)?;
        let era_year = self.plain_date_optional_integer(p, era_year_value)?;
        let options = self.temporal_zoned_date_time_options(p, options)?;
        let Some(day) = day else {
            return Err(self.type_error(p, "Missing ZonedDateTime field".into()));
        };
        let year = self.resolve_calendar_year(p, &calendar, year, era.as_deref(), era_year)?;
        let month_code = month_code
            .map(|code| self.calendarized_month_code(p, code, &calendar, year))
            .transpose()?;
        let Some(month) = month.or(month_code) else {
            return Err(self.type_error(p, "Missing ZonedDateTime field".into()));
        };
        if month < 1 || day < 1 {
            return Err(self.range_error(p, "Invalid ZonedDateTime".into()));
        }
        if month_code.is_some_and(|month_code| month_code != month) {
            return Err(self.range_error(p, "month and monthCode must agree".into()));
        }
        let iso = quench_intl::calendar_date_to_iso_with_overflow(
            year,
            month as u32,
            day as u32,
            &calendar,
            options.overflow == "constrain",
        )
        .ok_or_else(|| self.range_error(p, "Invalid ZonedDateTime".into()))?;
        let date = super::temporal_date::checked_iso_date(iso.0, iso.1 as i32, iso.2 as i32)
            .ok_or_else(|| self.range_error(p, "Invalid ZonedDateTime".into()))?;
        let time = [
            hour.unwrap_or(0) as u32,
            minute.unwrap_or(0) as u32,
            second.unwrap_or(0) as u32,
            millisecond.unwrap_or(0) as u32,
            microsecond.unwrap_or(0) as u32,
            nanosecond.unwrap_or(0) as u32,
        ];
        let local_epoch = local_epoch_from_iso_fields(date, time);
        let offset = if let Some(offset) = offset {
            if !quench_temporal::valid_timezone_offset(&offset) {
                return Err(self.range_error(p, "Invalid offset".into()));
            }
            Some(
                parse_offset_nanoseconds(&offset)
                    .ok_or_else(|| self.range_error(p, "Invalid offset".into()))?,
            )
        } else {
            None
        };
        let epoch_nanoseconds = resolve_zoned_local_epoch(
            date,
            time,
            &timezone,
            local_epoch,
            offset,
            false,
            &options.offset,
            &options.disambiguation,
        )
        .ok_or_else(|| self.range_error(p, "Offset does not match time zone".into()))?;
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

    pub(super) fn make_temporal_zoned_date_time(
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
pub(super) struct ZonedDateTimeRecord {
    pub(super) epoch_nanoseconds: i128,
    pub(super) time_zone: String,
    pub(super) calendar: String,
}

#[derive(Clone, Copy)]
struct IsoZonedDateTimeBase {
    date: super::temporal_date::IsoDate,
    time: [u32; 6],
    offset_nanoseconds: Option<i128>,
    time_zone_offset_syntax: bool,
    z_designator: bool,
    leap_second: bool,
}

struct ParsedZonedDateTimeString {
    time_zone: String,
    calendar: String,
    local: IsoZonedDateTimeBase,
}

fn parse_zoned_date_time_string(text: &str) -> Option<ParsedZonedDateTimeString> {
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
    let local = parse_iso_zoned_base_fields(base)?;
    Some(ParsedZonedDateTimeString {
        time_zone,
        calendar,
        local,
    })
}

pub(super) fn parse_relative_date_details(
    text: &str,
) -> Option<(super::temporal_date::IsoDate, Option<ZonedDateTimeRecord>)> {
    let (local, zoned_epoch_nanoseconds) = if let Some(parsed) = parse_zoned_date_time_string(text)
    {
        let local = parsed.local;
        let record = resolve_zoned_date_time_string(parsed, "reject")?;
        (local, Some(record))
    } else {
        let (base, annotations) = text
            .split_once('[')
            .map_or((text, None), |(base, tail)| (base, Some(tail)));
        if let Some(annotations) = annotations {
            let (calendar, tail) = annotations.split_once(']')?;
            let calendar = calendar.strip_prefix('!').unwrap_or(calendar);
            if !calendar.starts_with("u-ca=") || !tail.is_empty() {
                return None;
            }
            super::temporal_date_parse::parse_calendar_identifier_name(
                calendar.strip_prefix("u-ca=")?,
            )?;
        }
        (parse_iso_zoned_base_fields(base)?, None)
    };
    if local.z_designator && !text.contains('[') {
        return None;
    }
    let date = local.date;
    super::temporal_date::checked_iso_date(date.year, date.month as i32, date.day as i32)?;
    let mut time = [0; 6];
    for (target, field) in time.iter_mut().zip(local.time) {
        *target = i32::try_from(field).ok()?;
    }
    super::temporal_plain_date_time_conversion::is_within_bounds(
        (date.year, date.month, date.day),
        time,
    )
    .then_some((date, zoned_epoch_nanoseconds))
}

fn resolve_zoned_date_time_string(
    parsed: ParsedZonedDateTimeString,
    offset_mode: &str,
) -> Option<ZonedDateTimeRecord> {
    let ParsedZonedDateTimeString {
        time_zone,
        calendar,
        local,
    } = parsed;
    let [hour, minute, second, millisecond, microsecond, nanosecond] = local.time;
    let local_epoch_nanoseconds = i128::from(super::temporal_date::days_from_iso_date(local.date))
        * NANOSECONDS_PER_DAY
        + i128::from(hour) * NANOSECONDS_PER_HOUR
        + i128::from(minute) * NANOSECONDS_PER_MINUTE
        + i128::from(second) * NANOSECONDS_PER_SECOND
        + i128::from(millisecond) * i128::from(NANOSECONDS_PER_MILLISECOND)
        + i128::from(microsecond) * i128::from(NANOSECONDS_PER_MICROSECOND)
        + i128::from(nanosecond);
    if local_epoch_nanoseconds < -MAX_EPOCH_NANOSECONDS && !matches!(offset_mode, "use" | "ignore")
    {
        return None;
    }
    let epoch_nanoseconds = resolve_zoned_local_epoch(
        local.date,
        local.time,
        &time_zone,
        local_epoch_nanoseconds,
        local.offset_nanoseconds,
        local.z_designator,
        offset_mode,
        "compatible",
    )?;
    if epoch_nanoseconds.unsigned_abs() > MAX_EPOCH_NANOSECONDS as u128 {
        return None;
    }
    Some(ZonedDateTimeRecord {
        epoch_nanoseconds,
        time_zone,
        calendar,
    })
}

fn resolve_zoned_local_epoch(
    date: super::temporal_date::IsoDate,
    time: [u32; 6],
    time_zone: &str,
    local_epoch: i128,
    offset: Option<i128>,
    z_designator: bool,
    offset_mode: &str,
    disambiguation: &str,
) -> Option<i128> {
    let zone_epoch = || zoned_local_epoch_from_iso_fields(date, time, time_zone, disambiguation);
    let Some(offset) = offset else {
        return zone_epoch();
    };
    let offset_epoch = local_epoch.checked_sub(offset)?;
    if offset_mode == "use" || z_designator {
        return Some(offset_epoch);
    }
    if offset_mode == "ignore" {
        return zone_epoch();
    }
    let actual_offset = fixed_time_zone_offset_nanoseconds(time_zone)
        .or_else(|| timezone_offset_nanoseconds(time_zone, offset_epoch));
    match (offset_mode, actual_offset == Some(offset)) {
        (_, true) => Some(offset_epoch),
        ("prefer", false) => zone_epoch(),
        _ => None,
    }
}

pub(super) fn parse_iso_zoned_base(
    value: &str,
) -> Option<(chrono::NaiveDateTime, Option<i128>, bool)> {
    let parsed = parse_iso_zoned_base_fields(value)?;
    let date =
        chrono::NaiveDate::from_ymd_opt(parsed.date.year, parsed.date.month, parsed.date.day)?;
    let [hour, minute, second, millisecond, microsecond, nanosecond] = parsed.time;
    let subsecond = millisecond * NANOSECONDS_PER_MILLISECOND
        + microsecond * NANOSECONDS_PER_MICROSECOND
        + nanosecond;
    let local = date.and_hms_nano_opt(hour, minute, second, subsecond)?;
    Some((local, parsed.offset_nanoseconds, parsed.leap_second))
}

pub(super) fn parse_iso_instant_epoch_nanoseconds(value: &str) -> Option<i128> {
    let base = value.split_once('[').map_or(value, |(base, _)| base);
    let parsed = parse_iso_zoned_base_fields(base)?;
    let offset = parsed.offset_nanoseconds?;
    local_epoch_from_iso_fields(parsed.date, parsed.time).checked_sub(offset)
}

fn parse_iso_zoned_base_fields(value: &str) -> Option<IsoZonedDateTimeBase> {
    let Some((date, time)) = value.split_once(['T', 't', ' ']) else {
        if value.ends_with(['Z', 'z']) {
            return None;
        }
        return Some(IsoZonedDateTimeBase {
            date: parse_iso_zoned_date_fields(value)?,
            time: [0; 6],
            offset_nanoseconds: None,
            time_zone_offset_syntax: true,
            z_designator: false,
            leap_second: false,
        });
    };
    if date == "-000000" || date.starts_with("-000000-") {
        return None;
    }
    let date = parse_iso_zoned_date_fields(date)?;
    let (time, offset_text, z_designator) = if let Some(time) = time.strip_suffix(['Z', 'z']) {
        (time, Some("+00:00"), true)
    } else if let Some(index) = time.get(1..)?.find(['+', '-']).map(|index| index + 1) {
        (&time[..index], Some(&time[index..]), false)
    } else {
        (time, None, false)
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
                .filter(|_| clock.len() >= ISO_TIME_FIELD_DIGITS * 2)
                .and_then(|value| value.parse().ok())
                .unwrap_or(0),
            clock
                .get(ISO_TIME_FIELD_DIGITS * 2..ISO_TIME_FIELD_DIGITS * 3)
                .filter(|_| clock.len() == ISO_TIME_FIELD_DIGITS * 3)
                .and_then(|value| value.parse().ok())
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
    let millisecond = nanosecond / NANOSECONDS_PER_MILLISECOND;
    let microsecond = nanosecond / NANOSECONDS_PER_MICROSECOND % MICROSECONDS_PER_MILLISECOND;
    let nanosecond = nanosecond % NANOSECONDS_PER_MICROSECOND;
    let time_zone_offset_syntax = offset_text.is_none_or(quench_temporal::valid_timezone_offset);
    let offset = match offset_text {
        Some(value) => Some(parse_offset_nanoseconds(value)?),
        None => None,
    };
    Some(IsoZonedDateTimeBase {
        date,
        time: [hour, minute, second, millisecond, microsecond, nanosecond],
        offset_nanoseconds: offset,
        time_zone_offset_syntax,
        z_designator,
        leap_second,
    })
}

fn parse_iso_zoned_date_fields(value: &str) -> Option<super::temporal_date::IsoDate> {
    let (year_text, month_text, day_text) = if value.starts_with(['+', '-']) {
        let sign = &value[..1];
        let body = value.get(1..)?;
        let year = body.get(..EXTENDED_YEAR_DIGITS)?;
        let remainder = body.get(EXTENDED_YEAR_DIGITS..)?;
        let (month, day) = parse_month_day(remainder)?;
        let year = format!("{sign}{year}");
        return super::temporal_date::checked_iso_date(
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
    super::temporal_date::checked_iso_date(
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

pub(super) fn timezone_offset_nanoseconds(zone: &str, epoch: i128) -> Option<i128> {
    if let Some(offset) = fixed_time_zone_offset_nanoseconds(zone) {
        return Some(offset);
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

fn fixed_time_zone_offset_nanoseconds(zone: &str) -> Option<i128> {
    if ZERO_OFFSET_TIME_ZONES.contains(&zone) {
        return Some(0);
    }
    parse_offset_nanoseconds(zone)
}

fn zoned_time_zones_equivalent(left: &str, right: &str) -> bool {
    if left == right {
        return true;
    }
    let left_is_fixed = left.starts_with(['+', '-']) || left.eq_ignore_ascii_case("utc");
    let right_is_fixed = right.starts_with(['+', '-']) || right.eq_ignore_ascii_case("utc");
    left_is_fixed && right_is_fixed
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
    let rounded =
        round_temporal_nanoseconds(nanoseconds, quantum, &options.rounding_mode) * quantum;
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

pub(super) fn format_offset_nanoseconds(offset: i128) -> String {
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
    quench_temporal::valid_timezone_offset(value)
}

fn time_zone_from_datetime_identifier(value: &str) -> Option<String> {
    let (base, has_annotations) = value
        .split_once('[')
        .map_or((value, false), |(base, _)| (base, true));
    if has_annotations {
        return parse_zoned_date_time_string(value).map(|parsed| parsed.time_zone);
    }
    let parsed = parse_iso_zoned_base_fields(base)?;
    if parsed.z_designator {
        return Some("UTC".into());
    }
    if !parsed.time_zone_offset_syntax {
        return None;
    }
    canonical_time_zone(&format_offset_nanoseconds(parsed.offset_nanoseconds?))
}

fn zoned_local_epoch(local: chrono::NaiveDateTime, zone: &str) -> Option<i128> {
    zoned_local_epoch_with_disambiguation(local, zone, "compatible")
}

pub(super) fn zoned_local_epoch_with_disambiguation(
    local: chrono::NaiveDateTime,
    zone: &str,
    disambiguation: &str,
) -> Option<i128> {
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
            chrono::LocalResult::Ambiguous(first, second) => match disambiguation {
                "reject" => return None,
                "later" if first.timestamp() <= second.timestamp() => second,
                "later" => first,
                _ if first.timestamp() <= second.timestamp() => first,
                _ => second,
            },
            chrono::LocalResult::None => {
                if disambiguation == "reject" {
                    return None;
                }
                let before = local.checked_sub_signed(Duration::days(1))?;
                let after = local.checked_add_signed(Duration::days(1))?;
                let before_offset = zone
                    .offset_from_utc_datetime(&before)
                    .fix()
                    .local_minus_utc();
                let after_offset = zone
                    .offset_from_utc_datetime(&after)
                    .fix()
                    .local_minus_utc();
                let offset = if disambiguation == "earlier" {
                    after_offset
                } else {
                    before_offset
                };
                let utc = local
                    .and_utc()
                    .checked_sub_signed(Duration::seconds(i64::from(offset)))?;
                return Some(
                    i128::from(utc.timestamp()) * NANOSECONDS_PER_SECOND
                        + i128::from(utc.timestamp_subsec_nanos()),
                );
            }
        };
        i128::from(instant.timestamp()) * NANOSECONDS_PER_SECOND
            + i128::from(instant.timestamp_subsec_nanos())
    };
    (instant.unsigned_abs() <= MAX_EPOCH_NANOSECONDS as u128).then_some(instant)
}

pub(super) fn zoned_local_epoch_from_iso_fields(
    date: super::temporal_date::IsoDate,
    time: [u32; 6],
    zone: &str,
    disambiguation: &str,
) -> Option<i128> {
    if time
        .iter()
        .zip(ZONED_DATE_TIME_TIME_FIELD_LIMITS)
        .any(|(field, limit)| *field > limit as u32)
    {
        return None;
    }
    let local_epoch = local_epoch_from_iso_fields(date, time);
    if let Some(offset) = fixed_time_zone_offset_nanoseconds(zone) {
        return local_epoch.checked_sub(offset);
    }
    let [hour, minute, second, millisecond, microsecond, nanosecond] = time;
    let [
        _,
        _,
        _,
        millisecond_scale,
        microsecond_scale,
        nanosecond_scale,
    ] = super::temporal_date_arithmetic::TIME_UNIT_NANOSECOND_SCALES;
    let subsecond = millisecond * millisecond_scale as u32
        + microsecond * microsecond_scale as u32
        + nanosecond * nanosecond_scale as u32;
    let local = chrono::NaiveDate::from_ymd_opt(date.year, date.month, date.day)?
        .and_hms_nano_opt(hour, minute, second, subsecond)?;
    zoned_local_epoch_with_disambiguation(local, zone, disambiguation)
}

pub(super) fn local_epoch_from_iso_fields(
    date: super::temporal_date::IsoDate,
    time: [u32; 6],
) -> i128 {
    i128::from(super::temporal_date::days_from_iso_date(date)) * NANOSECONDS_PER_DAY
        + time
            .iter()
            .zip(super::temporal_date_arithmetic::TIME_UNIT_NANOSECOND_SCALES)
            .map(|(field, scale)| i128::from(*field) * scale)
            .sum::<i128>()
}

pub(super) fn zoned_date_time_fields(epoch: i128, zone: &str) -> Option<[i32; 9]> {
    if let Some(offset) = fixed_time_zone_offset_nanoseconds(zone) {
        let local = epoch.checked_add(offset)?;
        let date_days = i64::try_from(local.div_euclid(NANOSECONDS_PER_DAY)).ok()?;
        let date = quench_temporal::civil_from_days(date_days)?;
        let date =
            super::temporal_date::checked_iso_date(date.year, date.month as i32, date.day as i32)?;
        return zoned_fields_from_date_and_time(date, local.rem_euclid(NANOSECONDS_PER_DAY));
    }
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
    let date = super::temporal_date::checked_iso_date(
        local.year(),
        i32::try_from(local.month()).ok()?,
        i32::try_from(local.day()).ok()?,
    )?;
    let time = i128::from(local.hour()) * NANOSECONDS_PER_HOUR
        + i128::from(local.minute()) * NANOSECONDS_PER_MINUTE
        + i128::from(local.second()) * NANOSECONDS_PER_SECOND
        + i128::from(subsecond);
    zoned_fields_from_date_and_time(date, time)
}

fn zoned_fields_from_date_and_time(
    date: super::temporal_date::IsoDate,
    mut time: i128,
) -> Option<[i32; 9]> {
    let hour = time / NANOSECONDS_PER_HOUR;
    time %= NANOSECONDS_PER_HOUR;
    let minute = time / NANOSECONDS_PER_MINUTE;
    time %= NANOSECONDS_PER_MINUTE;
    let second = time / NANOSECONDS_PER_SECOND;
    time %= NANOSECONDS_PER_SECOND;
    let millisecond = time / i128::from(NANOSECONDS_PER_MILLISECOND);
    time %= i128::from(NANOSECONDS_PER_MILLISECOND);
    let microsecond = time / i128::from(NANOSECONDS_PER_MICROSECOND);
    let nanosecond = time % i128::from(NANOSECONDS_PER_MICROSECOND);
    Some([
        date.year,
        date.month as i32,
        date.day as i32,
        i32::try_from(hour).ok()?,
        i32::try_from(minute).ok()?,
        i32::try_from(second).ok()?,
        i32::try_from(millisecond).ok()?,
        i32::try_from(microsecond).ok()?,
        i32::try_from(nanosecond).ok()?,
    ])
}

fn find_time_zone_transition(time_zone: &str, epoch: i128, direction: &str) -> Option<i128> {
    if time_zone.starts_with(['+', '-']) || time_zone.eq_ignore_ascii_case("utc") {
        return None;
    }
    let zone = time_zone.parse::<chrono_tz::Tz>().ok()?;
    let offset_at = |instant: i128| -> Option<i32> {
        let seconds = i64::try_from(instant.div_euclid(NANOSECONDS_PER_SECOND)).ok()?;
        let nanoseconds = u32::try_from(instant.rem_euclid(NANOSECONDS_PER_SECOND)).ok()?;
        zone.timestamp_opt(seconds, nanoseconds)
            .single()
            .map(|date| date.offset().fix().local_minus_utc())
    };
    let current_offset = offset_at(epoch)?;
    let base_offset = if direction == "previous" {
        let previous_instant = epoch.checked_sub(NANOSECOND)?;
        let previous_offset = offset_at(previous_instant)?;
        if previous_offset != current_offset {
            previous_offset
        } else {
            current_offset
        }
    } else {
        current_offset
    };
    let mut previous = epoch;
    for _ in 0..TIME_ZONE_TRANSITION_SEARCH_LIMIT {
        let candidate = if direction == "next" {
            previous.checked_add(NANOSECONDS_PER_DAY)?
        } else {
            previous.checked_sub(NANOSECONDS_PER_DAY)?
        };
        if candidate.unsigned_abs() > MAX_EPOCH_NANOSECONDS as u128 {
            return None;
        }
        if offset_at(candidate)? != base_offset {
            let (mut low, mut high) = if direction == "next" {
                (previous, candidate)
            } else {
                (candidate, previous)
            };
            if direction == "next" {
                while high - low > NANOSECOND {
                    let middle = low + (high - low) / 2;
                    if offset_at(middle)? == base_offset {
                        low = middle;
                    } else {
                        high = middle;
                    }
                }
                return Some(high);
            }
            while high - low > NANOSECOND {
                let middle = low + (high - low) / 2;
                if offset_at(middle)? == base_offset {
                    high = middle;
                } else {
                    low = middle;
                }
            }
            return Some(high);
        }
        previous = candidate;
    }
    None
}

fn round_zoned_local_nanoseconds(
    epoch_nanoseconds: i128,
    offset_nanoseconds: i128,
    quantum: i128,
    rounding_mode: &str,
) -> Option<i128> {
    let local_nanoseconds = epoch_nanoseconds.checked_add(offset_nanoseconds)?;
    let quotient = local_nanoseconds.div_euclid(quantum);
    let remainder = local_nanoseconds.rem_euclid(quantum);
    let tie = remainder * ROUNDING_TIE_FACTOR == quantum;
    let above_tie = remainder * ROUNDING_TIE_FACTOR > quantum;
    let round_up = match rounding_mode {
        "trunc" | "floor" => false,
        "ceil" | "expand" => remainder != 0,
        "halfExpand" | "halfCeil" => remainder * ROUNDING_TIE_FACTOR >= quantum,
        "halfFloor" => above_tie,
        "halfTrunc" => above_tie || tie && local_nanoseconds < 0,
        "halfEven" => above_tie || tie && quotient % ROUNDING_TIE_FACTOR != 0,
        _ => return None,
    };
    quotient
        .checked_add(i128::from(round_up))?
        .checked_mul(quantum)?
        .checked_sub(offset_nanoseconds)
}

fn round_zoned_date_time_day(
    epoch_nanoseconds: i128,
    time_zone: &str,
    rounding_mode: &str,
) -> Option<i128> {
    let (start, next) = zoned_date_time_day_bounds(epoch_nanoseconds, time_zone)?;
    let day_length = next.checked_sub(start)?.max(NANOSECOND);
    let elapsed = epoch_nanoseconds.saturating_sub(start).clamp(0, day_length);
    let elapsed_twice = elapsed.checked_mul(ROUNDING_TIE_FACTOR)?;
    let round_up = match rounding_mode {
        "trunc" | "floor" => false,
        "ceil" | "expand" => elapsed != 0,
        "halfExpand" | "halfCeil" => elapsed_twice >= day_length,
        "halfFloor" | "halfTrunc" | "halfEven" => elapsed_twice > day_length,
        _ => return None,
    };
    Some(if round_up { next } else { start })
}

fn zoned_date_time_day_bounds(epoch_nanoseconds: i128, time_zone: &str) -> Option<(i128, i128)> {
    let fields = zoned_date_time_fields(epoch_nanoseconds, time_zone)?;
    let date = chrono::NaiveDate::from_ymd_opt(
        fields[0],
        u32::try_from(fields[1]).ok()?,
        u32::try_from(fields[2]).ok()?,
    )?;
    let start = zoned_local_epoch(date.and_hms_nano_opt(0, 0, 0, 0)?, time_zone)?;
    let next_date = date.succ_opt()?;
    let next = zoned_local_epoch(next_date.and_hms_nano_opt(0, 0, 0, 0)?, time_zone)?;
    Some((start, next))
}

fn normalize_zoned_difference_unit(value: &str) -> &str {
    value.strip_suffix('s').unwrap_or(value)
}

fn canonical_zoned_difference_unit(value: &str) -> Option<&'static str> {
    let normalized = normalize_zoned_difference_unit(value);
    ZONED_DATE_TIME_DIFFERENCE_UNITS
        .iter()
        .copied()
        .find(|unit| *unit == normalized)
}

fn zoned_date_time_unit_rank(value: &str) -> usize {
    ZONED_DATE_TIME_DIFFERENCE_UNITS
        .iter()
        .position(|unit| *unit == value)
        .unwrap_or(ZONED_DATE_TIME_DIFFERENCE_UNITS.len())
}

fn zoned_date_time_unit_nanoseconds(value: &str) -> Option<i128> {
    ZONED_DATE_TIME_ROUND_UNITS
        .iter()
        .find(|(unit, _, _)| *unit == value)
        .map(|(_, nanoseconds, _)| *nanoseconds)
}

fn zoned_date_time_increment_is_valid(increment: i128, limit: i128) -> bool {
    increment >= MIN_ROUNDING_INCREMENT
        && if limit == MIN_ROUNDING_INCREMENT {
            increment == MIN_ROUNDING_INCREMENT
        } else {
            increment < limit && limit % increment == 0
        }
}
