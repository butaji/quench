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
            self.validate_zoned_date_time_options(
                p,
                args.get(1).copied().unwrap_or(Value::UNDEFINED),
            )?;
            let input = self.temporal_zoned_date_time_record(
                p,
                args.first().copied().unwrap_or(Value::UNDEFINED),
            )?;
            return self.make_temporal_zoned_date_time(p, this, input);
        }
        if native == Native::TemporalZonedDateTimeCompare {
            let left = self.temporal_zoned_date_time_record(
                p,
                args.first().copied().unwrap_or(Value::UNDEFINED),
            )?;
            let right = self.temporal_zoned_date_time_record(
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
    ) -> Result<ZonedDateTimeRecord, JsError> {
        match self.heap.get(value).cloned() {
            Some(Cell::TemporalZonedDateTime {
                epoch_nanoseconds,
                time_zone,
                calendar,
                ..
            }) => Ok(ZonedDateTimeRecord {
                epoch_nanoseconds,
                time_zone,
                calendar,
            }),
            Some(Cell::String(_)) => {
                let text = self.to_string(p, value)?.to_string();
                parse_zoned_date_time_string(&text)
                    .ok_or_else(|| self.range_error(p, "Invalid ZonedDateTime".into()))
            }
            Some(Cell::Object(_)) | Some(Cell::Proxy { .. }) => {
                let epoch_atom = self.intern_atom("epochNanoseconds");
                let epoch_value = self.get_property(p, value, epoch_atom)?;
                let epoch_nanoseconds = match self.heap.get(epoch_value).cloned() {
                    Some(Cell::BigInt(text)) => text
                        .parse::<i128>()
                        .map_err(|_| self.range_error(p, "Invalid epochNanoseconds".into()))?,
                    _ => return Err(self.type_error(p, "Invalid ZonedDateTime".into())),
                };
                let zone_atom = self.intern_atom("timeZoneId");
                let zone = self.get_property(p, value, zone_atom)?;
                let time_zone = self.temporal_timezone_id(p, zone)?;
                let calendar_atom = self.intern_atom("calendarId");
                let calendar_value = self.get_property(p, value, calendar_atom)?;
                let calendar_text = self.to_string(p, calendar_value)?.to_string();
                let calendar =
                    super::temporal_date_parse::parse_calendar_identifier(&calendar_text)
                        .ok_or_else(|| self.range_error(p, "Invalid calendar".into()))?;
                Ok(ZonedDateTimeRecord {
                    epoch_nanoseconds,
                    time_zone,
                    calendar,
                })
            }
            _ => Err(self.type_error(p, "Invalid ZonedDateTime value".into())),
        }
    }

    fn validate_zoned_date_time_options(
        &mut self,
        p: &ResidualProgram,
        options: Value,
    ) -> Result<(), JsError> {
        if options.is_undefined() {
            return Ok(());
        }
        if !self.is_object_like(options) {
            return Err(self.type_error(p, "Invalid options".into()));
        }
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
        }
        Ok(())
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
    let (base, annotations) = text.split_once('[')?;
    if !annotations.ends_with(']') || annotations.matches('[').count() != 0 {
        return None;
    }
    let annotations = annotations[..annotations.len() - 1].split("][");
    let mut time_zone = None;
    let mut calendar = None;
    for annotation in annotations {
        if let Some(value) = annotation.strip_prefix("u-ca=") {
            calendar = Some(value);
        } else if !annotation.contains('=') {
            time_zone = Some(annotation);
        }
    }
    let time_zone = canonical_time_zone(time_zone?)?;
    let parsed = chrono::DateTime::parse_from_rfc3339(base).ok()?;
    let epoch_nanoseconds = i128::from(parsed.timestamp()) * NANOSECONDS_PER_SECOND
        + i128::from(parsed.timestamp_subsec_nanos());
    if epoch_nanoseconds.unsigned_abs() > MAX_EPOCH_NANOSECONDS as u128 {
        return None;
    }
    let calendar = calendar.unwrap_or("iso8601");
    let calendar = super::temporal_date_parse::parse_calendar_identifier(calendar)?;
    Some(ZonedDateTimeRecord {
        epoch_nanoseconds,
        time_zone,
        calendar,
    })
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
