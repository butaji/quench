use super::temporal_date::{
    ISO_MONTH_CODE_DIGITS, checked_iso_date, iso_days_in_month, iso_days_in_year, iso_is_leap_year,
};
use super::*;

const DEFAULT_REFERENCE_ISO_YEAR: i32 = 1972;
const DEFAULT_REFERENCE_ISO_DAY: u32 = 1;
const MIN_SUPPORTED_ISO_MONTH: i32 = 4;
const MAX_SUPPORTED_ISO_MONTH: i32 = 9;
const BASIC_ISO_YEAR_MONTH_LENGTH: usize = 6;
const BASIC_ISO_DATE_LENGTH: usize = BASIC_ISO_YEAR_MONTH_LENGTH + 2;
const SIGNED_COMPACT_YEAR_MONTH_LENGTH: usize = 9;
const SIGNED_COMPACT_DATE_LENGTH: usize = SIGNED_COMPACT_YEAR_MONTH_LENGTH + 2;
const BASIC_ISO_YEAR_LENGTH: usize = 4;
const EXTENDED_ISO_YEAR_LENGTH: usize = 7;
const BASIC_ISO_TIME_MINUTES_LENGTH: usize = 4;
const BASIC_ISO_TIME_SECONDS_LENGTH: usize = 6;
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
    ("era", Native::TemporalPlainYearMonthEraGetter),
    ("eraYear", Native::TemporalPlainYearMonthEraYearGetter),
    (
        "referenceISODay",
        Native::TemporalPlainYearMonthReferenceISODayGetter,
    ),
    (
        "daysInMonth",
        Native::TemporalPlainYearMonthDaysInMonthGetter,
    ),
    ("daysInYear", Native::TemporalPlainYearMonthDaysInYearGetter),
    (
        "monthsInYear",
        Native::TemporalPlainYearMonthMonthsInYearGetter,
    ),
    ("inLeapYear", Native::TemporalPlainYearMonthInLeapYearGetter),
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
        let reference = if reference_value.is_undefined() {
            DEFAULT_REFERENCE_ISO_DAY
        } else {
            self.plain_date_integer(p, reference_value)? as u32
        };
        self.validate_plain_year_month_range(p, year, month, reference)?;
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
    ) -> Result<(), JsError> {
        let in_year_range = (super::temporal_date::MIN_ISO_YEAR
            ..=super::temporal_date::MAX_ISO_YEAR)
            .contains(&year);
        let in_month_range = (1..=super::temporal_date::ISO_MONTHS_PER_YEAR).contains(&month);
        let in_year_month_range = (year, month)
            >= (super::temporal_date::MIN_ISO_YEAR, MIN_SUPPORTED_ISO_MONTH)
            && (year, month) <= (super::temporal_date::MAX_ISO_YEAR, MAX_SUPPORTED_ISO_MONTH);
        let valid_reference_day =
            iso_days_in_month(year, month).is_some_and(|days| (1..=days as u32).contains(&day));
        if !in_year_range || !in_month_range || !in_year_month_range || !valid_reference_day {
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
        temporal_date_parse::parse_calendar_identifier_name(&text)
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
        if matches!(
            native,
            Native::TemporalPlainMonthDayCompare | Native::TemporalPlainYearMonthCompare
        ) {
            return self.temporal_calendar_projection_compare(p, native, args);
        }
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
            return self.temporal_calendar_projection_from(p, native, args);
        }
        Err(self.type_error(
            p,
            "Temporal calendar method called on incompatible receiver".into(),
        ))
    }

    fn temporal_calendar_projection_compare(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let from_native = match native {
            Native::TemporalPlainMonthDayCompare => Native::TemporalPlainMonthDayFrom,
            Native::TemporalPlainYearMonthCompare => Native::TemporalPlainYearMonthFrom,
            _ => unreachable!("not a Temporal calendar compare native"),
        };
        let left = self.temporal_calendar_projection_from(
            p,
            from_native,
            &[args.first().copied().unwrap_or(Value::UNDEFINED)],
        )?;
        let right = self.temporal_calendar_projection_from(
            p,
            from_native,
            &[args.get(1).copied().unwrap_or(Value::UNDEFINED)],
        )?;
        self.temporal_calendar_projection_order(left, right, native)
    }

    fn temporal_calendar_projection_order(
        &mut self,
        left: Value,
        right: Value,
        native: Native,
    ) -> Result<Value, JsError> {
        let ordering = match (native, self.heap.get(left), self.heap.get(right)) {
            (
                Native::TemporalPlainMonthDayCompare,
                Some(Cell::TemporalPlainMonthDay {
                    month: lm, day: ld, ..
                }),
                Some(Cell::TemporalPlainMonthDay {
                    month: rm, day: rd, ..
                }),
            ) => (lm, ld).cmp(&(rm, rd)),
            (
                Native::TemporalPlainYearMonthCompare,
                Some(Cell::TemporalPlainYearMonth {
                    year: ly,
                    month: lm,
                    reference_iso_day: ld,
                    ..
                }),
                Some(Cell::TemporalPlainYearMonth {
                    year: ry,
                    month: rm,
                    reference_iso_day: rd,
                    ..
                }),
            ) => (ly, lm, ld).cmp(&(ry, rm, rd)),
            _ => unreachable!("calendar comparison operands are not branded"),
        };
        Ok(Value::number(match ordering {
            std::cmp::Ordering::Less => -1.0,
            std::cmp::Ordering::Equal => 0.0,
            std::cmp::Ordering::Greater => 1.0,
        }))
    }

    fn temporal_plain_month_day_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
        (month, day, calendar, reference_year): (u32, u32, String, i32),
    ) -> Result<Value, JsError> {
        let calendar_fields =
            quench_intl::calendar_fields_from_iso(reference_year, month, day, &calendar);
        match native {
            Native::TemporalPlainMonthDayCalendarIdGetter => {
                Ok(self.heap.alloc(Cell::String(calendar.into())))
            }
            Native::TemporalPlainMonthDayDayGetter => Ok(Value::number(f64::from(
                calendar_fields.as_ref().map_or(day, |fields| fields.day),
            ))),
            Native::TemporalPlainMonthDayMonthCodeGetter => Ok(self.heap.alloc(Cell::String(
                calendar_fields.map_or_else(
                    || format!("M{month:0width$}", width = ISO_MONTH_CODE_DIGITS),
                    |fields| fields.month_code,
                )
                .into(),
            ))),
            Native::TemporalPlainMonthDayEquals => {
                let constructor = self.native_value(Native::TemporalPlainMonthDay);
                let other = self.temporal_plain_month_day_from(p, constructor, args)?;
                Ok(match self.heap.get(other) {
                    Some(Cell::TemporalPlainMonthDay {
                        month: other_month,
                        day: other_day,
                        calendar: other_calendar,
                        reference_iso_year: other_year,
                        ..
                    }) if *other_month == month
                        && *other_day == day
                        && *other_calendar == calendar
                        && *other_year == reference_year =>
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
                if year.is_undefined() {
                    return Err(self.type_error(p, "Missing year".into()));
                }
                let year = self.plain_date_integer(p, year)?;
                let day =
                    day.min(iso_days_in_month(year, month as i32).unwrap_or(day as i32) as u32);
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
            Native::TemporalPlainMonthDayWith => {
                self.temporal_plain_month_day_with(p, args, month, day, calendar, reference_year)
            }
            Native::TemporalPlainMonthDayValueOf => {
                Err(self.type_error(p, "Cannot convert PlainMonthDay to a number".into()))
            }
            _ => Err(self.type_error(p, "Temporal.PlainMonthDay method not implemented".into())),
        }
    }

    fn temporal_plain_month_day_with(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        original_month: u32,
        original_day: u32,
        calendar: String,
        reference_year: i32,
    ) -> Result<Value, JsError> {
        let changes = args.first().copied().unwrap_or(Value::UNDEFINED);
        self.validate_plain_month_day_changes(p, changes)?;

        let day_atom = self.intern_atom("day");
        let day_value = self.get_property(p, changes, day_atom)?;
        let day = if day_value.is_undefined() {
            original_day as i32
        } else {
            self.plain_date_integer(p, day_value)?
        };
        if day < 1 {
            return Err(self.range_error(p, "Invalid PlainMonthDay".into()));
        }

        let month_atom = self.intern_atom("month");
        let month_value = self.get_property(p, changes, month_atom)?;
        let month = self.plain_date_optional_integer(p, month_value)?;
        let month_code_atom = self.intern_atom("monthCode");
        let month_code_value = self.get_property(p, changes, month_code_atom)?;
        let month_code = if month_code_value.is_undefined() {
            None
        } else {
            Some(self.to_string(p, month_code_value)?.to_string())
        };
        if calendar != "iso8601" && month.is_some() && month_code.is_none() {
            return Err(self.type_error(p, "Missing monthCode".into()));
        }
        let year_atom = self.intern_atom("year");
        let year_value = self.get_property(p, changes, year_atom)?;
        let year = if year_value.is_undefined() {
            reference_year
        } else {
            self.plain_date_integer(p, year_value)?
        };
        let options = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let constrain = self.plain_date_overflow(p, options)?;
        let month_code = month_code
            .map(|code| {
                super::temporal_date::parse_iso_month_code(&code)
                    .ok_or_else(|| self.range_error(p, "Invalid monthCode".into()))
            })
            .transpose()?;

        if month.is_none()
            && month_code.is_none()
            && year_value.is_undefined()
            && day_value.is_undefined()
        {
            return Err(self.type_error(p, "Invalid fields".into()));
        }
        let month = match (month, month_code) {
            (Some(month), Some(code)) if month != code => {
                return Err(self.range_error(p, "Conflicting month fields".into()));
            }
            (Some(month), _) => month,
            (None, Some(code)) => code,
            (None, None) => original_month as i32,
        };
        if month <= 0 {
            return Err(self.range_error(p, "Invalid PlainMonthDay".into()));
        }
        let month = if constrain {
            month.min(super::temporal_date::ISO_MONTHS_PER_YEAR)
        } else {
            month
        };
        let days_in_month = iso_days_in_month(year, month)
            .ok_or_else(|| self.range_error(p, "Invalid PlainMonthDay".into()))?;
        let day = if constrain {
            day.min(days_in_month)
        } else {
            if day > days_in_month {
                return Err(self.range_error(p, "Invalid PlainMonthDay".into()));
            }
            day
        };
        let constructor = self.native_value(Native::TemporalPlainMonthDay);
        let calendar = self.heap.alloc(Cell::String(calendar.into()));
        let args = [
            Value::number(f64::from(month)),
            Value::number(f64::from(day)),
            calendar,
            Value::number(f64::from(reference_year)),
        ];
        self.temporal_plain_month_day_construct(p, &args, constructor)
    }

    fn validate_plain_month_day_changes(
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

    fn temporal_plain_year_month_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
        (year, month, calendar, reference_day): (i32, u32, String, u32),
    ) -> Result<Value, JsError> {
        let calendar_fields = quench_intl::calendar_fields_from_iso(
            year,
            month,
            reference_day,
            &calendar,
        );
        match native {
            Native::TemporalPlainYearMonthCalendarIdGetter => {
                Ok(self.heap.alloc(Cell::String(calendar.into())))
            }
            Native::TemporalPlainYearMonthYearGetter => Ok(Value::number(f64::from(
                calendar_fields.as_ref().map_or(year, |fields| fields.year),
            ))),
            Native::TemporalPlainYearMonthMonthGetter => Ok(Value::number(f64::from(
                calendar_fields
                    .as_ref()
                    .map_or(month as u32, |fields| fields.month),
            ))),
            Native::TemporalPlainYearMonthMonthCodeGetter => Ok(self.heap.alloc(Cell::String(
                calendar_fields.map_or_else(
                    || format!("M{month:0width$}", width = ISO_MONTH_CODE_DIGITS),
                    |fields| fields.month_code,
                ).into(),
            ))),
            Native::TemporalPlainYearMonthEraGetter => Ok(calendar_fields
                .and_then(|fields| fields.era)
                .map_or(Value::UNDEFINED, |era| self.heap.alloc(Cell::String(era.into())))),
            Native::TemporalPlainYearMonthEraYearGetter => Ok(calendar_fields
                .and_then(|fields| fields.era_year)
                .map_or(Value::UNDEFINED, |year| Value::number(f64::from(year)))),
            Native::TemporalPlainYearMonthReferenceISODayGetter => {
                Ok(Value::number(f64::from(reference_day)))
            }
            Native::TemporalPlainYearMonthDaysInMonthGetter => Ok(Value::number(f64::from(
                calendar_fields.as_ref().map_or_else(
                    || iso_days_in_month(year, month as i32).unwrap_or(31) as u32,
                    |fields| fields.days_in_month,
                ),
            ))),
            Native::TemporalPlainYearMonthDaysInYearGetter => Ok(Value::number(f64::from(
                calendar_fields.as_ref().map_or_else(
                    || iso_days_in_year(year),
                    |fields| fields.days_in_year,
                ),
            ))),
            Native::TemporalPlainYearMonthMonthsInYearGetter => Ok(Value::number(f64::from(
                calendar_fields.as_ref().map_or(
                    super::temporal_date::ISO_MONTHS_PER_YEAR as u32,
                    |fields| fields.months_in_year,
                ),
            ))),
            Native::TemporalPlainYearMonthInLeapYearGetter => Ok(if calendar_fields
                .map_or_else(|| iso_is_leap_year(year), |fields| fields.is_leap_year)
            {
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
                        reference_iso_day: other_reference_day,
                        ..
                    }) if *other_year == year
                        && *other_month == month
                        && *other_calendar == calendar
                        && *other_reference_day == reference_day =>
                    {
                        Value::TRUE
                    }
                    _ => Value::FALSE,
                })
            }
            Native::TemporalPlainYearMonthAdd | Native::TemporalPlainYearMonthSubtract => {
                self.temporal_plain_year_month_add(
                    p,
                    native,
                    args,
                    year,
                    month,
                    reference_day,
                    calendar,
                )
            }
            Native::TemporalPlainYearMonthWith => {
                self.temporal_plain_year_month_with(
                    p,
                    args,
                    calendar_fields.as_ref().map_or(year, |fields| fields.year),
                    calendar_fields
                        .as_ref()
                        .map_or(month, |fields| fields.month),
                    calendar_fields.as_ref().map_or_else(
                        || format!("M{month:02}"),
                        |fields| fields.month_code.clone(),
                    ),
                    calendar,
                )
            }
            Native::TemporalPlainYearMonthUntil | Native::TemporalPlainYearMonthSince => self
                .temporal_plain_year_month_difference_native(
                    p,
                    native,
                    args,
                    year,
                    month,
                    reference_day,
                    calendar,
                ),
            Native::TemporalPlainYearMonthToPlainDate => {
                let item = args.first().copied().unwrap_or(Value::UNDEFINED);
                if !self.is_object_like(item) {
                    return Err(self.type_error(p, "Invalid PlainDate fields".into()));
                }
                let day_atom = self.intern_atom("day");
                let day_value = self.get_property(p, item, day_atom)?;
                if day_value.is_undefined() {
                    return Err(self.type_error(p, "Missing day".into()));
                }
                let day = self.plain_date_integer(p, day_value)?;
                if day < 1 {
                    return Err(self.range_error(p, "Invalid PlainDate".into()));
                }
                let fields = quench_intl::calendar_fields_from_iso(
                    year,
                    month,
                    reference_day,
                    &calendar,
                );
                let (calendar_year, calendar_month) = fields.map_or((year, month), |fields| {
                    (fields.year, fields.month)
                });
                let iso = quench_intl::calendar_date_to_iso_with_overflow(
                    calendar_year,
                    calendar_month,
                    day as u32,
                    &calendar,
                    true,
                )
                .ok_or_else(|| self.range_error(p, "Invalid PlainDate".into()))?;
                let date = checked_iso_date(iso.0, iso.1 as i32, iso.2 as i32)
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
        reference_day: u32,
        calendar: String,
    ) -> Result<Value, JsError> {
        let mut duration =
            self.duration_record(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        self.validate_duration_fields(p, &duration)?;
        let options = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let constrain = self.plain_date_overflow(p, options)?;
        if duration[2..].iter().any(|field| *field != 0.0) {
            return Err(self.range_error(p, "Invalid duration".into()));
        }
        if native == Native::TemporalPlainYearMonthSubtract {
            duration[0] = -duration[0];
            duration[1] = -duration[1];
        }
        if checked_iso_date(year, month as i32, reference_day as i32).is_none() {
            return Err(self.range_error(p, "Invalid PlainYearMonth".into()));
        }
        let result = quench_intl::calendar_date_add(
            (year, month, reference_day),
            (duration[0] as i64, duration[1] as i64, 0, 0),
            &calendar,
            constrain,
        )
            .ok_or_else(|| self.range_error(p, "Invalid PlainYearMonth".into()))?;
        Ok(self.heap.alloc(Cell::TemporalPlainYearMonth {
            object: Box::new(Self::empty_object(self.temporal_plain_year_month_proto)),
            year: result.0,
            month: result.1,
            calendar,
            reference_iso_day: result.2,
        }))
    }

    fn temporal_plain_year_month_with(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        year: i32,
        month: u32,
        month_code: String,
        calendar: String,
    ) -> Result<Value, JsError> {
        let changes = args.first().copied().unwrap_or(Value::UNDEFINED);
        let (changed_year, changed_month, changed_code, era, era_year) =
            self.temporal_plain_year_month_change_fields(p, changes)?;
        if changed_month.is_some_and(|month| month <= 0) {
            return Err(self.range_error(p, "Invalid PlainYearMonth".into()));
        }
        let options = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let constrain = self.plain_date_overflow(p, options)?;
        if changed_year.is_none()
            && changed_month.is_none()
            && changed_code.is_none()
            && era.is_none()
            && era_year.is_none()
        {
            return Err(self.type_error(p, "Invalid fields".into()));
        }
        if changed_year.is_none() && era.is_some() != era_year.is_some() {
            return Err(self.type_error(p, "era and eraYear must be provided together".into()));
        }
        let original_year = year;
        let year = if changed_year.is_some() {
            changed_year.unwrap_or(year)
        } else if era.is_some() {
            self.resolve_calendar_year(
                p,
                &calendar,
                None,
                era.as_deref(),
                era_year,
            )?
        } else {
            year
        };
        let changed_code = changed_code.or_else(|| {
            (changed_month.is_none() && year != original_year).then(|| month_code)
        });
        let changed_code = changed_code
            .map(|code| self.heap.alloc(Cell::String(code.into())))
            .map(|code| {
                self.plain_date_month_code(
                    p,
                    code,
                    &calendar,
                    changed_year.unwrap_or(year),
                    constrain,
                )
            })
            .transpose()?;
        let month = match (changed_month, changed_code) {
            (Some(month), Some(code)) if month != code => {
                return Err(self.range_error(p, "Conflicting month fields".into()));
            }
            (Some(month), _) => month,
            (None, Some(code)) => code,
            (None, None) => month as i32,
        };
        let date = quench_intl::calendar_date_to_iso_with_overflow(
            year,
            month as u32,
            1,
            &calendar,
            constrain,
        )
        .ok_or_else(|| self.range_error(p, "Invalid PlainYearMonth".into()))?;
        let constructor = self.native_value(Native::TemporalPlainYearMonth);
        self.make_plain_year_month(p, constructor, date.0, date.1, calendar, date.2)
    }

    fn temporal_plain_year_month_change_fields(
        &mut self,
        p: &ResidualProgram,
        changes: Value,
    ) -> Result<(Option<i32>, Option<i32>, Option<String>, Option<String>, Option<i32>), JsError> {
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
        let era_atom = self.intern_atom("era");
        let era_value = self.get_property(p, changes, era_atom)?;
        let era = if era_value.is_undefined() {
            None
        } else {
            Some(self.to_string(p, era_value)?.to_string())
        };
        let era_year_atom = self.intern_atom("eraYear");
        let era_year_value = self.get_property(p, changes, era_year_atom)?;
        let era_year = self.plain_date_optional_integer(p, era_year_value)?;
        Ok((year, month, month_code, era, era_year))
    }

    fn temporal_plain_year_month_difference_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
        year: i32,
        month: u32,
        reference_day: u32,
        calendar: String,
    ) -> Result<Value, JsError> {
        let constructor = self.native_value(Native::TemporalPlainYearMonth);
        let other_args = [args.first().copied().unwrap_or(Value::UNDEFINED)];
        let other = self.temporal_plain_year_month_from(p, constructor, &other_args)?;
        let (other_year, other_month, other_reference_day, other_calendar) = match self.heap.get(other) {
            Some(Cell::TemporalPlainYearMonth {
                year,
                month,
                calendar,
                reference_iso_day,
                ..
            }) => (*year, *month, *reference_iso_day, calendar.clone()),
            _ => return Err(self.type_error(p, "Invalid PlainYearMonth".into())),
        };
        if calendar != other_calendar {
            return Err(self.range_error(p, "Calendars must match".into()));
        }
        self.temporal_plain_year_month_difference(
            p,
            native,
            (year, month, reference_day),
            (other_year, other_month, other_reference_day),
            calendar,
            args.get(1).copied().unwrap_or(Value::UNDEFINED),
        )
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
        args: &[Value],
    ) -> Result<Value, JsError> {
        let constructor = self.native_value(match native {
            Native::TemporalPlainMonthDayFrom => Native::TemporalPlainMonthDay,
            Native::TemporalPlainYearMonthFrom => Native::TemporalPlainYearMonth,
            _ => unreachable!("not a calendar projection from method"),
        });
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
            reference_iso_year,
            ..
        }) = self.heap.get(value)
        {
            let (month, day, calendar, year) =
                (*month, *day, calendar.clone(), *reference_iso_year);
            let _ = self.plain_date_overflow(p, options)?;
            return self.make_plain_month_day(p, constructor, month, day, calendar, year);
        }
        if let Some(Cell::String(text)) = self.heap.get(value) {
            let text = text.host_string().to_owned();
            let (date, calendar) = parse_plain_month_day_string(self, p, &text)?;
            let _ = self.plain_date_overflow(p, options)?;
            return self.make_plain_month_day(
                p,
                constructor,
                date.month,
                date.day,
                calendar,
                DEFAULT_REFERENCE_ISO_YEAR,
            );
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
            let (_, month, day, calendar) = self.temporal_plain_date_slots(p, date)?;
            return self.make_plain_month_day(
                p,
                constructor,
                month,
                day,
                calendar,
                DEFAULT_REFERENCE_ISO_YEAR,
            );
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
        let month_code_text = if month_code.is_undefined() {
            None
        } else {
            Some(self.to_string(p, month_code)?.to_string())
        };
        let month_code = month_code_text
            .as_deref()
            .map(|text| {
                super::temporal_date::parse_iso_month_code_syntax(text)
                    .ok_or_else(|| self.range_error(p, "Invalid monthCode".into()))
            })
            .transpose()?;
        let year_atom = self.intern_atom("year");
        let year_value = self.get_property(p, bag, year_atom)?;
        let year = if year_value.is_undefined() {
            DEFAULT_REFERENCE_ISO_YEAR
        } else {
            self.plain_date_integer(p, year_value)?
        };
        let constrain = self.plain_date_overflow(p, options)?;
        let non_iso_calendar = !matches!(calendar.as_str(), "iso8601" | "gregory");
        if non_iso_calendar && year_value.is_undefined() && month.is_some() {
            return Err(self.type_error(p, "Missing year".into()));
        }
        if non_iso_calendar && !year_value.is_undefined() {
            if let Some(code) = month_code_text.as_deref() {
                let ordinal = quench_intl::calendar_month_from_code(year, code, &calendar)
                    .ok_or_else(|| self.range_error(p, "Invalid monthCode".into()))?;
                if month.is_some_and(|month| month != ordinal as i32) {
                    return Err(self.range_error(p, "month and monthCode must agree".into()));
                }
                let iso = quench_intl::calendar_date_to_iso_with_overflow(
                    year,
                    ordinal,
                    day as u32,
                    &calendar,
                    constrain,
                )
                .ok_or_else(|| self.range_error(p, "Invalid PlainMonthDay".into()))?;
                return self.make_plain_month_day_from_iso_date(p, constructor, iso, calendar);
            }
        }
        if month.is_none()
            && year_value.is_undefined()
            && !matches!(calendar.as_str(), "iso8601" | "gregory")
        {
            if let Some(code) = month_code_text.as_deref() {
                let iso = quench_intl::calendar_reference_date_from_code(
                    code,
                    day as u32,
                    &calendar,
                    constrain,
                )
                .ok_or_else(|| self.range_error(p, "Invalid PlainMonthDay".into()))?;
                return self.make_plain_month_day(p, constructor, iso.1, iso.2, calendar, iso.0);
            }
        }
        let month_code = month_code
            .map(|month| {
                (1..=super::temporal_date::ISO_MONTHS_PER_YEAR)
                    .contains(&month)
                    .then_some(month)
                    .ok_or_else(|| self.range_error(p, "Invalid monthCode".into()))
            })
            .transpose()?;
        let month = match (month, month_code) {
            (Some(month), Some(code)) if month != code => {
                return Err(self.range_error(p, "month and monthCode must agree".into()));
            }
            (Some(month), _) => month,
            (None, Some(code)) => code,
            (None, None) => return Err(self.type_error(p, "Missing month".into())),
        };
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
        if day > max_day {
            return Err(self.range_error(p, "Invalid PlainMonthDay".into()));
        }
        self.make_plain_month_day(
            p,
            constructor,
            month as u32,
            day as u32,
            calendar,
            DEFAULT_REFERENCE_ISO_YEAR,
        )
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
        let (calendar, year, month, month_code) = self.temporal_plain_year_month_fields(p, bag)?;
        let constrain = self.plain_date_overflow(p, options)?;
        let month = match (month, month_code) {
            (Some(month), Some(code)) if month != code => {
                return Err(self.range_error(p, "Conflicting month fields".into()));
            }
            (Some(month), _) => month,
            (None, Some(code)) => code,
            (None, None) => return Err(self.type_error(p, "Missing month".into())),
        };
        if month <= 0 {
            return Err(self.range_error(p, "Invalid PlainYearMonth".into()));
        }
        let iso = quench_intl::calendar_date_to_iso_with_overflow(
            year,
            month as u32,
            1,
            &calendar,
            constrain,
        )
        .ok_or_else(|| self.range_error(p, "Invalid PlainYearMonth".into()))?;
        self.make_plain_year_month(
            p,
            constructor,
            iso.0,
            iso.1,
            calendar,
            iso.2,
        )
    }

    fn temporal_plain_year_month_fields(
        &mut self,
        p: &ResidualProgram,
        bag: Value,
    ) -> Result<(String, i32, Option<i32>, Option<i32>), JsError> {
        let calendar_atom = self.intern_atom("calendar");
        let calendar_value = self.get_property(p, bag, calendar_atom)?;
        let calendar = self.temporal_calendar_property(p, calendar_value)?;
        let month_atom = self.intern_atom("month");
        let month_value = self.get_property(p, bag, month_atom)?;
        let month = self.plain_date_optional_integer(p, month_value)?;
        let month_code_atom = self.intern_atom("monthCode");
        let month_code_value = self.get_property(p, bag, month_code_atom)?;
        let month_code_text = if month_code_value.is_undefined() {
            None
        } else {
            let string_or_object = matches!(self.heap.get(month_code_value), Some(Cell::String(_)))
                || self.is_object_like(month_code_value);
            if !string_or_object {
                return Err(self.type_error(p, "Invalid monthCode".into()));
            }
            let text = self.to_string(p, month_code_value)?.to_string();
            if self.is_object_like(month_code_value) && !text.starts_with('M') {
                return Err(self.type_error(p, "Invalid monthCode".into()));
            }
            Some(text)
        };
        let year_atom = self.intern_atom("year");
        let year_value = self.get_property(p, bag, year_atom)?;
        let year = self.plain_date_optional_integer(p, year_value)?;
        let era_atom = self.intern_atom("era");
        let era_value = self.get_property(p, bag, era_atom)?;
        let era = if era_value.is_undefined() {
            None
        } else {
            Some(self.to_string(p, era_value)?.to_string())
        };
        let era_year_atom = self.intern_atom("eraYear");
        let era_year_value = self.get_property(p, bag, era_year_atom)?;
        let era_year = self.plain_date_optional_integer(p, era_year_value)?;
        let year = self.resolve_calendar_year(p, &calendar, year, era.as_deref(), era_year)?;
        let month_code = month_code_text
            .map(|text| self.parse_plain_date_month_code(p, &text, &calendar, year, false))
            .transpose()?;
        Ok((calendar, year, month, month_code))
    }

    fn make_plain_month_day(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
        month: u32,
        day: u32,
        calendar: String,
        reference_year: i32,
    ) -> Result<Value, JsError> {
        let args = [
            Value::number(f64::from(month)),
            Value::number(f64::from(day)),
            self.heap.alloc(Cell::String(calendar.into())),
            Value::number(f64::from(reference_year)),
        ];
        self.temporal_plain_month_day_construct(p, &args, constructor)
    }

    fn make_plain_month_day_from_iso_date(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
        (year, month, day): (i32, u32, u32),
        calendar: String,
    ) -> Result<Value, JsError> {
        let reference = quench_intl::calendar_fields_from_iso(year, month, day, &calendar)
            .and_then(|fields| {
                quench_intl::calendar_reference_date_from_code(
                    &fields.month_code,
                    fields.day,
                    &calendar,
                    false,
                )
            });
        let (iso_year, iso_month, iso_day) = reference.unwrap_or((
            DEFAULT_REFERENCE_ISO_YEAR,
            month,
            day,
        ));
        self.make_plain_month_day(p, constructor, iso_month, iso_day, calendar, iso_year)
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

    pub(super) fn temporal_calendar_property(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<String, JsError> {
        match self.heap.get(value) {
            None if value.is_undefined() => Ok("iso8601".into()),
            Some(Cell::String(_)) => {
                let text = self.to_string(p, value)?.to_string();
                temporal_date_parse::calendar_identifier_from_string(&text)
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
            | Native::TemporalPlainYearMonthEraGetter
            | Native::TemporalPlainYearMonthEraYearGetter
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
    if let Some(date) = temporal_date_parse::parse_plain_month_day_string(text) {
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
        validate_plain_year_month_calendar_string(vm, p, text, &date.1)?;
        return Ok(date);
    }
    if let Some((padded, year, month)) = normalize_year_month_date(text) {
        if let Some(date) = temporal_date_parse::parse_plain_date_string(&padded) {
            validate_plain_year_month_calendar_string(vm, p, text, &date.1)?;
            return Ok(date);
        }
        if !text.contains(['Z', 'z'])
            && let Some((local, _, _)) = super::temporal_zoned_date_time::parse_iso_zoned_base(text)
        {
            use chrono::Datelike;
            let date = checked_iso_date(local.year(), local.month() as i32, 1)
                .ok_or_else(|| vm.range_error(p, "Invalid PlainYearMonth".into()))?;
            let calendar = temporal_date_parse::parse_calendar_identifier(text)
                .ok_or_else(|| vm.range_error(p, "Invalid PlainYearMonth".into()))?;
            validate_plain_year_month_calendar_string(vm, p, text, &calendar)?;
            return Ok((date, calendar));
        }
        if !text.contains(['Z', 'z']) {
            if let Ok(parsed) = parse_plain_year_month_boundary(vm, p, text, year, month) {
                validate_plain_year_month_calendar_string(vm, p, text, &parsed.1)?;
                return Ok(parsed);
            }
        }
    }
    Err(vm.range_error(p, "Invalid PlainYearMonth".into()))
}

fn validate_plain_year_month_calendar_string<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    text: &str,
    calendar: &str,
) -> Result<(), JsError> {
    let base = text.split('[').next().unwrap_or(text);
    if calendar != "iso8601" && !base.contains(['T', 't', ' ']) && base.len() <= 7 {
        return Err(vm.range_error(p, "Invalid PlainYearMonth".into()));
    }
    Ok(())
}

fn normalize_year_month_date(text: &str) -> Option<(String, i32, u32)> {
    let annotation = text.find('[').unwrap_or(text.len());
    let (base, suffix) = text.split_at(annotation);
    let time_start = base.find(['T', 't', ' ']).unwrap_or(base.len());
    let (date, time) = base.split_at(time_start);
    let (year, month) = year_month_components(date)?;
    let time = normalize_iso_time(time)?;
    Some((
        format!("{year}-{month}-01{time}{suffix}"),
        year.parse().ok()?,
        month.parse().ok()?,
    ))
}

fn normalize_iso_time(time: &str) -> Option<String> {
    if time.is_empty() {
        return Some(String::new());
    }
    let (designator, clock_and_offset) = time.split_at(1);
    let offset_start = clock_and_offset
        .find(['+', '-'])
        .map(|index| index + 1)
        .unwrap_or(time.len());
    let (clock, offset) = time[1..].split_at(offset_start - 1);
    let fraction_start = clock.find(['.', ',']).unwrap_or(clock.len());
    let (clock, fraction) = clock.split_at(fraction_start);
    let clock = match clock.len() {
        BASIC_ISO_TIME_MINUTES_LENGTH => {
            format!("{}:{}", &clock[..2], &clock[2..])
        }
        BASIC_ISO_TIME_SECONDS_LENGTH => {
            format!("{}:{}:{}", &clock[..2], &clock[2..4], &clock[4..])
        }
        _ => clock.to_owned(),
    };
    let offset = normalize_iso_offset(offset)?;
    Some(format!("{designator}{clock}{fraction}{offset}"))
}

fn normalize_iso_offset(offset: &str) -> Option<String> {
    if offset.is_empty() {
        return Some(String::new());
    }
    let (sign, digits) = offset.split_at(1);
    match digits.len() {
        4 if digits.bytes().all(|byte| byte.is_ascii_digit()) => {
            Some(format!("{sign}{}:{}", &digits[..2], &digits[2..]))
        }
        6 if digits.bytes().all(|byte| byte.is_ascii_digit()) => Some(format!(
            "{sign}{}:{}:{}",
            &digits[..2],
            &digits[2..4],
            &digits[4..]
        )),
        _ => Some(offset.to_owned()),
    }
}

fn year_month_components(date: &str) -> Option<(&str, &str)> {
    if date.bytes().all(|byte| byte.is_ascii_digit()) {
        return compact_year_month_components(date);
    }
    if date.starts_with(['+', '-']) && date[1..].bytes().all(|byte| byte.is_ascii_digit()) {
        return signed_compact_year_month_components(date);
    }
    let fields = date.split('-').collect::<Vec<_>>();
    match fields.as_slice() {
        ["", year, month] if valid_iso_month(month) => Some((&date[..year.len() + 1], *month)),
        ["", year, month, day] if valid_iso_month(month) && valid_iso_day(day) => {
            Some((&date[..year.len() + 1], *month))
        }
        [year, month] if valid_iso_month(month) => Some((*year, *month)),
        [year, month, day] if valid_iso_month(month) && valid_iso_day(day) => Some((*year, *month)),
        _ => None,
    }
}

fn valid_iso_month(value: &str) -> bool {
    value.len() == ISO_MONTH_CODE_DIGITS && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn valid_iso_day(value: &str) -> bool {
    valid_iso_month(value)
}

fn compact_year_month_components(date: &str) -> Option<(&str, &str)> {
    match date.len() {
        BASIC_ISO_YEAR_MONTH_LENGTH => Some((
            &date[..BASIC_ISO_YEAR_LENGTH],
            &date[BASIC_ISO_YEAR_LENGTH..],
        )),
        BASIC_ISO_DATE_LENGTH => Some((
            &date[..BASIC_ISO_YEAR_LENGTH],
            &date[BASIC_ISO_YEAR_LENGTH..BASIC_ISO_YEAR_LENGTH + 2],
        )),
        _ => None,
    }
}

fn signed_compact_year_month_components(date: &str) -> Option<(&str, &str)> {
    match date.len() {
        SIGNED_COMPACT_YEAR_MONTH_LENGTH => Some((
            &date[..EXTENDED_ISO_YEAR_LENGTH],
            &date[EXTENDED_ISO_YEAR_LENGTH..],
        )),
        SIGNED_COMPACT_DATE_LENGTH => Some((
            &date[..EXTENDED_ISO_YEAR_LENGTH],
            &date[EXTENDED_ISO_YEAR_LENGTH..EXTENDED_ISO_YEAR_LENGTH + 2],
        )),
        _ => None,
    }
}

fn parse_plain_year_month_boundary<H: Host>(
    vm: &mut Vm<H>,
    p: &ResidualProgram,
    text: &str,
    year: i32,
    month: u32,
) -> Result<(super::temporal_date::IsoDate, String), JsError> {
    let lower_edge = year == super::temporal_date::MIN_ISO_YEAR && month == 4;
    let upper_edge = year == super::temporal_date::MAX_ISO_YEAR && month == 9;
    if !lower_edge && !upper_edge {
        return Err(vm.range_error(p, "Invalid PlainYearMonth".into()));
    }
    let calendar = if !text.contains('[') {
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
    let [hour, minute, second, millisecond, microsecond, nanosecond] = time;
    let mut to_unsigned =
        |field| u32::try_from(field).map_err(|_| vm.range_error(p, "Invalid PlainTime".into()));
    let time = [
        to_unsigned(hour)?,
        to_unsigned(minute)?,
        to_unsigned(second)?,
        to_unsigned(millisecond)?,
        to_unsigned(microsecond)?,
        to_unsigned(nanosecond)?,
    ];
    vm.make_zoned_date_time_from_local(
        p,
        super::temporal_date::IsoDate { year, month, day },
        time,
        calendar,
        time_zone,
        "compatible",
    )
}

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
        let date = &date[..month_end];
        let text = if annotation.is_empty() {
            date.to_owned()
        } else {
            format!("{date}[{annotation}")
        };
        return Ok(vm.heap.alloc(Cell::String(text.into())));
    }
    Err(vm.type_error(p, "Invalid Temporal calendar object".into()))
}
