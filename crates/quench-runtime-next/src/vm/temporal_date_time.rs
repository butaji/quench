use super::temporal_date::{IsoDate, checked_iso_date};
use super::*;

const MONTH_CODE_DIGITS: usize = 2;
const TIME_COMPONENT_WIDTH: usize = 2;
const FRACTIONAL_SECOND_DIGITS: usize = 9;
const MILLISECOND_FIELD: usize = 3;
const MICROSECOND_FIELD: usize = 4;
const NANOSECOND_FIELD: usize = 5;
const HOUR_LIMIT: i32 = 23;
const MINUTE_SECOND_LIMIT: i32 = 59;
const SUBSECOND_LIMIT: i32 = 999;

struct PlainDateTimeFromFields {
    calendar: String,
    day: Option<i32>,
    month: Option<i32>,
    month_code: Option<i32>,
    year: Option<i32>,
    time: [Option<i32>; 6],
}

impl<H: Host> Vm<H> {
    pub(super) fn install_temporal_plain_date_time(
        &mut self,
        p: &ResidualProgram,
        temporal: Value,
    ) -> Result<(), JsError> {
        let constructor =
            self.native_with_realm(Native::TemporalPlainDateTime, temporal, self.realm.globals);
        self.set_builtin_function_name(constructor, "PlainDateTime")?;
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
        self.set_builtin_named(p, constructor, "from", Native::TemporalPlainDateTimeFrom)?;
        self.set_builtin_named(
            p,
            constructor,
            "compare",
            Native::TemporalPlainDateTimeCompare,
        )?;
        for (name, native) in [
            ("add", Native::TemporalPlainDateTimeAdd),
            ("subtract", Native::TemporalPlainDateTimeSubtract),
            ("round", Native::TemporalPlainDateTimeRound),
            ("until", Native::TemporalPlainDateTimeUntil),
            ("since", Native::TemporalPlainDateTimeSince),
            ("toString", Native::TemporalPlainDateTimeToString),
            ("toJSON", Native::TemporalPlainDateTimeToJSON),
            ("toPlainDate", Native::TemporalPlainDateTimeToPlainDate),
            ("toPlainTime", Native::TemporalPlainDateTimeToPlainTime),
            ("toZonedDateTime", Native::TemporalPlainDateTimeToZonedDateTime),
            ("with", Native::TemporalPlainDateTimeWith),
            ("withCalendar", Native::TemporalPlainDateTimeWithCalendar),
            ("withPlainTime", Native::TemporalPlainDateTimeWithPlainTime),
            ("valueOf", Native::TemporalPlainDateTimeValueOf),
        ] {
            self.set_builtin_named(p, prototype, name, native)?;
        }
        for (name, native) in [
            ("calendarId", Native::TemporalPlainDateTimeCalendarIdGetter),
            ("year", Native::TemporalPlainDateTimeYearGetter),
            ("month", Native::TemporalPlainDateTimeMonthGetter),
            ("monthCode", Native::TemporalPlainDateTimeMonthCodeGetter),
            ("day", Native::TemporalPlainDateTimeDayGetter),
            ("era", Native::TemporalPlainDateTimeEraGetter),
            ("eraYear", Native::TemporalPlainDateTimeEraYearGetter),
            ("dayOfWeek", Native::TemporalPlainDateTimeDayOfWeekGetter),
            ("dayOfYear", Native::TemporalPlainDateTimeDayOfYearGetter),
            ("weekOfYear", Native::TemporalPlainDateTimeWeekOfYearGetter),
            ("yearOfWeek", Native::TemporalPlainDateTimeYearOfWeekGetter),
            ("daysInWeek", Native::TemporalPlainDateTimeDaysInWeekGetter),
            ("daysInMonth", Native::TemporalPlainDateTimeDaysInMonthGetter),
            ("daysInYear", Native::TemporalPlainDateTimeDaysInYearGetter),
            ("monthsInYear", Native::TemporalPlainDateTimeMonthsInYearGetter),
            ("inLeapYear", Native::TemporalPlainDateTimeInLeapYearGetter),
            ("hour", Native::TemporalPlainDateTimeHourGetter),
            ("minute", Native::TemporalPlainDateTimeMinuteGetter),
            ("second", Native::TemporalPlainDateTimeSecondGetter),
            (
                "millisecond",
                Native::TemporalPlainDateTimeMillisecondGetter,
            ),
            (
                "microsecond",
                Native::TemporalPlainDateTimeMicrosecondGetter,
            ),
            ("nanosecond", Native::TemporalPlainDateTimeNanosecondGetter),
        ] {
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
        self.set_builtin_named(p, prototype, "equals", Native::TemporalPlainDateTimeEquals)?;
        if let Some(symbol) = self.well_known_symbols.get("toStringTag").copied() {
            let value = self
                .heap
                .alloc(Cell::String("Temporal.PlainDateTime".into()));
            self.set_symbol_property(prototype, symbol, value)?;
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
        self.set_builtin_value_named(temporal, "PlainDateTime", constructor)
    }

    pub(super) fn temporal_plain_date_time_construct(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        let year = self.plain_date_integer(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let month = self.plain_date_integer(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
        let day = self.plain_date_integer(p, args.get(2).copied().unwrap_or(Value::UNDEFINED))?;
        let mut time = [0; 6];
        for (index, value) in time.iter_mut().enumerate() {
            let argument = args.get(index + 3).copied().unwrap_or(Value::UNDEFINED);
            if !argument.is_undefined() {
                *value = self.plain_date_integer(p, argument)?;
            }
        }
        let calendar = args.get(9).copied().unwrap_or(Value::UNDEFINED);
        let calendar = if calendar.is_undefined() {
            "iso8601".to_owned()
        } else if matches!(self.heap.get(calendar), Some(Cell::String(_))) {
            let value = self.to_string(p, calendar)?.to_string();
            super::temporal_date_parse::parse_calendar_identifier(&value)
                .ok_or_else(|| self.range_error(p, "Invalid calendar".into()))?
        } else {
            return Err(self.type_error(p, "Invalid calendar".into()));
        };
        let date = checked_iso_date(year, month, day)
            .ok_or_else(|| self.range_error(p, "Invalid PlainDateTime".into()))?;
        self.validate_plain_date_time_time(p, &time)?;
        super::temporal_plain_date_time_conversion::validate_bounds(
            self, p, date.year, date.month, date.day, time,
        )?;
        let prototype_atom = self.intern_atom("prototype");
        let prototype = self.get_property(p, new_target, prototype_atom)?;
        let prototype = if self.is_object_like(prototype) {
            prototype
        } else {
            self.object_proto
        };
        Ok(self.heap.alloc(Cell::TemporalPlainDateTime {
            object: Box::new(Self::empty_object(prototype)),
            date: (date.year, date.month, date.day),
            time: time.map(|value| value as u32),
            calendar,
        }))
    }

    pub(super) fn validate_plain_date_time_time(
        &mut self,
        p: &ResidualProgram,
        time: &[i32; 6],
    ) -> Result<(), JsError> {
        let valid = (0..=HOUR_LIMIT).contains(&time[0])
            && (0..=MINUTE_SECOND_LIMIT).contains(&time[1])
            && (0..=MINUTE_SECOND_LIMIT).contains(&time[2])
            && time[3..]
                .iter()
                .all(|value| (0..=SUBSECOND_LIMIT).contains(value));
        if valid {
            Ok(())
        } else {
            Err(self.range_error(p, "Invalid PlainDateTime time".into()))
        }
    }

    pub(super) fn temporal_plain_date_time_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if native == Native::TemporalPlainDateTimeFrom {
            let constructor = self.temporal_plain_date_time_constructor(p)?;
            return self.temporal_plain_date_time_from(p, constructor, args);
        }
        if native == Native::TemporalPlainDateTimeCompare {
            return self.temporal_plain_date_time_compare(p, args);
        }
        if native == Native::TemporalPlainDateTimeEquals {
            return self.temporal_plain_date_time_equals(p, this, args);
        }
        if matches!(
            native,
            Native::TemporalPlainDateTimeAdd | Native::TemporalPlainDateTimeSubtract
        ) {
            return self.temporal_plain_date_time_arithmetic(p, native, this, args);
        }
        if native == Native::TemporalPlainDateTimeRound {
            return self.temporal_plain_date_time_round(p, this, args);
        }
        if matches!(native, Native::TemporalPlainDateTimeUntil | Native::TemporalPlainDateTimeSince) {
            return self.temporal_plain_date_time_difference(p, native, this, args);
        }
        if matches!(
            native,
            Native::TemporalPlainDateTimeToString | Native::TemporalPlainDateTimeToJSON
        ) {
            let (date, time, _) = self.temporal_plain_date_time_slots(p, this)?;
            let date = super::temporal_date::format_iso_date(date.year, date.month, date.day);
            let fraction = time[MILLISECOND_FIELD]
                * super::temporal_date_arithmetic::TIME_UNIT_NANOSECOND_SCALES
                    [MILLISECOND_FIELD] as u32
                + time[MICROSECOND_FIELD]
                    * super::temporal_date_arithmetic::TIME_UNIT_NANOSECOND_SCALES
                        [MICROSECOND_FIELD] as u32
                + time[NANOSECOND_FIELD];
            let fraction = if fraction == 0 {
                String::new()
            } else {
                let digits = format!("{fraction:0width$}", width = FRACTIONAL_SECOND_DIGITS);
                format!(".{}", digits.trim_end_matches('0'))
            };
            let text = format!(
                "{date}T{:0width$}:{:0width$}:{:0width$}{fraction}",
                time[0],
                time[1],
                time[2],
                width = TIME_COMPONENT_WIDTH,
            );
            return Ok(self.heap.alloc(Cell::String(text.into())));
        }
        if native == Native::TemporalPlainDateTimeToPlainDate {
            let (date, _, calendar) = self.temporal_plain_date_time_slots(p, this)?;
            let constructor = self.temporal_plain_date_constructor(p)?;
            return self.make_temporal_plain_date(p, date, calendar, constructor);
        }
        if native == Native::TemporalPlainDateTimeToPlainTime {
            let (_, time, _) = self.temporal_plain_date_time_slots(p, this)?;
            let args = time.map(|value| Value::number(f64::from(value)));
            return self.temporal_plain_time_construct(p, &args);
        }
        if native == Native::TemporalPlainDateTimeValueOf {
            return Err(self.type_error(p, "Cannot convert PlainDateTime to a number".into()));
        }
        if native == Native::TemporalPlainDateTimeToZonedDateTime {
            return self.temporal_plain_date_time_to_zoned_date_time(p, this, args);
        }
        if native == Native::TemporalPlainDateTimeWith {
            return self.temporal_plain_date_time_with(p, this, args);
        }
        if native == Native::TemporalPlainDateTimeWithCalendar {
            return self.temporal_plain_date_time_with_calendar(p, this, args);
        }
        if native == Native::TemporalPlainDateTimeWithPlainTime {
            return self.temporal_plain_date_time_with_plain_time(p, this, args);
        }
        if native == Native::TemporalPlainDateTime {
            return Err(self.type_error(p, "Temporal.PlainDateTime requires new".into()));
        }
        let (date, time, calendar) = self.temporal_plain_date_time_slots(p, this)?;
        let value = match native {
            Native::TemporalPlainDateTimeCalendarIdGetter => {
                return Ok(self.heap.alloc(Cell::String(calendar.into())));
            }
            Native::TemporalPlainDateTimeYearGetter => i64::from(date.year),
            Native::TemporalPlainDateTimeMonthGetter => i64::from(date.month),
            Native::TemporalPlainDateTimeDayGetter => i64::from(date.day),
            Native::TemporalPlainDateTimeMonthCodeGetter => {
                return Ok(self.heap.alloc(Cell::String(
                    format!("M{:0width$}", date.month, width = MONTH_CODE_DIGITS).into(),
                )));
            }
            Native::TemporalPlainDateTimeHourGetter => i64::from(time[0]),
            Native::TemporalPlainDateTimeMinuteGetter => i64::from(time[1]),
            Native::TemporalPlainDateTimeSecondGetter => i64::from(time[2]),
            Native::TemporalPlainDateTimeMillisecondGetter => i64::from(time[3]),
            Native::TemporalPlainDateTimeMicrosecondGetter => i64::from(time[4]),
            Native::TemporalPlainDateTimeNanosecondGetter => i64::from(time[5]),
            Native::TemporalPlainDateTimeEraGetter | Native::TemporalPlainDateTimeEraYearGetter => {
                return Ok(Value::UNDEFINED);
            }
            Native::TemporalPlainDateTimeDayOfWeekGetter => {
                i64::from(super::temporal_date::iso_day_of_week(date))
            }
            Native::TemporalPlainDateTimeDayOfYearGetter => {
                i64::from(super::temporal_date::iso_day_of_year(date))
            }
            Native::TemporalPlainDateTimeWeekOfYearGetter => {
                return Ok(super::temporal_date::temporal_iso_week(date, &calendar)
                    .map_or(Value::UNDEFINED, |(week, _)| Value::number(f64::from(week))));
            }
            Native::TemporalPlainDateTimeYearOfWeekGetter => {
                return Ok(super::temporal_date::temporal_iso_week(date, &calendar)
                    .map_or(Value::UNDEFINED, |(_, year)| Value::number(f64::from(year))));
            }
            Native::TemporalPlainDateTimeDaysInWeekGetter => {
                super::temporal_date::ISO_DAYS_PER_WEEK
            }
            Native::TemporalPlainDateTimeDaysInMonthGetter => i64::from(
                super::temporal_date::iso_days_in_month(date.year, date.month as i32)
                    .unwrap_or(31),
            ),
            Native::TemporalPlainDateTimeDaysInYearGetter => {
                i64::from(super::temporal_date::iso_days_in_year(date.year))
            }
            Native::TemporalPlainDateTimeMonthsInYearGetter => {
                i64::from(super::temporal_date::ISO_MONTHS_PER_YEAR)
            }
            Native::TemporalPlainDateTimeInLeapYearGetter => {
                return Ok(if super::temporal_date::iso_is_leap_year(date.year) {
                    Value::TRUE
                } else {
                    Value::FALSE
                });
            }
            _ => unreachable!("not a Temporal.PlainDateTime native"),
        };
        Ok(Value::number(value as f64))
    }

    pub(super) fn temporal_plain_date_time_from(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        if self.is_object_like(value) {
            if !matches!(
                self.heap.get(value),
                Some(
                    Cell::TemporalPlainDateTime { .. }
                        | Cell::TemporalPlainDate { .. }
                        | Cell::TemporalZonedDateTime { .. }
                )
            ) {
                return self.temporal_plain_date_time_from_bag(p, constructor, value, args);
            }
            let plain_date_constructor = self.native_value(Native::TemporalPlainDate);
            let date = self.temporal_plain_date_from(p, plain_date_constructor, &[value])?;
            let (year, month, day, calendar) = self.temporal_plain_date_slots(p, date)?;
            let time = super::temporal_plain_date_time_conversion::to_date_time(
                self,
                p,
                value,
                args.get(1).copied().unwrap_or(Value::UNDEFINED),
            )?
            .map(|value| value as i32);
            let args = [
                Value::number(f64::from(year)),
                Value::number(f64::from(month)),
                Value::number(f64::from(day)),
                Value::number(f64::from(time[0])),
                Value::number(f64::from(time[1])),
                Value::number(f64::from(time[2])),
                Value::number(f64::from(time[3])),
                Value::number(f64::from(time[4])),
                Value::number(f64::from(time[5])),
                self.heap.alloc(Cell::String(calendar.into())),
            ];
            return self.temporal_plain_date_time_construct(p, &args, constructor);
        }
        if !self.is_string(value) {
            return Err(self.type_error(p, "Invalid PlainDateTime".into()));
        }
        let text = self.to_string(p, value)?;
        let (date, calendar) = super::temporal_date_parse::parse_plain_date_string(&text)
            .ok_or_else(|| self.range_error(p, "Invalid PlainDateTime string".into()))?;
        let base = text.split('[').next().unwrap_or(&text);
        let time = if let Some((_, time)) = base.split_once(['T', 't', ' ']) {
            let time = self.heap.alloc(Cell::String(time.to_owned().into()));
            super::temporal_plain_date_time_conversion::parse_time_string(self, p, time)?
        } else {
            [0; 6]
        };
        let _ = self.plain_date_overflow(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
        let args = [
            Value::number(f64::from(date.year)),
            Value::number(f64::from(date.month)),
            Value::number(f64::from(date.day)),
            Value::number(f64::from(time[0])),
            Value::number(f64::from(time[1])),
            Value::number(f64::from(time[2])),
            Value::number(f64::from(time[3])),
            Value::number(f64::from(time[4])),
            Value::number(f64::from(time[5])),
            self.heap.alloc(Cell::String(calendar.into())),
        ];
        self.temporal_plain_date_time_construct(p, &args, constructor)
    }

    fn temporal_plain_date_time_from_bag(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
        bag: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let fields = self.read_plain_date_time_fields(p, bag)?;
        let constrain =
            self.plain_date_overflow(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
        let resolved = self.resolve_plain_date_time_fields(p, fields, constrain)?;
        self.make_plain_date_time(
            p,
            constructor,
            resolved.0,
            resolved.1,
            resolved.2,
            resolved.3,
            resolved.4,
        )
    }

    fn read_plain_date_time_fields(
        &mut self,
        p: &ResidualProgram,
        bag: Value,
    ) -> Result<PlainDateTimeFromFields, JsError> {
        let calendar_atom = self.intern_atom("calendar");
        let calendar = self.get_property(p, bag, calendar_atom)?;
        let calendar = self.temporal_calendar_property(p, calendar)?;
        let day = self.plain_date_field(p, bag, "day")?;
        let mut time = [None; 6];
        for (index, name) in super::temporal_plain_date_time_conversion::TIME_FIELDS
            .iter()
            .take(super::temporal_plain_date_time_conversion::TIME_FIELDS_BEFORE_MONTH)
            .enumerate()
        {
            time[index] = self.plain_date_field(p, bag, name)?;
        }
        let month = self.plain_date_field(p, bag, "month")?;
        let month_code = self.plain_date_time_month_code_field(p, bag)?;
        for (index, name) in super::temporal_plain_date_time_conversion::TIME_FIELDS
            .iter()
            .enumerate()
            .skip(super::temporal_plain_date_time_conversion::TIME_FIELDS_BEFORE_MONTH)
        {
            time[index] = self.plain_date_field(p, bag, name)?;
        }
        let year = self.plain_date_field(p, bag, "year")?;
        Ok(PlainDateTimeFromFields {
            calendar,
            day,
            month,
            month_code,
            year,
            time,
        })
    }

    fn plain_date_field(
        &mut self,
        p: &ResidualProgram,
        bag: Value,
        name: &str,
    ) -> Result<Option<i32>, JsError> {
        let atom = self.intern_atom(name);
        let value = self.get_property(p, bag, atom)?;
        self.plain_date_optional_integer(p, value)
    }

    pub(super) fn plain_date_time_month_code_field(
        &mut self,
        p: &ResidualProgram,
        bag: Value,
    ) -> Result<Option<i32>, JsError> {
        let month_code_atom = self.intern_atom("monthCode");
        let value = self.get_property(p, bag, month_code_atom)?;
        if value.is_undefined() {
            return Ok(None);
        }
        let primitive = if self.is_string(value) {
            value
        } else if self.is_object_like(value) {
            self.to_primitive(p, value, "string")?
        } else {
            return Err(self.type_error(p, "Invalid monthCode".into()));
        };
        let Some(Cell::String(text)) = self.heap.get(primitive) else {
            return Err(self.type_error(p, "Invalid monthCode".into()));
        };
        super::temporal_date::parse_iso_month_code_syntax(text.host_string())
            .map(Some)
            .ok_or_else(|| self.range_error(p, "Invalid monthCode".into()))
    }

    fn resolve_plain_date_time_fields(
        &mut self,
        p: &ResidualProgram,
        fields: PlainDateTimeFromFields,
        constrain: bool,
    ) -> Result<(i32, u32, u32, String, [i32; 6]), JsError> {
        let year = fields
            .year
            .ok_or_else(|| self.type_error(p, "Missing year".into()))?;
        let day = fields
            .day
            .ok_or_else(|| self.type_error(p, "Missing day".into()))?;
        let month_code = fields
            .month_code
            .map(|month| {
                (1..=super::temporal_date::ISO_MONTHS_PER_YEAR)
                    .contains(&month)
                    .then_some(month)
                    .ok_or_else(|| self.range_error(p, "Invalid monthCode".into()))
            })
            .transpose()?;
        let month = match (fields.month, month_code) {
            (Some(month), Some(code)) if month != code => {
                return Err(self.range_error(p, "month and monthCode must agree".into()));
            }
            (Some(month), _) => month,
            (None, Some(code)) => code,
            (None, None) => return Err(self.type_error(p, "Missing month".into())),
        };
        if month < 1 || day < 1 {
            return Err(self.range_error(p, "Invalid PlainDateTime".into()));
        }
        let month = if constrain {
            month.min(super::temporal_date::ISO_MONTHS_PER_YEAR)
        } else {
            month
        };
        let max_day = super::temporal_date::iso_days_in_month(year, month).unwrap_or(31);
        let day = if constrain { day.min(max_day) } else { day };
        let date = checked_iso_date(year, month, day)
            .ok_or_else(|| self.range_error(p, "Invalid PlainDateTime".into()))?;
        let time = self.resolve_plain_date_time_time(p, fields.time, constrain)?;
        super::temporal_plain_date_time_conversion::validate_bounds(
            self, p, date.year, date.month, date.day, time,
        )?;
        Ok((date.year, date.month, date.day, fields.calendar, time))
    }

    fn resolve_plain_date_time_time(
        &mut self,
        p: &ResidualProgram,
        fields: [Option<i32>; 6],
        constrain: bool,
    ) -> Result<[i32; 6], JsError> {
        let time = fields
            .into_iter()
            .zip(super::temporal_plain_date_time_conversion::TIME_LIMITS)
            .map(|(value, limit)| {
                let value = value.unwrap_or_default();
                if constrain {
                    value.clamp(0, limit)
                } else {
                    value
                }
            })
            .collect::<Vec<_>>();
        let [hour, microsecond, millisecond, minute, nanosecond, second] =
            <[i32; 6]>::try_from(time).map_err(|_| JsError("invalid time field width".into()))?;
        let time = [hour, minute, second, millisecond, microsecond, nanosecond];
        self.validate_plain_date_time_time(p, &time)?;
        Ok(time)
    }

    fn make_plain_date_time(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
        year: i32,
        month: u32,
        day: u32,
        calendar: String,
        time: [i32; 6],
    ) -> Result<Value, JsError> {
        let args = [
            Value::number(f64::from(year)),
            Value::number(f64::from(month)),
            Value::number(f64::from(day)),
            Value::number(f64::from(time[0])),
            Value::number(f64::from(time[1])),
            Value::number(f64::from(time[2])),
            Value::number(f64::from(time[3])),
            Value::number(f64::from(time[4])),
            Value::number(f64::from(time[5])),
            self.heap.alloc(Cell::String(calendar.into())),
        ];
        self.temporal_plain_date_time_construct(p, &args, constructor)
    }

    fn temporal_plain_date_time_compare(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let constructor = self.temporal_plain_date_time_constructor(p)?;
        let left = self.temporal_plain_date_time_from(
            p,
            constructor,
            &[args.first().copied().unwrap_or(Value::UNDEFINED)],
        )?;
        let right = self.temporal_plain_date_time_from(
            p,
            constructor,
            &[args.get(1).copied().unwrap_or(Value::UNDEFINED)],
        )?;
        let left = self.temporal_plain_date_time_slots(p, left)?;
        let right = self.temporal_plain_date_time_slots(p, right)?;
        let left_key = (left.0.year, left.0.month, left.0.day, left.1);
        let right_key = (right.0.year, right.0.month, right.0.day, right.1);
        let ordering = left_key.cmp(&right_key);
        Ok(Value::number(match ordering {
            std::cmp::Ordering::Less => -1.0,
            std::cmp::Ordering::Equal => 0.0,
            std::cmp::Ordering::Greater => 1.0,
        }))
    }

    pub(super) fn temporal_plain_date_time_constructor(
        &mut self,
        p: &ResidualProgram,
    ) -> Result<Value, JsError> {
        let temporal_atom = self.intern_atom("Temporal");
        let temporal = self.get_property(p, self.realm.globals, temporal_atom)?;
        let constructor_atom = self.intern_atom("PlainDateTime");
        self.get_property(p, temporal, constructor_atom)
    }

    fn temporal_plain_date_time_equals(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let left = self.temporal_plain_date_time_slots(p, this)?;
        let other = args.first().copied().unwrap_or(Value::UNDEFINED);
        let right = match self.temporal_plain_date_time_slots(p, other) {
            Ok(right) => right,
            Err(_) => return Ok(Value::FALSE),
        };
        Ok(if left == right {
            Value::TRUE
        } else {
            Value::FALSE
        })
    }

    pub(super) fn temporal_plain_date_time_slots(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<(IsoDate, [u32; 6], String), JsError> {
        match self.heap.get(value) {
            Some(Cell::TemporalPlainDateTime {
                date,
                time,
                calendar,
                ..
            }) => Ok((
                IsoDate {
                    year: date.0,
                    month: date.1,
                    day: date.2,
                },
                time.map(|value| value as u32),
                calendar.clone(),
            )),
            _ => Err(self.type_error(
                p,
                "Temporal.PlainDateTime method called on incompatible receiver".into(),
            )),
        }
    }

    pub(super) fn temporal_plain_date_time_arithmetic(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let mut duration =
            self.duration_record(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        if native == Native::TemporalPlainDateTimeSubtract {
            duration.iter_mut().for_each(|field| *field = -*field);
        }
        self.validate_duration_fields(p, &duration)?;
        let constrain = self.plain_date_overflow(
            p,
            args.get(1).copied().unwrap_or(Value::UNDEFINED),
        )?;
        let (date, time, calendar) = self.temporal_plain_date_time_slots(p, this)?;
        let month_delta = i128::from(
            duration[super::temporal_date_arithmetic::DURATION_YEARS_FIELD] as i64,
        )
            * i128::from(super::temporal_date::ISO_MONTHS_PER_YEAR)
            + i128::from(
                duration[super::temporal_date_arithmetic::DURATION_MONTHS_FIELD] as i64,
            );
        let original_day = date.day;
        let mut date = super::temporal_date::shift_iso_months(date, month_delta)
            .ok_or_else(|| self.range_error(p, "Invalid PlainDateTime".into()))?;
        if !constrain && date.day != original_day {
            return Err(self.range_error(p, "Invalid PlainDateTime".into()));
        }

        let mut time_nanos = time
            .iter()
            .zip(super::temporal_date_arithmetic::TIME_UNIT_NANOSECOND_SCALES)
            .map(|(value, scale)| i128::from(*value) * scale)
            .sum::<i128>();
        time_nanos += duration[super::temporal_date_arithmetic::DURATION_HOURS_FIELD..]
            .iter()
            .zip(super::temporal_date_arithmetic::TIME_UNIT_NANOSECOND_SCALES)
            .map(|(value, scale)| *value as i128 * scale)
            .sum::<i128>();
        let carry_days = time_nanos.div_euclid(super::temporal_date_arithmetic::NANOS_PER_DAY);
        let remainder = time_nanos.rem_euclid(super::temporal_date_arithmetic::NANOS_PER_DAY);
        let duration_days = i128::from(
            duration[super::temporal_date_arithmetic::DURATION_WEEKS_FIELD] as i64,
        )
            * i128::from(super::temporal_date_arithmetic::DAYS_PER_WEEK)
            + i128::from(duration[super::temporal_date_arithmetic::DURATION_DAYS_FIELD] as i64);
        let days = i64::try_from(duration_days + carry_days)
            .map_err(|_| self.range_error(p, "Invalid PlainDateTime".into()))?;
        if days != 0 {
            date = super::temporal_date::shift_iso_days(date, days)
                .ok_or_else(|| self.range_error(p, "Invalid PlainDateTime".into()))?;
        }

        let mut time = [0_i32; 6];
        let mut remainder = remainder;
        for (index, scale) in super::temporal_date_arithmetic::TIME_UNIT_NANOSECOND_SCALES
            .into_iter()
            .enumerate()
        {
            time[index] = i32::try_from(remainder / scale)
                .map_err(|_| self.range_error(p, "Invalid PlainDateTime time".into()))?;
            remainder %= scale;
        }
        let constructor = self.temporal_plain_date_time_constructor(p)?;
        self.make_plain_date_time(
            p,
            constructor,
            date.year,
            date.month,
            date.day,
            calendar,
            time,
        )
    }

    fn temporal_plain_date_time_round(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let (date, time, calendar) = self.temporal_plain_date_time_slots(p, this)?;
        let options = args.first().copied().unwrap_or(Value::UNDEFINED);
        let parsed = super::temporal_instant_round::read_options(self, p, options)?;
        let (_, scale) = super::temporal_instant_round::parse_unit(
            self,
            p,
            parsed.smallest_unit.as_deref(),
        )?;
        let increment =
            super::temporal_instant_round::validate_increment(self, p, parsed.increment, scale)?;
        let mode = super::temporal_instant_round::validate_mode(
            self,
            p,
            parsed.rounding_mode.as_deref(),
        )?;
        let total = time
            .iter()
            .zip(super::temporal_date_arithmetic::TIME_UNIT_NANOSECOND_SCALES)
            .map(|(value, scale)| i128::from(*value) * scale)
            .sum::<i128>();
        let quantum = scale * increment;
        let rounded = super::temporal_zoned_date_time::round_temporal_nanoseconds(
            total,
            quantum,
            mode,
        ) * quantum;
        let carry = rounded.div_euclid(super::temporal_date_arithmetic::NANOS_PER_DAY);
        let remainder = rounded.rem_euclid(super::temporal_date_arithmetic::NANOS_PER_DAY);
        let days = i64::try_from(carry)
            .map_err(|_| self.range_error(p, "Invalid PlainDateTime".into()))?;
        let date = super::temporal_date::shift_iso_days(date, days)
            .ok_or_else(|| self.range_error(p, "Invalid PlainDateTime".into()))?;
        let mut time = [0_i32; 6];
        let mut remainder = remainder;
        for (index, scale) in super::temporal_date_arithmetic::TIME_UNIT_NANOSECOND_SCALES
            .into_iter()
            .enumerate()
        {
            time[index] = i32::try_from(remainder / scale)
                .map_err(|_| self.range_error(p, "Invalid PlainDateTime time".into()))?;
            remainder %= scale;
        }
        let constructor = self.temporal_plain_date_time_constructor(p)?;
        self.make_plain_date_time(
            p,
            constructor,
            date.year,
            date.month,
            date.day,
            calendar,
            time,
        )
    }

    fn temporal_plain_date_time_with(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let (date, time, calendar) = self.temporal_plain_date_time_slots(p, this)?;
        let changes = args.first().copied().unwrap_or(Value::UNDEFINED);
        if !self.is_object_like(changes)
            || matches!(self.heap.get(changes), Some(Cell::Array { .. }))
            || matches!(
                self.heap.get(changes),
                Some(
                    Cell::TemporalPlainDate { .. }
                        | Cell::TemporalPlainDateTime { .. }
                        | Cell::TemporalPlainMonthDay { .. }
                        | Cell::TemporalPlainYearMonth { .. }
                        | Cell::TemporalZonedDateTime { .. }
                )
            )
        {
            return Err(self.type_error(p, "Invalid date-time".into()));
        }

        for name in ["calendar", "timeZone"] {
            let atom = self.intern_atom(name);
            if !self.get_property(p, changes, atom)?.is_undefined() {
                return Err(self.type_error(p, format!("Invalid {name}")));
            }
        }

        let mut year = date.year;
        let mut month = date.month as i32;
        let mut month_code = None;
        let mut day = date.day as i32;
        let mut time = time.map(|value| value as i32);
        let names = if calendar == "iso8601" {
            &[
                "day",
                "hour",
                "microsecond",
                "millisecond",
                "minute",
                "month",
                "monthCode",
                "nanosecond",
                "second",
                "year",
            ][..]
        } else {
            &[
                "day",
                "hour",
                "microsecond",
                "millisecond",
                "minute",
                "month",
                "monthCode",
                "nanosecond",
                "second",
                "year",
                "era",
                "eraYear",
            ][..]
        };
        let mut recognized = false;
        let mut month_was_provided = false;
        let mut year_was_provided = false;
        let mut era_was_provided = false;
        let mut era_year_was_provided = false;
        for name in names {
            let atom = self.intern_atom(name);
            let value = self.get_property(p, changes, atom)?;
            if value.is_undefined() {
                continue;
            }
            recognized = true;
            match *name {
                "day" => day = self.plain_date_integer(p, value)?,
                "hour" => time[0] = self.plain_date_integer(p, value)?,
                "microsecond" => time[MICROSECOND_FIELD] = self.plain_date_integer(p, value)?,
                "millisecond" => time[MILLISECOND_FIELD] = self.plain_date_integer(p, value)?,
                "minute" => time[1] = self.plain_date_integer(p, value)?,
                "month" => {
                    month = self.plain_date_integer(p, value)?;
                    month_was_provided = true;
                }
                "monthCode" => month_code = Some(self.plain_date_month_code(p, value)?),
                "nanosecond" => time[NANOSECOND_FIELD] = self.plain_date_integer(p, value)?,
                "second" => time[2] = self.plain_date_integer(p, value)?,
                "year" => {
                    year = self.plain_date_integer(p, value)?;
                    year_was_provided = true;
                }
                "era" => era_was_provided = true,
                "eraYear" => era_year_was_provided = true,
                _ => unreachable!(),
            }
        }
        if era_was_provided != era_year_was_provided && !year_was_provided {
            return Err(self.type_error(p, "era and eraYear must be provided together".into()));
        }
        if era_was_provided && !year_was_provided {
            return Err(self.type_error(p, "Unsupported calendar era fields".into()));
        }
        if !recognized {
            return Err(self.type_error(p, "Insufficient date-time data".into()));
        }
        if let Some(code_month) = month_code {
            if !(1..=super::temporal_date::ISO_MONTHS_PER_YEAR).contains(&code_month) {
                return Err(self.range_error(p, "Invalid monthCode".into()));
            }
            if month_was_provided && month != code_month {
                return Err(self.range_error(p, "Month mismatch".into()));
            }
            if !month_was_provided {
                month = code_month;
            }
        }

        let options = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let options_primitive = !options.is_undefined() && !self.is_object_like(options);
        let overflow = if options.is_undefined() || options_primitive {
            "constrain".to_owned()
        } else {
            let atom = self.intern_atom("overflow");
            let value = self.get_property(p, options, atom)?;
            if value.is_undefined() {
                "constrain".to_owned()
            } else {
                self.to_string(p, value)?.to_string()
            }
        };
        if !matches!(overflow.as_str(), "constrain" | "reject") {
            return Err(self.range_error(p, "Invalid overflow".into()));
        }
        let constrain = overflow == "constrain";
        let month = if constrain {
            month.clamp(1, super::temporal_date::ISO_MONTHS_PER_YEAR)
        } else {
            month
        };
        let max_day = super::temporal_date::iso_days_in_month(year, month).unwrap_or(31);
        if constrain {
            day = day.min(max_day);
        }
        let date = checked_iso_date(year, month, day)
            .ok_or_else(|| self.range_error(p, "Invalid PlainDateTime".into()))?;
        for (value, limit) in time.iter_mut().zip([HOUR_LIMIT, MINUTE_SECOND_LIMIT, MINUTE_SECOND_LIMIT, SUBSECOND_LIMIT, SUBSECOND_LIMIT, SUBSECOND_LIMIT]) {
            if constrain {
                *value = (*value).clamp(0, limit);
            }
        }
        self.validate_plain_date_time_time(p, &time)?;
        super::temporal_plain_date_time_conversion::validate_bounds(
            self, p, date.year, date.month, date.day, time,
        )?;
        if options_primitive {
            return Err(self.type_error(p, "Invalid options".into()));
        }
        let constructor = self.temporal_plain_date_time_constructor(p)?;
        self.make_plain_date_time(
            p,
            constructor,
            date.year,
            date.month,
            date.day,
            calendar,
            time,
        )
    }

    fn temporal_plain_date_time_with_calendar(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let (date, time, _) = self.temporal_plain_date_time_slots(p, this)?;
        let calendar_value = args.first().copied().unwrap_or(Value::UNDEFINED);
        if calendar_value.is_undefined() {
            return Err(self.type_error(p, "Missing calendar".into()));
        }
        let calendar = match self.heap.get(calendar_value) {
            Some(Cell::String(value)) => {
                super::temporal_date_parse::calendar_identifier_from_string(value.host_string())
                    .ok_or_else(|| self.range_error(p, "Invalid calendar".into()))?
            }
            Some(
                Cell::TemporalPlainDate { calendar, .. }
                | Cell::TemporalPlainDateTime { calendar, .. }
                | Cell::TemporalPlainMonthDay { calendar, .. }
                | Cell::TemporalPlainYearMonth { calendar, .. }
                | Cell::TemporalZonedDateTime { calendar, .. },
            ) => calendar.clone(),
            _ => return Err(self.type_error(p, "Invalid calendar".into())),
        };
        let constructor = self.temporal_plain_date_time_constructor(p)?;
        self.make_plain_date_time(
            p,
            constructor,
            date.year,
            date.month,
            date.day,
            calendar,
            time.map(|value| value as i32),
        )
    }

    fn temporal_plain_date_time_with_plain_time(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let (date, _, calendar) = self.temporal_plain_date_time_slots(p, this)?;
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let time = super::temporal_plain_date_time_conversion::to_time(self, p, value)?;
        super::temporal_plain_date_time_conversion::validate_bounds(
            self, p, date.year, date.month, date.day, time,
        )?;
        let constructor = self.temporal_plain_date_time_constructor(p)?;
        self.make_plain_date_time(
            p,
            constructor,
            date.year,
            date.month,
            date.day,
            calendar,
            time,
        )
    }

    fn temporal_plain_date_time_to_zoned_date_time(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let (date, time, calendar) = self.temporal_plain_date_time_slots(p, this)?;
        let timezone_value = args.first().copied().unwrap_or(Value::UNDEFINED);
        if timezone_value.is_undefined() {
            return Err(self.type_error(p, "Missing time zone".into()));
        }
        let timezone = self.temporal_timezone_id(p, timezone_value)?;
        let options = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let disambiguation = if options.is_undefined() {
            "compatible"
        } else {
            if !self.is_object_like(options) {
                return Err(self.type_error(p, "Invalid options".into()));
            }
            let key = self.intern_atom("disambiguation");
            let value = self.get_property(p, options, key)?;
            if value.is_undefined() {
                "compatible"
            } else {
                let mode = self.to_string(p, value)?.to_string();
                if !super::temporal_zoned_date_time::DISAMBIGUATION_OPTIONS.contains(&mode.as_str()) {
                    return Err(self.range_error(p, "Invalid disambiguation".into()));
                }
                return self.make_zoned_date_time_from_local(
                    p,
                    date,
                    time,
                    calendar,
                    timezone,
                    &mode,
                );
            }
        };
        self.make_zoned_date_time_from_local(p, date, time, calendar, timezone, disambiguation)
    }

    pub(super) fn make_zoned_date_time_from_local(
        &mut self,
        p: &ResidualProgram,
        date: super::temporal_date::IsoDate,
        time: [u32; 6],
        calendar: String,
        timezone: String,
        disambiguation: &str,
    ) -> Result<Value, JsError> {
        let epoch = super::temporal_zoned_date_time::zoned_local_epoch_from_iso_fields(
            date,
            time,
            &timezone,
            disambiguation,
        )
        .ok_or_else(|| self.range_error(p, "Invalid time zone transition".into()))?;
        if epoch.unsigned_abs()
            > super::temporal_zoned_date_time::MAX_EPOCH_NANOSECONDS as u128
        {
            return Err(self.range_error(p, "Invalid instant".into()));
        }
        let temporal_atom = self.intern_atom("Temporal");
        let temporal = self.get_property(p, self.realm.globals, temporal_atom)?;
        let constructor_atom = self.intern_atom("ZonedDateTime");
        let constructor = self.get_property(p, temporal, constructor_atom)?;
        self.make_temporal_zoned_date_time(
            p,
            constructor,
            super::temporal_zoned_date_time::ZonedDateTimeRecord {
                epoch_nanoseconds: epoch,
                time_zone: timezone,
                calendar,
            },
        )
    }
}
