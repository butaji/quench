use super::temporal_date::{IsoDate, checked_iso_date};
use super::*;

const MONTH_CODE_DIGITS: usize = 2;
const HOUR_LIMIT: i32 = 23;
const MINUTE_SECOND_LIMIT: i32 = 59;
const SUBSECOND_LIMIT: i32 = 999;

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
            ("calendarId", Native::TemporalPlainDateTimeCalendarIdGetter),
            ("year", Native::TemporalPlainDateTimeYearGetter),
            ("month", Native::TemporalPlainDateTimeMonthGetter),
            ("monthCode", Native::TemporalPlainDateTimeMonthCodeGetter),
            ("day", Native::TemporalPlainDateTimeDayGetter),
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
            return self.temporal_plain_date_time_from(p, this, args);
        }
        if native == Native::TemporalPlainDateTimeCompare {
            return self.temporal_plain_date_time_compare(p, args);
        }
        if native == Native::TemporalPlainDateTimeEquals {
            return self.temporal_plain_date_time_equals(p, this, args);
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
            _ => unreachable!("not a Temporal.PlainDateTime native"),
        };
        Ok(Value::number(value as f64))
    }

    fn temporal_plain_date_time_from(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        if self.is_object_like(value) {
            let plain_date_constructor = self.native_value(Native::TemporalPlainDate);
            let date = self.temporal_plain_date_from(p, plain_date_constructor, &[value])?;
            let (year, month, day, calendar) = self.temporal_plain_date_slots(p, date)?;
            let time = if matches!(self.heap.get(value), Some(Cell::TemporalPlainDate { .. })) {
                [0; 6]
            } else {
                super::temporal_plain_date_time_conversion::to_date_time(self, p, value)?
                    .map(|value| value as i32)
            };
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

    fn temporal_plain_date_time_compare(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let constructor = self.native_value(Native::TemporalPlainDateTime);
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
}
