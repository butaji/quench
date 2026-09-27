use super::temporal_date::{
    ISO_MONTH_CODE_DIGITS, checked_iso_date, iso_days_in_month, iso_days_in_year, iso_is_leap_year,
};
use super::*;

const DEFAULT_REFERENCE_ISO_YEAR: i32 = 1972;
const DEFAULT_REFERENCE_ISO_DAY: u32 = 1;
const PLAIN_MONTH_DAY_GETTERS: &[(&str, Native)] = &[
    ("calendarId", Native::TemporalPlainMonthDayCalendarIdGetter),
    ("day", Native::TemporalPlainMonthDayDayGetter),
    ("monthCode", Native::TemporalPlainMonthDayMonthCodeGetter),
];
const PLAIN_YEAR_MONTH_GETTERS: &[(&str, Native)] = &[
    ("calendarId", Native::TemporalPlainYearMonthCalendarIdGetter),
    ("year", Native::TemporalPlainYearMonthYearGetter),
    ("month", Native::TemporalPlainYearMonthMonthGetter),
    ("monthCode", Native::TemporalPlainYearMonthMonthCodeGetter),
    (
        "referenceISODay",
        Native::TemporalPlainYearMonthReferenceISODayGetter,
    ),
];
const PLAIN_MONTH_DAY_METHODS: &[(&str, Native)] = &[
    ("toString", Native::TemporalPlainMonthDayToString),
    ("toJSON", Native::TemporalPlainMonthDayToJSON),
    (
        "toLocaleString",
        Native::TemporalPlainMonthDayToLocaleString,
    ),
    ("toPlainDate", Native::TemporalPlainMonthDayToPlainDate),
    ("with", Native::TemporalPlainMonthDayWith),
    ("equals", Native::TemporalPlainMonthDayEquals),
    ("valueOf", Native::TemporalPlainMonthDayValueOf),
];
const PLAIN_YEAR_MONTH_METHODS: &[(&str, Native)] = &[
    ("toString", Native::TemporalPlainYearMonthToString),
    ("toJSON", Native::TemporalPlainYearMonthToJSON),
    (
        "toLocaleString",
        Native::TemporalPlainYearMonthToLocaleString,
    ),
    ("add", Native::TemporalPlainYearMonthAdd),
    ("subtract", Native::TemporalPlainYearMonthSubtract),
    ("until", Native::TemporalPlainYearMonthUntil),
    ("since", Native::TemporalPlainYearMonthSince),
    ("with", Native::TemporalPlainYearMonthWith),
    ("equals", Native::TemporalPlainYearMonthEquals),
    ("valueOf", Native::TemporalPlainYearMonthValueOf),
    ("toPlainDate", Native::TemporalPlainYearMonthToPlainDate),
];

