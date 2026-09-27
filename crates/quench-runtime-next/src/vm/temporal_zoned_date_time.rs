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
const ISO_MONTH_DIGITS: usize = 2;
const ISO_DAY_DIGITS: usize = 2;
const MAX_FRACTION_DIGITS: usize = 9;
const ISO_TIME_FIELD_DIGITS: usize = 2;
const ISO_HOUR_LIMIT: u32 = 23;
const ISO_MINUTE_LIMIT: u32 = 59;
const ISO_SECOND_LIMIT: u32 = 59;
const ISO_LEAP_SECOND: u32 = ISO_SECOND_LIMIT + 1;
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
            if !quench_temporal::valid_timezone_offset(&text) {
                return Err(self.range_error(p, "Invalid time zone".into()));
            }
            let seconds = quench_temporal::offset_seconds(&text);
            let sign = if seconds < 0 { '-' } else { '+' };
            let seconds = seconds.unsigned_abs();
            let hours = seconds / SECONDS_PER_HOUR as u32;
            let minutes = seconds / SECONDS_PER_MINUTE as u32 % SECONDS_PER_MINUTE as u32;
            return Ok(format!("{sign}{hours:02}:{minutes:02}"));
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

fn parse_iso_zoned_base(value: &str) -> Option<(chrono::NaiveDateTime, Option<i128>, bool)> {
    let (date, time) = value.split_once(['T', 't', ' '])?;
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

fn canonical_time_zone(value: &str) -> Option<String> {
    if value.eq_ignore_ascii_case("utc") {
        return Some("UTC".into());
    }
    if value.starts_with(['+', '-']) {
        if !quench_temporal::valid_timezone_offset(value) {
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