impl<H: Host> Vm<H> {
    pub(super) fn install_temporal_calendar_projections(
        &mut self,
        p: &ResidualProgram,
        temporal: Value,
    ) -> Result<(), JsError> {
        for (name, native, from, compare, getters, methods, is_month_day) in [
            (
                "PlainMonthDay",
                Native::TemporalPlainMonthDay,
                Native::TemporalPlainMonthDayFrom,
                Native::TemporalPlainMonthDayCompare,
                PLAIN_MONTH_DAY_GETTERS,
                PLAIN_MONTH_DAY_METHODS,
                true,
            ),
            (
                "PlainYearMonth",
                Native::TemporalPlainYearMonth,
                Native::TemporalPlainYearMonthFrom,
                Native::TemporalPlainYearMonthCompare,
                PLAIN_YEAR_MONTH_GETTERS,
                PLAIN_YEAR_MONTH_METHODS,
                false,
            ),
        ] {
            let constructor = self.native_with_realm(native, temporal, self.realm.globals);
            self.set_builtin_function_name(constructor, name)?;
            let prototype = self.object();
            if is_month_day {
                self.temporal_plain_month_day_proto = prototype;
            } else {
                self.temporal_plain_year_month_proto = prototype;
            }
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
            self.set_builtin_named(p, constructor, "from", from)?;
            self.set_builtin_named(p, constructor, "compare", compare)?;
            for (property, getter) in getters {
                let getter_value = self.native_value(*getter);
                self.set_builtin_function_name(getter_value, &format!("get {property}"))?;
                let atom = self.intern_atom(property);
                self.set_property(prototype, atom, Value::UNDEFINED)?;
                self.set_property_attributes(
                    prototype,
                    PropertyKey::string(atom),
                    PropertyAttributes {
                        writable: false,
                        enumerable: false,
                        configurable: true,
                        accessor: true,
                        getter: Some(getter_value),
                        setter: None,
                    },
                );
            }
            for (method, native) in methods {
                self.set_builtin_named(p, prototype, method, *native)?;
            }
            if let Some(symbol) = self.well_known_symbols.get("toStringTag").copied() {
                let tag = self
                    .heap
                    .alloc(Cell::String(format!("Temporal.{name}").into()));
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
            self.set_builtin_value_named(temporal, name, constructor)?;
        }
        Ok(())
    }

    pub(super) fn temporal_calendar_projection_construct(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        match native {
            Native::TemporalPlainMonthDay => {
                self.temporal_plain_month_day_construct(p, args, new_target)
            }
            Native::TemporalPlainYearMonth => {
                self.temporal_plain_year_month_construct(p, args, new_target)
            }
            _ => unreachable!("not a Temporal calendar projection constructor"),
        }
    }

    fn temporal_plain_month_day_construct(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        let month =
            self.plain_date_integer(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let day = self.plain_date_integer(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
        let calendar =
            self.calendar_argument(p, args.get(2).copied().unwrap_or(Value::UNDEFINED))?;
        let year_value = args.get(3).copied().unwrap_or(Value::UNDEFINED);
        let year = if year_value.is_undefined() {
            DEFAULT_REFERENCE_ISO_YEAR
        } else {
            self.plain_date_integer(p, year_value)?
        };
        let year = if checked_iso_date(year, month, day).is_some() {
            year
        } else if calendar == "iso8601" && !(-271_821..=275_760).contains(&year) {
            DEFAULT_REFERENCE_ISO_YEAR
        } else {
            return Err(self.range_error(p, "Invalid PlainMonthDay".into()));
        };
        let prototype =
            self.constructor_prototype(p, new_target, self.temporal_plain_month_day_proto)?;
        Ok(self.heap.alloc(Cell::TemporalPlainMonthDay {
            object: Box::new(Self::empty_object(prototype)),
            month: month as u32,
            day: day as u32,
            calendar,
            reference_iso_year: year,
        }))
    }

    fn temporal_plain_year_month_construct(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        let year = self.plain_date_integer(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let month = self.plain_date_integer(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
        let calendar =
            self.calendar_argument(p, args.get(2).copied().unwrap_or(Value::UNDEFINED))?;
        let reference_value = args.get(3).copied().unwrap_or(Value::UNDEFINED);
        let default_reference = reference_value.is_undefined();
        let reference = if default_reference {
            DEFAULT_REFERENCE_ISO_DAY
        } else {
            self.plain_date_integer(p, reference_value)? as u32
        };
        self.validate_plain_year_month_range(p, year, month, reference, default_reference)?;
        let prototype =
            self.constructor_prototype(p, new_target, self.temporal_plain_year_month_proto)?;
        Ok(self.heap.alloc(Cell::TemporalPlainYearMonth {
            object: Box::new(Self::empty_object(prototype)),
            year,
            month: month as u32,
            calendar,
            reference_iso_day: reference,
        }))
    }

    fn validate_plain_year_month_range(
        &mut self,
        p: &ResidualProgram,
        year: i32,
        month: i32,
        day: u32,
        default_reference: bool,
    ) -> Result<(), JsError> {
        let in_year_range = (super::temporal_date::MIN_ISO_YEAR
            ..=super::temporal_date::MAX_ISO_YEAR)
            .contains(&year);
        let in_month_range = (1..=super::temporal_date::ISO_MONTHS_PER_YEAR).contains(&month);
        let implicit_lower_boundary =
            default_reference && year == super::temporal_date::MIN_ISO_YEAR && month == 4;
        let valid_iso_range = checked_iso_date(year, month, day as i32).is_some();
        if !in_year_range || !in_month_range || !valid_iso_range && !implicit_lower_boundary {
            return Err(self.range_error(p, "Invalid PlainYearMonth".into()));
        }
        Ok(())
    }

    fn calendar_argument(&mut self, p: &ResidualProgram, value: Value) -> Result<String, JsError> {
        if value.is_undefined() {
            return Ok("iso8601".to_owned());
        }
        if !matches!(self.heap.get(value), Some(Cell::String(_))) {
            return Err(self.type_error(p, "Invalid calendar".into()));
        }
        let text = self.to_string(p, value)?.to_string();
        temporal_date_parse::parse_calendar_identifier(&text)
            .ok_or_else(|| self.range_error(p, "Invalid calendar".into()))
    }

    fn constructor_prototype(
        &mut self,
        p: &ResidualProgram,
        new_target: Value,
        default: Value,
    ) -> Result<Value, JsError> {
        let atom = self.intern_atom("prototype");
        let prototype = self.get_property(p, new_target, atom)?;
        Ok(if self.is_object_like(prototype) {
            prototype
        } else {
            default
        })
    }

    pub(super) fn temporal_calendar_projection_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match self.heap.get(this) {
            Some(Cell::TemporalPlainMonthDay {
                month,
                day,
                calendar,
                reference_iso_year,
                ..
            }) if is_plain_month_day_native(native) => {
                let (month, day, calendar, reference_iso_year) =
                    (*month, *day, calendar.clone(), *reference_iso_year);
                return self.temporal_plain_month_day_native(
                    p,
                    native,
                    args,
                    (month, day, calendar, reference_iso_year),
                );
            }
            Some(Cell::TemporalPlainYearMonth {
                year,
                month,
                calendar,
                reference_iso_day,
                ..
            }) if is_plain_year_month_native(native) => {
                let (year, month, calendar, reference_iso_day) =
                    (*year, *month, calendar.clone(), *reference_iso_day);
                return self.temporal_plain_year_month_native(
                    p,
                    native,
                    args,
                    (year, month, calendar, reference_iso_day),
                );
            }
            _ => {}
        }
        if matches!(
            native,
            Native::TemporalPlainMonthDayFrom | Native::TemporalPlainYearMonthFrom
        ) {
            return self.temporal_calendar_projection_from(p, native, this, args);
        }
        Err(self.type_error(
            p,
            "Temporal calendar method called on incompatible receiver".into(),
        ))
    }

    fn temporal_plain_month_day_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
        (month, day, calendar, _reference_year): (u32, u32, String, i32),
    ) -> Result<Value, JsError> {
        match native {
            Native::TemporalPlainMonthDayCalendarIdGetter => {
                Ok(self.heap.alloc(Cell::String(calendar.into())))
            }
            Native::TemporalPlainMonthDayDayGetter => Ok(Value::number(f64::from(day))),
            Native::TemporalPlainMonthDayMonthCodeGetter => Ok(self.heap.alloc(Cell::String(
                format!("M{month:0width$}", width = ISO_MONTH_CODE_DIGITS).into(),
            ))),
            Native::TemporalPlainMonthDayEquals => {
                let constructor = self.native_value(Native::TemporalPlainMonthDay);
                let other = self.temporal_plain_month_day_from(p, constructor, args)?;
                Ok(match self.heap.get(other) {
                    Some(Cell::TemporalPlainMonthDay {
                        month: other_month,
                        day: other_day,
                        calendar: other_calendar,
                        ..
                    }) if *other_month == month
                        && *other_day == day
                        && *other_calendar == calendar =>
                    {
                        Value::TRUE
                    }
                    _ => Value::FALSE,
                })
            }
            Native::TemporalPlainMonthDayToPlainDate => {
                let item = args.first().copied().unwrap_or(Value::UNDEFINED);
                if !self.is_object_like(item) {
                    return Err(self.type_error(p, "Invalid PlainDate fields".into()));
                }
                let year_atom = self.intern_atom("year");
                let year = self.get_property(p, item, year_atom)?;
                let year = self.plain_date_integer(p, year)?;
                let date = checked_iso_date(year, month as i32, day as i32)
                    .ok_or_else(|| self.range_error(p, "Invalid PlainDate".into()))?;
                Ok(self.heap.alloc(Cell::TemporalPlainDate {
                    object: Box::new(Self::empty_object(self.temporal_plain_date_proto)),
                    year: date.year,
                    month: date.month,
                    day: date.day,
                    calendar,
                }))
            }
            Native::TemporalPlainMonthDayValueOf => {
                Err(self.type_error(p, "Cannot convert PlainMonthDay to a number".into()))
            }
            _ => Err(self.type_error(p, "Temporal.PlainMonthDay method not implemented".into())),
        }
    }

    fn temporal_plain_year_month_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
        (year, month, calendar, reference_day): (i32, u32, String, u32),
    ) -> Result<Value, JsError> {
        match native {
            Native::TemporalPlainYearMonthCalendarIdGetter => {
                Ok(self.heap.alloc(Cell::String(calendar.into())))
            }
            Native::TemporalPlainYearMonthYearGetter => Ok(Value::number(f64::from(year))),
            Native::TemporalPlainYearMonthMonthGetter => Ok(Value::number(f64::from(month))),
            Native::TemporalPlainYearMonthMonthCodeGetter => Ok(self.heap.alloc(Cell::String(
                format!("M{month:0width$}", width = ISO_MONTH_CODE_DIGITS).into(),
            ))),
            Native::TemporalPlainYearMonthReferenceISODayGetter => {
                Ok(Value::number(f64::from(reference_day)))
            }
            Native::TemporalPlainYearMonthDaysInMonthGetter => Ok(Value::number(f64::from(
                iso_days_in_month(year, month as i32).unwrap_or(31),
            ))),
            Native::TemporalPlainYearMonthDaysInYearGetter => {
                Ok(Value::number(f64::from(iso_days_in_year(year))))
            }
            Native::TemporalPlainYearMonthMonthsInYearGetter => Ok(Value::number(f64::from(
                super::temporal_date::ISO_MONTHS_PER_YEAR,
            ))),
            Native::TemporalPlainYearMonthInLeapYearGetter => Ok(if iso_is_leap_year(year) {
                Value::TRUE
            } else {
                Value::FALSE
            }),
            Native::TemporalPlainYearMonthEquals => {
                let constructor = self.native_value(Native::TemporalPlainYearMonth);
                let other = self.temporal_plain_year_month_from(p, constructor, args)?;
                Ok(match self.heap.get(other) {
                    Some(Cell::TemporalPlainYearMonth {
                        year: other_year,
                        month: other_month,
                        calendar: other_calendar,
                        ..
                    }) if *other_year == year
                        && *other_month == month
                        && *other_calendar == calendar =>
                    {
                        Value::TRUE
                    }
                    _ => Value::FALSE,
                })
            }
            Native::TemporalPlainYearMonthAdd | Native::TemporalPlainYearMonthSubtract => {
                self.temporal_plain_year_month_add(p, native, args, year, month, calendar)
            }
            Native::TemporalPlainYearMonthWith => {
                self.temporal_plain_year_month_with(p, args, year, month, calendar, reference_day)
            }
            Native::TemporalPlainYearMonthToPlainDate => {
                let item = args.first().copied().unwrap_or(Value::UNDEFINED);
                if !self.is_object_like(item) {
                    return Err(self.type_error(p, "Invalid PlainDate fields".into()));
                }
                let day_atom = self.intern_atom("day");
                let day = self.get_property(p, item, day_atom)?;
                let day = self.plain_date_integer(p, day)?;
                let date = checked_iso_date(year, month as i32, day)
                    .ok_or_else(|| self.range_error(p, "Invalid PlainDate".into()))?;
                Ok(self.heap.alloc(Cell::TemporalPlainDate {
                    object: Box::new(Self::empty_object(self.temporal_plain_date_proto)),
                    year: date.year,
                    month: date.month,
                    day: date.day,
                    calendar,
                }))
            }
            Native::TemporalPlainYearMonthValueOf => {
                Err(self.type_error(p, "Cannot convert PlainYearMonth to a number".into()))
            }
            _ => Err(self.type_error(p, "Temporal.PlainYearMonth method not implemented".into())),
        }
    }

    fn temporal_plain_year_month_add(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
        year: i32,
        month: u32,
        calendar: String,
    ) -> Result<Value, JsError> {
        let mut duration =
            self.duration_record(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        self.validate_duration_fields(p, &duration)?;
        let options = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        self.plain_date_overflow(p, options)?;
        if duration[2..].iter().any(|field| *field != 0.0) {
            return Err(self.range_error(p, "Invalid duration".into()));
        }
        if native == Native::TemporalPlainYearMonthSubtract {
            duration[0] = -duration[0];
            duration[1] = -duration[1];
        }
        if checked_iso_date(year, month as i32, 1).is_none() {
            return Err(self.range_error(p, "Invalid PlainYearMonth".into()));
        }
        let delta = (duration[0] as i128) * i128::from(super::temporal_date::ISO_MONTHS_PER_YEAR)
            + duration[1] as i128;
        let start = super::temporal_date::IsoDate {
            year,
            month,
            day: 1,
        };
        let result = super::temporal_date::shift_iso_months(start, delta)
            .ok_or_else(|| self.range_error(p, "Invalid PlainYearMonth".into()))?;
        Ok(self.heap.alloc(Cell::TemporalPlainYearMonth {
            object: Box::new(Self::empty_object(self.temporal_plain_year_month_proto)),
            year: result.year,
            month: result.month,
            calendar,
            reference_iso_day: DEFAULT_REFERENCE_ISO_DAY,
        }))
    }

    fn temporal_plain_year_month_with(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        year: i32,
        month: u32,
        calendar: String,
        reference_day: u32,
    ) -> Result<Value, JsError> {
        let changes = args.first().copied().unwrap_or(Value::UNDEFINED);
        let (changed_year, changed_month, changed_code) =
            self.temporal_plain_year_month_change_fields(p, changes)?;
        if changed_month.is_some_and(|month| month <= 0) {
            return Err(self.range_error(p, "Invalid PlainYearMonth".into()));
        }
        let options = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let constrain = self.plain_date_overflow(p, options)?;
        if changed_year.is_none() && changed_month.is_none() && changed_code.is_none() {
            return Err(self.type_error(p, "Invalid fields".into()));
        }
        let year = changed_year.unwrap_or(year);
        let changed_code = changed_code
            .map(|code| self.heap.alloc(Cell::String(code.into())))
            .map(|code| self.plain_date_month_code(p, code))
            .transpose()?;
        let month = match (changed_month, changed_code) {
            (Some(month), Some(code)) if month != code => {
                return Err(self.range_error(p, "Conflicting month fields".into()));
            }
            (Some(month), _) => month,
            (None, Some(code)) => code,
            (None, None) => month as i32,
        };
        let month = if constrain {
            month.clamp(1, super::temporal_date::ISO_MONTHS_PER_YEAR)
        } else {
            month
        };
        let constructor = self.native_value(Native::TemporalPlainYearMonth);
        self.make_plain_year_month(p, constructor, year, month as u32, calendar, reference_day)
    }

    fn temporal_plain_year_month_change_fields(
        &mut self,
        p: &ResidualProgram,
        changes: Value,
    ) -> Result<(Option<i32>, Option<i32>, Option<String>), JsError> {
        self.validate_plain_year_month_changes(p, changes)?;
        let month_atom = self.intern_atom("month");
        let month_value = self.get_property(p, changes, month_atom)?;
        let month = self.plain_date_optional_integer(p, month_value)?;
        let month_code_atom = self.intern_atom("monthCode");
        let month_code = self.get_property(p, changes, month_code_atom)?;
        let month_code = if month_code.is_undefined() {
            None
        } else {
            Some(self.to_string(p, month_code)?.to_string())
        };
        let year_atom = self.intern_atom("year");
        let year_value = self.get_property(p, changes, year_atom)?;
        let year = self.plain_date_optional_integer(p, year_value)?;
        Ok((year, month, month_code))
    }

    fn validate_plain_year_month_changes(
        &mut self,
        p: &ResidualProgram,
        changes: Value,
    ) -> Result<(), JsError> {
        if !self.is_object_like(changes)
            || matches!(
                self.heap.get(changes),
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
            return Err(self.type_error(p, "Invalid fields".into()));
        }
        let calendar_atom = self.intern_atom("calendar");
        let calendar = self.get_property(p, changes, calendar_atom)?;
        let time_zone_atom = self.intern_atom("timeZone");
        let time_zone = self.get_property(p, changes, time_zone_atom)?;
        if !calendar.is_undefined() || !time_zone.is_undefined() {
            return Err(self.type_error(p, "Invalid fields".into()));
        }
        Ok(())
    }

    fn temporal_calendar_projection_from(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        constructor: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::TemporalPlainMonthDayFrom => {
                self.temporal_plain_month_day_from(p, constructor, args)
            }
            Native::TemporalPlainYearMonthFrom => {
                self.temporal_plain_year_month_from(p, constructor, args)
            }
            _ => unreachable!("not a calendar projection from method"),
        }
    }

    fn temporal_plain_month_day_from(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let options = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        if let Some(Cell::TemporalPlainMonthDay {
            month,
            day,
            calendar,
            ..
        }) = self.heap.get(value)
        {
            let (month, day, calendar) = (*month, *day, calendar.clone());
            let _ = self.plain_date_overflow(p, options)?;
            return self.make_plain_month_day(p, constructor, month, day, calendar);
        }
        if let Some(Cell::String(text)) = self.heap.get(value) {
            let text = text.host_string().to_owned();
            let (date, calendar) = parse_plain_month_day_string(self, p, &text)?;
            let _ = self.plain_date_overflow(p, options)?;
            return self.make_plain_month_day(p, constructor, date.month, date.day, calendar);
        }
        if !self.is_object_like(value) {
            return Err(self.type_error(p, "Invalid PlainMonthDay".into()));
        }
        if matches!(
            self.heap.get(value),
            Some(
                Cell::TemporalPlainDate { .. }
                    | Cell::TemporalPlainDateTime { .. }
                    | Cell::TemporalZonedDateTime { .. }
            )
        ) {
            let plain_date_constructor = self.native_value(Native::TemporalPlainDate);
            let date =
                self.temporal_plain_date_from(p, plain_date_constructor, &[value, options])?;
            let (year, month, day, calendar) = self.temporal_plain_date_slots(p, date)?;
            let _ = year;
            return self.make_plain_month_day(p, constructor, month, day, calendar);
        }
        self.temporal_plain_month_day_from_bag(p, constructor, value, options)
    }

    fn temporal_plain_month_day_from_bag(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
        bag: Value,
        options: Value,
    ) -> Result<Value, JsError> {
        let calendar_atom = self.intern_atom("calendar");
        let calendar_value = self.get_property(p, bag, calendar_atom)?;
        let calendar = self.temporal_calendar_property(p, calendar_value)?;
        let day_atom = self.intern_atom("day");
        let day = self.get_property(p, bag, day_atom)?;
        if day.is_undefined() {
            return Err(self.type_error(p, "Missing day".into()));
        }
        let day = self.plain_date_integer(p, day)?;
        let month_atom = self.intern_atom("month");
        let month_value = self.get_property(p, bag, month_atom)?;
        let month = self.plain_date_optional_integer(p, month_value)?;
        let month_code_atom = self.intern_atom("monthCode");
        let month_code = self.get_property(p, bag, month_code_atom)?;
        let month_code = if month_code.is_undefined() {
            None
        } else {
            Some(self.plain_date_month_code(p, month_code)?)
        };
        let month = match (month, month_code) {
            (Some(month), Some(code)) if month != code => {
                return Err(self.range_error(p, "month and monthCode must agree".into()));
            }
            (Some(month), _) => month,
            (None, Some(code)) => code,
            (None, None) => return Err(self.type_error(p, "Missing month".into())),
        };
        let year_atom = self.intern_atom("year");
        let year_value = self.get_property(p, bag, year_atom)?;
        let year = if year_value.is_undefined() {
            DEFAULT_REFERENCE_ISO_YEAR
        } else {
            self.plain_date_integer(p, year_value)?
        };
        let constrain = self.plain_date_overflow(p, options)?;
        if month < 1 || day < 1 {
            return Err(self.range_error(p, "Invalid PlainMonthDay".into()));
        }
        let month = if constrain {
            month.clamp(1, super::temporal_date::ISO_MONTHS_PER_YEAR)
        } else {
            month
        };
        let max_day = iso_days_in_month(year, month).unwrap_or(31);
        let day = if constrain { day.min(max_day) } else { day };
        checked_iso_date(year, month, day)
            .ok_or_else(|| self.range_error(p, "Invalid PlainMonthDay".into()))?;
        self.make_plain_month_day(p, constructor, month as u32, day as u32, calendar)
    }

    fn temporal_plain_year_month_from(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let options = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        if let Some(Cell::TemporalPlainYearMonth {
            year,
            month,
            calendar,
            reference_iso_day,
            ..
        }) = self.heap.get(value)
        {
            let (year, month, calendar, day) =
                (*year, *month, calendar.clone(), *reference_iso_day);
            let _ = self.plain_date_overflow(p, options)?;
            return self.make_plain_year_month(p, constructor, year, month, calendar, day);
        }
        if let Some(Cell::String(text)) = self.heap.get(value) {
            let text = text.host_string().to_owned();
            let (date, calendar) = parse_plain_year_month_string(self, p, &text)?;
            let _ = self.plain_date_overflow(p, options)?;
            return self.make_plain_year_month(
                p,
                constructor,
                date.year,
                date.month,
                calendar,
                DEFAULT_REFERENCE_ISO_DAY,
            );
        }
        self.temporal_plain_year_month_from_bag(p, constructor, value, options)
    }

    fn temporal_plain_year_month_from_bag(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
        bag: Value,
        options: Value,
    ) -> Result<Value, JsError> {
        if !self.is_object_like(bag) {
            return Err(self.type_error(p, "Invalid PlainYearMonth".into()));
        }
        let (calendar, year, month) = self.temporal_plain_year_month_fields(p, bag)?;
        let constrain = self.plain_date_overflow(p, options)?;
        let month = match month {
            Some(month) => month,
            None => return Err(self.type_error(p, "Missing month".into())),
        };
        let month = if constrain {
            month.clamp(1, super::temporal_date::ISO_MONTHS_PER_YEAR)
        } else {
            month
        };
        self.make_plain_year_month(
            p,
            constructor,
            year,
            month as u32,
            calendar,
            DEFAULT_REFERENCE_ISO_DAY,
        )
    }

    fn temporal_plain_year_month_fields(
        &mut self,
        p: &ResidualProgram,
        bag: Value,
    ) -> Result<(String, i32, Option<i32>), JsError> {
        let calendar_atom = self.intern_atom("calendar");
        let calendar_value = self.get_property(p, bag, calendar_atom)?;
        let calendar = self.temporal_calendar_property(p, calendar_value)?;
        let month_atom = self.intern_atom("month");
        let month_value = self.get_property(p, bag, month_atom)?;
        let month = self.plain_date_optional_integer(p, month_value)?;
        let month_code_atom = self.intern_atom("monthCode");
        let month_code_value = self.get_property(p, bag, month_code_atom)?;
        let month_code = if month_code_value.is_undefined() {
            None
        } else {
            Some(self.plain_date_month_code(p, month_code_value)?)
        };
        let year_atom = self.intern_atom("year");
        let year_value = self.get_property(p, bag, year_atom)?;
        let year = self
            .plain_date_optional_integer(p, year_value)?
            .ok_or_else(|| self.type_error(p, "Missing year".into()))?;
        let month = match (month, month_code) {
            (Some(month), Some(code)) if month != code => {
                return Err(self.range_error(p, "Conflicting month fields".into()));
            }
            (Some(month), _) => Some(month),
            (None, code) => code,
        };
        Ok((calendar, year, month))
    }

    fn make_plain_month_day(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
        month: u32,
        day: u32,
        calendar: String,
    ) -> Result<Value, JsError> {
        let args = [
            Value::number(f64::from(month)),
            Value::number(f64::from(day)),
            self.heap.alloc(Cell::String(calendar.into())),
            Value::number(f64::from(DEFAULT_REFERENCE_ISO_YEAR)),
        ];
        self.temporal_plain_month_day_construct(p, &args, constructor)
    }

    fn make_plain_year_month(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
        year: i32,
        month: u32,
        calendar: String,
        day: u32,
    ) -> Result<Value, JsError> {
        let reference = if day == DEFAULT_REFERENCE_ISO_DAY {
            Value::UNDEFINED
        } else {
            Value::number(f64::from(day))
        };
        let args = [
            Value::number(f64::from(year)),
            Value::number(f64::from(month)),
            self.heap.alloc(Cell::String(calendar.into())),
            reference,
        ];
        self.temporal_plain_year_month_construct(p, &args, constructor)
    }

    fn temporal_calendar_property(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<String, JsError> {
        match self.heap.get(value) {
            None if value.is_undefined() => Ok("iso8601".into()),
            Some(Cell::String(_)) => {
                let text = self.to_string(p, value)?.to_string();
                temporal_date_parse::parse_calendar_identifier(&text)
                    .ok_or_else(|| self.range_error(p, "Invalid calendar".into()))
            }
            Some(Cell::TemporalPlainDate { calendar, .. })
            | Some(Cell::TemporalPlainDateTime { calendar, .. })
            | Some(Cell::TemporalPlainMonthDay { calendar, .. })
            | Some(Cell::TemporalPlainYearMonth { calendar, .. })
            | Some(Cell::TemporalZonedDateTime { calendar, .. }) => Ok(calendar.clone()),
            _ => Err(self.type_error(p, "Invalid calendar".into())),
        }
    }
}

fn is_plain_month_day_native(native: Native) -> bool {
    matches!(
        native,
        Native::TemporalPlainMonthDayCalendarIdGetter
            | Native::TemporalPlainMonthDayDayGetter
            | Native::TemporalPlainMonthDayMonthCodeGetter
            | Native::TemporalPlainMonthDayEquals
            | Native::TemporalPlainMonthDayToPlainDate
            | Native::TemporalPlainMonthDayWith
            | Native::TemporalPlainMonthDayValueOf
    )
}

fn is_plain_year_month_native(native: Native) -> bool {
    matches!(
        native,
        Native::TemporalPlainYearMonthCalendarIdGetter
            | Native::TemporalPlainYearMonthYearGetter
            | Native::TemporalPlainYearMonthMonthGetter
            | Native::TemporalPlainYearMonthMonthCodeGetter
            | Native::TemporalPlainYearMonthReferenceISODayGetter
            | Native::TemporalPlainYearMonthDaysInMonthGetter
            | Native::TemporalPlainYearMonthDaysInYearGetter
            | Native::TemporalPlainYearMonthMonthsInYearGetter
            | Native::TemporalPlainYearMonthInLeapYearGetter
            | Native::TemporalPlainYearMonthEquals
            | Native::TemporalPlainYearMonthAdd
            | Native::TemporalPlainYearMonthSubtract
            | Native::TemporalPlainYearMonthUntil
            | Native::TemporalPlainYearMonthSince
            | Native::TemporalPlainYearMonthWith
            | Native::TemporalPlainYearMonthValueOf
            | Native::TemporalPlainYearMonthToPlainDate
    )
}

fn parse_plain_month_day_string<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    text: &str,
) -> Result<(super::temporal_date::IsoDate, String), JsError> {
    if text.contains(['\u{2212}', 'Z', 'z']) || text.starts_with("-000000") {
        return Err(vm.range_error(p, "Invalid PlainMonthDay".into()));
    }
    if let Some(date) = temporal_date_parse::parse_plain_date_string(text) {
        return Ok(date);
    }
    let (local, _, _) = super::temporal_zoned_date_time::parse_iso_zoned_base(text)
        .ok_or_else(|| vm.range_error(p, "Invalid PlainMonthDay".into()))?;
    use chrono::Datelike;
    let date = checked_iso_date(local.year(), local.month() as i32, local.day() as i32)
        .ok_or_else(|| vm.range_error(p, "Invalid PlainMonthDay".into()))?;
    let calendar = temporal_date_parse::parse_calendar_identifier(text)
        .ok_or_else(|| vm.range_error(p, "Invalid calendar".into()))?;
    Ok((date, calendar))
}

fn parse_plain_year_month_string<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    text: &str,
) -> Result<(super::temporal_date::IsoDate, String), JsError> {
    if let Some(date) = temporal_date_parse::parse_plain_date_string(text) {
        return Ok(date);
    }
    let annotation = text.find('[').unwrap_or(text.len());
    let (base, suffix) = text.split_at(annotation);
    let Some((year, month)) = base.rsplit_once('-') else {
        return Err(vm.range_error(p, "Invalid PlainYearMonth".into()));
    };
    let extended = year.starts_with(['+', '-']);
    let year_digits = year.strip_prefix(['+', '-']).unwrap_or(year);
    let year_valid = (4..=6).contains(&year_digits.len())
        && year_digits.bytes().all(|byte| byte.is_ascii_digit());
    let month_valid = month.len() == 2 && month.bytes().all(|byte| byte.is_ascii_digit());
    if !year_valid || !month_valid || (extended && year_digits.len() != 6) {
        return Err(vm.range_error(p, "Invalid PlainYearMonth".into()));
    }
    let padded = format!("{base}-01{suffix}");
    if let Some(parsed) = temporal_date_parse::parse_plain_date_string(&padded) {
        return Ok(parsed);
    }
    parse_plain_year_month_boundary(vm, p, text, year, month, suffix)
}

fn parse_plain_year_month_boundary<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    text: &str,
    year: &str,
    month: &str,
    suffix: &str,
) -> Result<(super::temporal_date::IsoDate, String), JsError> {
    let year = year
        .parse::<i32>()
        .map_err(|_| vm.range_error(p, "Invalid PlainYearMonth".into()))?;
    let month = month
        .parse::<u32>()
        .map_err(|_| vm.range_error(p, "Invalid PlainYearMonth".into()))?;
    let lower_edge = year == super::temporal_date::MIN_ISO_YEAR && month == 4;
    let upper_edge = year == super::temporal_date::MAX_ISO_YEAR && month == 9;
    if !lower_edge && !upper_edge {
        return Err(vm.range_error(p, "Invalid PlainYearMonth".into()));
    }
    let calendar = if suffix.is_empty() {
        "iso8601".to_owned()
    } else {
        temporal_date_parse::parse_calendar_identifier(text)
            .ok_or_else(|| vm.range_error(p, "Invalid PlainYearMonth".into()))?
    };
    Ok((
        super::temporal_date::IsoDate {
            year,
            month,
            day: DEFAULT_REFERENCE_ISO_DAY,
        },
        calendar,
    ))
}

pub(super) fn to_plain_month_day<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    this: Value,
) -> Result<Value, JsError> {
    let (_year, month, day, calendar) = vm.temporal_plain_date_slots(p, this)?;
    Ok(vm.heap.alloc(Cell::TemporalPlainMonthDay {
        object: Box::new(Vm::<H>::empty_object(vm.temporal_plain_month_day_proto)),
        month,
        day,
        calendar,
        reference_iso_year: DEFAULT_REFERENCE_ISO_YEAR,
    }))
}

pub(super) fn to_plain_year_month<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    this: Value,
) -> Result<Value, JsError> {
    let (year, month, _day, calendar) = vm.temporal_plain_date_slots(p, this)?;
    Ok(vm.heap.alloc(Cell::TemporalPlainYearMonth {
        object: Box::new(Vm::<H>::empty_object(vm.temporal_plain_year_month_proto)),
        year,
        month,
        calendar,
        reference_iso_day: DEFAULT_REFERENCE_ISO_DAY,
    }))
}

pub(super) fn to_zoned_date_time<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    this: Value,
    args: &[Value],
) -> Result<Value, JsError> {
    let (year, month, day, calendar) = vm.temporal_plain_date_slots(p, this)?;
    let item = args.first().copied().unwrap_or(Value::UNDEFINED);
    let (time_zone_value, time_value) = if vm.is_object_like(item) {
        let time_zone_atom = vm.intern_atom("timeZone");
        let time_zone = vm.get_property(p, item, time_zone_atom)?;
        if time_zone.is_undefined() {
            return Err(vm.type_error(p, "Invalid time zone".into()));
        }
        let plain_time_atom = vm.intern_atom("plainTime");
        let plain_time = vm.get_property(p, item, plain_time_atom)?;
        (time_zone, plain_time)
    } else {
        (item, Value::UNDEFINED)
    };
    if time_zone_value.is_undefined() {
        return Err(vm.type_error(p, "Invalid time zone".into()));
    }
    let time_zone = vm.temporal_timezone_id(p, time_zone_value)?;
    let time = if time_value.is_undefined() {
        [0; 6]
    } else {
        super::temporal_plain_date_time_conversion::to_time(vm, p, time_value)?
    };
    let iso_date = super::temporal_date::IsoDate { year, month, day };
    let local_midnight = i128::from(super::temporal_date::days_from_iso_date(iso_date))
        * super::temporal_zoned_date_time::NANOSECONDS_PER_DAY;
    let local_time = time
        .iter()
        .zip(TIME_NANOSECONDS)
        .map(|(field, scale)| i128::from(*field) * scale)
        .sum::<i128>();
    let local_epoch = local_midnight + local_time;
    let offset =
        super::temporal_zoned_date_time::timezone_offset_nanoseconds(&time_zone, local_epoch)
            .ok_or_else(|| vm.range_error(p, "Invalid time zone".into()))?;
    let epoch_nanoseconds = local_epoch - offset;
    if epoch_nanoseconds.unsigned_abs()
        > super::temporal_zoned_date_time::MAX_EPOCH_NANOSECONDS as u128
    {
        return Err(vm.range_error(p, "Invalid instant".into()));
    }
    let temporal_atom = vm.intern_atom("Temporal");
    let temporal = vm.get_property(p, vm.realm.globals, temporal_atom)?;
    let constructor_atom = vm.intern_atom("ZonedDateTime");
    let constructor = vm.get_property(p, temporal, constructor_atom)?;
    let args = [
        vm.heap.alloc(Cell::BigInt(epoch_nanoseconds.to_string())),
        vm.heap.alloc(Cell::String(time_zone.into())),
        vm.heap.alloc(Cell::String(calendar.into())),
    ];
    let result = vm.temporal_zoned_date_time_construct(p, &args, constructor)?;
    Ok(result)
}

const TIME_NANOSECONDS: [i128; 6] = [
    3_600_000_000_000,
    60_000_000_000,
    1_000_000_000,
    1_000_000,
    1_000,
    1,
];

pub(super) fn native<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    native: Native,
    this: Value,
    args: &[Value],
) -> Result<Value, JsError> {
    let options = match native {
        Native::TemporalPlainMonthDayToString | Native::TemporalPlainYearMonthToString => {
            args.first().copied().unwrap_or(Value::UNDEFINED)
        }
        Native::TemporalPlainMonthDayToLocaleString
        | Native::TemporalPlainYearMonthToLocaleString => {
            args.get(1).copied().unwrap_or(Value::UNDEFINED)
        }
        _ => Value::UNDEFINED,
    };
    if let Some(Cell::TemporalPlainMonthDay {
        month,
        day,
        calendar,
        reference_iso_year,
        ..
    }) = vm.heap.get(this)
    {
        let (month, day, calendar, year) = (*month, *day, calendar.clone(), *reference_iso_year);
        let text = vm.temporal_plain_date_to_string(p, year, month, day, &calendar, options)?;
        let text = if calendar == "iso8601" && !text.contains('[') {
            format!("{month:02}-{day:02}")
        } else {
            text
        };
        return Ok(vm.heap.alloc(Cell::String(text.into())));
    }
    if let Some(Cell::TemporalPlainYearMonth {
        year,
        month,
        calendar,
        reference_iso_day,
        ..
    }) = vm.heap.get(this)
    {
        let (year, month, calendar, day) = (*year, *month, calendar.clone(), *reference_iso_day);
        let text = vm.temporal_plain_date_to_string(p, year, month, day, &calendar, options)?;
        let (date, annotation) = text.split_once('[').unwrap_or((&text, ""));
        let month_end = if annotation.is_empty() {
            date.rfind('-').unwrap_or(date.len())
        } else {
            date.len()
        };
        return Ok(vm.heap.alloc(Cell::String(
            format!("{}{annotation}", &date[..month_end]).into(),
        )));
    }
    Err(vm.type_error(p, "Invalid Temporal calendar object".into()))
}
