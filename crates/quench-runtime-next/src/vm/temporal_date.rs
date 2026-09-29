use super::*;
pub(super) const ISO_MONTHS_PER_YEAR: i32 = 12;
const HEBREW_LEAP_MONTH_FALLBACK: &str = "M06";
pub(super) const ISO_DAYS_PER_WEEK: i64 = 7;
const ISO_WEEK_NUMBER_ADJUSTMENT: i64 = 10;
const ISO_UNIX_EPOCH_WEEKDAY: i64 = 4;
const ISO_COMMON_WEEKS_PER_YEAR: i64 = 52;
const ISO_LONG_WEEKS_PER_YEAR: i64 = 53;
const ISO_THURSDAY: i64 = 4;
const ISO_WEDNESDAY: i64 = 3;
const ISO_JANUARY: u32 = 1;
const ISO_FEBRUARY: i32 = 2;
const ISO_JANUARY_FIRST: u32 = 1;
pub(super) const MIN_ISO_YEAR: i32 = -271_821;
pub(super) const MAX_ISO_YEAR: i32 = 275_760;
const MAX_BASIC_ISO_YEAR: i32 = 9_999;
const BASIC_ISO_YEAR_DIGITS: usize = 4;
const EXTENDED_ISO_YEAR_DIGITS: usize = 6;
const ISO_MONTH_DAY_DIGITS: usize = 2;
pub(super) const ISO_MONTH_CODE_DIGITS: usize = 2;
const MONTHS_BEFORE_ISO_YEAR: i32 = 1;

pub(super) fn parse_iso_month_code(code: &str) -> Option<i32> {
    parse_iso_month_code_syntax(code).filter(|month| (1..=ISO_MONTHS_PER_YEAR).contains(month))
}

pub(super) fn parse_iso_month_code_syntax(code: &str) -> Option<i32> {
    let month_code = code.strip_prefix('M')?;
    let (digits, leap) = if let Some(digits) = month_code.strip_suffix('L') {
        (digits, true)
    } else {
        (month_code, false)
    };
    if digits.len() != ISO_MONTH_CODE_DIGITS || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let month = digits.parse::<i32>().ok()?;
    Some(if leap {
        month + ISO_LEAP_MONTH_CODE_OFFSET
    } else {
        month
    })
}

const ISO_LEAP_MONTH_CODE_OFFSET: i32 = 1_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct IsoDate {
    pub(super) year: i32,
    pub(super) month: u32,
    pub(super) day: u32,
}

impl<H: Host> Vm<H> {
    pub(super) fn install_temporal_plain_date(
        &mut self,
        p: &ResidualProgram,
        temporal: Value,
    ) -> Result<(), JsError> {
        let constructor =
            self.native_with_realm(Native::TemporalPlainDate, temporal, self.realm.globals);
        self.set_builtin_function_name(constructor, "PlainDate")?;
        let prototype = self.object();
        self.temporal_plain_date_proto = prototype;
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
        for (name, native) in [
            ("from", Native::TemporalPlainDateFrom),
            ("compare", Native::TemporalPlainDateCompare),
        ] {
            self.set_builtin_named(p, constructor, name, native)?;
        }
        for (name, native) in [
            ("calendarId", Native::TemporalPlainDateCalendarIdGetter),
            ("year", Native::TemporalPlainDateYearGetter),
            ("month", Native::TemporalPlainDateMonthGetter),
            ("monthCode", Native::TemporalPlainDateMonthCodeGetter),
            ("day", Native::TemporalPlainDateDayGetter),
            ("era", Native::TemporalPlainDateEraGetter),
            ("eraYear", Native::TemporalPlainDateEraYearGetter),
            ("dayOfWeek", Native::TemporalPlainDateDayOfWeekGetter),
            ("dayOfYear", Native::TemporalPlainDateDayOfYearGetter),
            ("weekOfYear", Native::TemporalPlainDateWeekOfYearGetter),
            ("yearOfWeek", Native::TemporalPlainDateYearOfWeekGetter),
            ("daysInWeek", Native::TemporalPlainDateDaysInWeekGetter),
            ("daysInMonth", Native::TemporalPlainDateDaysInMonthGetter),
            ("daysInYear", Native::TemporalPlainDateDaysInYearGetter),
            ("monthsInYear", Native::TemporalPlainDateMonthsInYearGetter),
            ("inLeapYear", Native::TemporalPlainDateInLeapYearGetter),
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
        for (name, native) in [
            ("toString", Native::TemporalPlainDateToString),
            ("toJSON", Native::TemporalPlainDateToJSON),
            ("toLocaleString", Native::TemporalPlainDateToLocaleString),
            ("toPlainDateTime", Native::TemporalPlainDateToPlainDateTime),
            ("toPlainMonthDay", Native::TemporalPlainDateToPlainMonthDay),
            (
                "toPlainYearMonth",
                Native::TemporalPlainDateToPlainYearMonth,
            ),
            ("toZonedDateTime", Native::TemporalPlainDateToZonedDateTime),
            ("with", Native::TemporalPlainDateWith),
            ("withCalendar", Native::TemporalPlainDateWithCalendar),
            ("equals", Native::TemporalPlainDateEquals),
            ("valueOf", Native::TemporalPlainDateValueOf),
            ("add", Native::TemporalPlainDateAdd),
            ("subtract", Native::TemporalPlainDateSubtract),
            ("until", Native::TemporalPlainDateUntil),
            ("since", Native::TemporalPlainDateSince),
        ] {
            self.set_builtin_named(p, prototype, name, native)?;
        }
        if let Some(symbol) = self.well_known_symbols.get("toStringTag").copied() {
            let tag = self.heap.alloc(Cell::String("Temporal.PlainDate".into()));
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
        self.set_builtin_value_named(temporal, "PlainDate", constructor)
    }

    pub(super) fn temporal_plain_date_construct(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        let year = self.plain_date_integer(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let month = self.plain_date_integer(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
        let day = self.plain_date_integer(p, args.get(2).copied().unwrap_or(Value::UNDEFINED))?;
        let calendar = match args.get(3).copied().unwrap_or(Value::UNDEFINED) {
            value if value.is_undefined() => "iso8601".to_owned(),
            value if matches!(self.heap.get(value), Some(Cell::String(_))) => {
                self.to_string(p, value)?.to_string()
            }
            _ => return Err(self.type_error(p, "Invalid calendar".into())),
        };
        let calendar = quench_intl::calendar_alias(&calendar);
        if !quench_intl::valid_calendar(&calendar) {
            return Err(self.range_error(p, "Invalid calendar".into()));
        }
        let date = checked_iso_date(year, month, day)
            .ok_or_else(|| self.range_error(p, "Invalid PlainDate".into()))?;
        self.make_temporal_plain_date(p, date, calendar, new_target)
    }

    pub(super) fn plain_date_integer(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<i32, JsError> {
        let number = self.to_number(p, value)?;
        if !number.is_finite()
            || number.trunc() < f64::from(i32::MIN)
            || number.trunc() > f64::from(i32::MAX)
        {
            return Err(self.range_error(p, "Invalid PlainDate".into()));
        }
        Ok(number.trunc() as i32)
    }

    pub(super) fn make_temporal_plain_date(
        &mut self,
        p: &ResidualProgram,
        date: IsoDate,
        calendar: String,
        new_target: Value,
    ) -> Result<Value, JsError> {
        let prototype_atom = self.intern_atom("prototype");
        let prototype = self.get_property(p, new_target, prototype_atom)?;
        let prototype = if self.is_object_like(prototype) {
            prototype
        } else {
            self.object_proto
        };
        Ok(self.heap.alloc(Cell::TemporalPlainDate {
            object: Box::new(Self::empty_object(prototype)),
            year: date.year,
            month: date.month,
            day: date.day,
            calendar,
        }))
    }

    pub(super) fn temporal_plain_date_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if native == Native::TemporalPlainDateWith {
            return self.temporal_plain_date_with(p, this, args);
        }
        if native == Native::TemporalPlainDateWithCalendar {
            return self.temporal_plain_date_with_calendar(p, this, args);
        }
        match native {
            Native::TemporalPlainDate => {
                Err(self.type_error(p, "Temporal.PlainDate requires new".into()))
            }
            Native::TemporalPlainDateFrom => {
                let constructor = self.temporal_plain_date_constructor(p)?;
                self.temporal_plain_date_from(p, constructor, args)
            }
            Native::TemporalPlainDateCompare => self.temporal_plain_date_compare(p, args),
            Native::TemporalPlainDateEquals => self.temporal_plain_date_equals(p, this, args),
            Native::TemporalPlainDateValueOf => Err(self.type_error(
                p,
                "Temporal.PlainDate.prototype.valueOf is not allowed".into(),
            )),
            Native::TemporalPlainDateToString => {
                let (year, month, day, calendar) = self.temporal_plain_date_slots(p, this)?;
                let text = self.temporal_plain_date_to_string(
                    p,
                    year,
                    month,
                    day,
                    &calendar,
                    args.first().copied().unwrap_or(Value::UNDEFINED),
                )?;
                Ok(self.heap.alloc(Cell::String(text.into())))
            }
            Native::TemporalPlainDateToLocaleString => {
                self.temporal_to_locale_string(p, this, args)
            }
            Native::TemporalPlainDateToJSON => {
                let (year, month, day, _) = self.temporal_plain_date_slots(p, this)?;
                Ok(self
                    .heap
                    .alloc(Cell::String(format_iso_date(year, month, day).into())))
            }
            Native::TemporalPlainDateToPlainDateTime => {
                super::temporal_plain_date_time_conversion::convert(self, p, this, args)
            }
            Native::TemporalPlainDateToPlainMonthDay => {
                super::temporal_date_projection::to_plain_month_day(self, p, this)
            }
            Native::TemporalPlainDateToPlainYearMonth => {
                super::temporal_date_projection::to_plain_year_month(self, p, this)
            }
            Native::TemporalPlainDateToZonedDateTime => {
                super::temporal_date_projection::to_zoned_date_time(self, p, this, args)
            }
            Native::TemporalPlainDateCalendarIdGetter
            | Native::TemporalPlainDateYearGetter
            | Native::TemporalPlainDateMonthGetter
            | Native::TemporalPlainDateMonthCodeGetter
            | Native::TemporalPlainDateDayGetter
            | Native::TemporalPlainDateEraGetter
            | Native::TemporalPlainDateEraYearGetter
            | Native::TemporalPlainDateDayOfWeekGetter
            | Native::TemporalPlainDateDayOfYearGetter
            | Native::TemporalPlainDateWeekOfYearGetter
            | Native::TemporalPlainDateYearOfWeekGetter
            | Native::TemporalPlainDateDaysInWeekGetter
            | Native::TemporalPlainDateDaysInMonthGetter
            | Native::TemporalPlainDateDaysInYearGetter
            | Native::TemporalPlainDateMonthsInYearGetter
            | Native::TemporalPlainDateInLeapYearGetter => {
                let (year, month, day, calendar) = self.temporal_plain_date_slots(p, this)?;
                let calendar_fields =
                    quench_intl::calendar_fields_from_iso(year, month, day, &calendar);
                Ok(match native {
                    Native::TemporalPlainDateCalendarIdGetter => {
                        self.heap.alloc(Cell::String(calendar.into()))
                    }
                    Native::TemporalPlainDateYearGetter => Value::number(f64::from(
                        calendar_fields.as_ref().map_or(year, |fields| fields.year),
                    )),
                    Native::TemporalPlainDateMonthGetter => Value::number(f64::from(
                        calendar_fields
                            .as_ref()
                            .map_or(month, |fields| fields.month),
                    )),
                    Native::TemporalPlainDateMonthCodeGetter => self.heap.alloc(Cell::String(
                        calendar_fields
                            .map_or_else(
                                || format!("M{month:0width$}", width = ISO_MONTH_DAY_DIGITS),
                                |fields| fields.month_code,
                            )
                            .into(),
                    )),
                    Native::TemporalPlainDateDayGetter => Value::number(f64::from(
                        calendar_fields.as_ref().map_or(day, |fields| fields.day),
                    )),
                    Native::TemporalPlainDateEraGetter => calendar_fields
                        .and_then(|fields| fields.era)
                        .map_or(Value::UNDEFINED, |era| {
                            self.heap.alloc(Cell::String(era.into()))
                        }),
                    Native::TemporalPlainDateEraYearGetter => calendar_fields
                        .and_then(|fields| fields.era_year)
                        .map_or(Value::UNDEFINED, |year| Value::number(f64::from(year))),
                    Native::TemporalPlainDateDaysInMonthGetter => {
                        Value::number(f64::from(calendar_fields.as_ref().map_or_else(
                            || iso_days_in_month(year, month as i32).unwrap_or(31) as u32,
                            |fields| fields.days_in_month,
                        )))
                    }
                    Native::TemporalPlainDateDayOfWeekGetter => {
                        Value::number(f64::from(iso_day_of_week(IsoDate { year, month, day })))
                    }
                    Native::TemporalPlainDateDayOfYearGetter => {
                        Value::number(f64::from(calendar_fields.as_ref().map_or_else(
                            || iso_day_of_year(IsoDate { year, month, day }),
                            |fields| fields.day_of_year,
                        )))
                    }
                    Native::TemporalPlainDateWeekOfYearGetter => {
                        temporal_iso_week(IsoDate { year, month, day }, &calendar)
                            .map_or(Value::UNDEFINED, |(week, _)| Value::number(f64::from(week)))
                    }
                    Native::TemporalPlainDateYearOfWeekGetter => {
                        temporal_iso_week(IsoDate { year, month, day }, &calendar)
                            .map_or(Value::UNDEFINED, |(_, week_year)| {
                                Value::number(f64::from(week_year))
                            })
                    }
                    Native::TemporalPlainDateDaysInWeekGetter => {
                        Value::number(ISO_DAYS_PER_WEEK as f64)
                    }
                    Native::TemporalPlainDateDaysInYearGetter => Value::number(f64::from(
                        calendar_fields
                            .as_ref()
                            .map_or_else(|| iso_days_in_year(year), |fields| fields.days_in_year),
                    )),
                    Native::TemporalPlainDateMonthsInYearGetter => Value::number(f64::from(
                        calendar_fields
                            .as_ref()
                            .map_or(ISO_MONTHS_PER_YEAR as u32, |fields| fields.months_in_year),
                    )),
                    Native::TemporalPlainDateInLeapYearGetter => {
                        if calendar_fields
                            .map_or_else(|| iso_is_leap_year(year), |fields| fields.is_leap_year)
                        {
                            Value::TRUE
                        } else {
                            Value::FALSE
                        }
                    }
                    _ => Value::number(f64::from(day)),
                })
            }
            _ => unreachable!("not a Temporal.PlainDate native"),
        }
    }

    pub(super) fn temporal_plain_date_with(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let (iso_year, iso_month, iso_day, calendar) =
            self.temporal_plain_date_slots(p, this)?;
        let calendar_fields =
            quench_intl::calendar_fields_from_iso(iso_year, iso_month, iso_day, &calendar);
        let base_year = calendar_fields.as_ref().map_or(iso_year, |fields| fields.year);
        let base_month = calendar_fields
            .as_ref()
            .map_or(iso_month as i32, |fields| fields.month as i32);
        let base_day = calendar_fields
            .as_ref()
            .map_or(iso_day as i32, |fields| fields.day as i32);
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
            return Err(self.type_error(p, "Invalid fields".into()));
        }
        let calendar_atom = self.intern_atom("calendar");
        let time_zone_atom = self.intern_atom("timeZone");
        if !self.get_property(p, changes, calendar_atom)?.is_undefined()
            || !self
                .get_property(p, changes, time_zone_atom)?
                .is_undefined()
        {
            return Err(self.type_error(p, "Invalid fields".into()));
        }
        let day_atom = self.intern_atom("day");
        let day_value = self.get_property(p, changes, day_atom)?;
        let day = self
            .plain_date_optional_integer(p, day_value)?
            .unwrap_or(base_day as i32);
        let month_atom = self.intern_atom("month");
        let month_value = self.get_property(p, changes, month_atom)?;
        let month = self.plain_date_optional_integer(p, month_value)?;
        let month_code_atom = self.intern_atom("monthCode");
        let month_code_value = self.get_property(p, changes, month_code_atom)?;
        let mut month_code_text = if month_code_value.is_undefined() {
            None
        } else {
            Some(self.to_string(p, month_code_value)?.to_string())
        };
        let year_atom = self.intern_atom("year");
        let year_value = self.get_property(p, changes, year_atom)?;
        let changed_year = self.plain_date_optional_integer(p, year_value)?;
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
        if day_value.is_undefined()
            && month_value.is_undefined()
            && month_code_value.is_undefined()
            && year_value.is_undefined()
            && era_value.is_undefined()
            && era_year_value.is_undefined()
        {
            return Err(self.type_error(p, "Invalid fields".into()));
        }
        let year = if changed_year.is_some() {
            changed_year.unwrap_or(base_year)
        } else if era_value.is_undefined() && era_year_value.is_undefined() {
            base_year
        } else {
            self.resolve_calendar_year(p, &calendar, None, era.as_deref(), era_year)?
        };
        if month.is_none() && month_code_text.is_none() && year != base_year {
            month_code_text = calendar_fields.map(|fields| fields.month_code);
        }
        let options = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let primitive_options = !options.is_undefined() && !self.is_object_like(options);
        let overflow_atom = self.intern_atom("overflow");
        let overflow_value = if options.is_undefined() || primitive_options {
            Value::UNDEFINED
        } else {
            self.get_property(p, options, overflow_atom)?
        };
        let overflow = if overflow_value.is_undefined() {
            "constrain".to_owned()
        } else {
            self.to_string(p, overflow_value)?.to_string()
        };
        if !matches!(overflow.as_str(), "constrain" | "reject") {
            return Err(self.range_error(p, "Invalid overflow".into()));
        }
        let month_code_value =
            month_code_text.map(|value| self.heap.alloc(Cell::String(value.into())));
        let month_code = month_code_value
            .map(|value| {
                self.plain_date_month_code(
                    p,
                    value,
                    &calendar,
                    year,
                    overflow == "constrain",
                )
            })
            .transpose()?;
        let month = match (month, month_code) {
            (Some(month), Some(month_code)) if month != month_code => {
                return Err(self.range_error(p, "month and monthCode must agree".into()));
            }
            (Some(month), _) => month,
            (None, Some(month_code)) => month_code,
            (None, None) => base_month as i32,
        };
        if month < 1 || day < 1 {
            return Err(self.range_error(p, "Invalid PlainDate".into()));
        }
        let iso = quench_intl::calendar_date_to_iso_with_overflow(
            year,
            month as u32,
            day as u32,
            &calendar,
            overflow == "constrain",
        )
        .ok_or_else(|| self.range_error(p, "Invalid PlainDate".into()))?;
        let date = checked_iso_date(iso.0, iso.1 as i32, iso.2 as i32)
            .ok_or_else(|| self.range_error(p, "Invalid PlainDate".into()))?;
        if primitive_options {
            return Err(self.type_error(p, "Invalid options".into()));
        }
        Ok(self.heap.alloc(Cell::TemporalPlainDate {
            object: Box::new(Self::empty_object(self.temporal_plain_date_proto)),
            year: date.year,
            month: date.month,
            day: date.day,
            calendar,
        }))
    }

    fn temporal_plain_date_with_calendar(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let (year, month, day, _) = self.temporal_plain_date_slots(p, this)?;
        let calendar = args.first().copied().unwrap_or(Value::UNDEFINED);
        let calendar = match self.heap.get(calendar) {
            Some(Cell::String(value)) => {
                let text = value.host_string();
                temporal_date_parse::calendar_identifier_from_string(text)
                    .ok_or_else(|| self.range_error(p, "Invalid calendar".into()))?
            }
            Some(Cell::TemporalPlainDate { calendar, .. })
            | Some(Cell::TemporalPlainDateTime { calendar, .. })
            | Some(Cell::TemporalPlainMonthDay { calendar, .. })
            | Some(Cell::TemporalPlainYearMonth { calendar, .. }) => calendar.clone(),
            Some(Cell::TemporalZonedDateTime { calendar, .. }) => calendar.clone(),
            _ => return Err(self.type_error(p, "Invalid calendar".into())),
        };
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

    pub(super) fn temporal_plain_date_from(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let options = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        if let Some(Cell::TemporalPlainDate {
            year,
            month,
            day,
            calendar,
            ..
        }) = self.heap.get(value)
        {
            let (year, month, day, calendar) = (*year, *month, *day, calendar.clone());
            let _ = self.plain_date_overflow(p, options)?;
            let date = checked_iso_date(year, month as i32, day as i32)
                .ok_or_else(|| self.range_error(p, "Invalid PlainDate".into()))?;
            return self.make_temporal_plain_date(p, date, calendar, constructor);
        }
        if let Some(Cell::TemporalPlainDateTime { date, calendar, .. }) = self.heap.get(value) {
            let (date, calendar) = (*date, calendar.clone());
            let _ = self.plain_date_overflow(p, options)?;
            let date = checked_iso_date(date.0, date.1 as i32, date.2 as i32)
                .ok_or_else(|| self.range_error(p, "Invalid PlainDate".into()))?;
            return self.make_temporal_plain_date(p, date, calendar, constructor);
        }
        if let Some(Cell::TemporalZonedDateTime {
            epoch_nanoseconds,
            time_zone,
            calendar,
            ..
        }) = self.heap.get(value)
        {
            let (epoch_nanoseconds, time_zone, calendar) =
                (*epoch_nanoseconds, time_zone.clone(), calendar.clone());
            let _ = self.plain_date_overflow(p, options)?;
            let fields = super::temporal_zoned_date_time::zoned_date_time_fields(
                epoch_nanoseconds,
                &time_zone,
            )
            .ok_or_else(|| self.range_error(p, "Invalid PlainDate".into()))?;
            let date = checked_iso_date(fields[0], fields[1], fields[2])
                .ok_or_else(|| self.range_error(p, "Invalid PlainDate".into()))?;
            return self.make_temporal_plain_date(p, date, calendar, constructor);
        }
        let (date, calendar) = if let Some(Cell::String(text)) = self.heap.get(value) {
            let text = text.host_string().to_owned();
            let (date, calendar) = temporal_date_parse::parse_plain_date_string(&text)
                .ok_or_else(|| self.range_error(p, "Invalid calendar".into()))?;
            let _ = self.plain_date_overflow(p, options)?;
            (date, calendar)
        } else if self.is_object_like(value) {
            self.plain_date_from_bag(p, value, options)?
        } else {
            return Err(self.type_error(p, "Invalid PlainDate".into()));
        };
        self.make_temporal_plain_date(p, date, calendar, constructor)
    }

    pub(super) fn plain_date_overflow(
        &mut self,
        p: &ResidualProgram,
        options: Value,
    ) -> Result<bool, JsError> {
        if options.is_undefined() {
            return Ok(true);
        }
        if !self.is_object_like(options) {
            return Err(self.type_error(p, "Options must be an object".into()));
        }
        let atom = self.intern_atom("overflow");
        let value = self.get_property(p, options, atom)?;
        if value.is_undefined() {
            return Ok(true);
        }
        match self.to_string(p, value)?.to_string().as_str() {
            "constrain" => Ok(true),
            "reject" => Ok(false),
            _ => Err(self.range_error(p, "Invalid overflow option".into())),
        }
    }

    pub(super) fn temporal_plain_date_to_string(
        &mut self,
        p: &ResidualProgram,
        year: i32,
        month: u32,
        day: u32,
        calendar: &str,
        options: Value,
    ) -> Result<String, JsError> {
        let calendar_name = if options.is_undefined() {
            "auto".to_owned()
        } else {
            if !self.is_object_like(options) {
                return Err(self.type_error(p, "Options must be an object".into()));
            }
            let atom = self.intern_atom("calendarName");
            let value = self.get_property(p, options, atom)?;
            if value.is_undefined() {
                "auto".to_owned()
            } else {
                self.to_string(p, value)?.to_string()
            }
        };
        if !matches!(
            calendar_name.as_str(),
            "auto" | "always" | "never" | "critical"
        ) {
            return Err(self.range_error(p, "Invalid calendarName".into()));
        }
        let date = format_iso_date(year, month, day);
        let include_calendar = calendar_name == "always"
            || calendar_name == "critical"
            || calendar_name == "auto" && calendar != "iso8601";
        if !include_calendar {
            return Ok(date);
        }
        let critical_marker = if calendar_name == "critical" { "!" } else { "" };
        Ok(format!("{date}[{critical_marker}u-ca={calendar}]"))
    }

    pub(super) fn plain_date_from_bag(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        options: Value,
    ) -> Result<(IsoDate, String), JsError> {
        let calendar_atom = self.intern_atom("calendar");
        let day_atom = self.intern_atom("day");
        let month_atom = self.intern_atom("month");
        let month_code_atom = self.intern_atom("monthCode");
        let year_atom = self.intern_atom("year");
        let calendar = self.get_property(p, value, calendar_atom)?;
        let calendar = match self.heap.get(calendar) {
            None if calendar.is_undefined() => "iso8601".to_owned(),
            Some(Cell::String(value)) => {
                let value = value.host_string();
                temporal_date_parse::calendar_identifier_from_string(value)
                    .ok_or_else(|| self.range_error(p, "Invalid calendar".into()))?
            }
            Some(Cell::TemporalPlainDate { calendar, .. })
            | Some(Cell::TemporalPlainDateTime { calendar, .. })
            | Some(Cell::TemporalPlainMonthDay { calendar, .. })
            | Some(Cell::TemporalPlainYearMonth { calendar, .. }) => calendar.clone(),
            Some(Cell::TemporalZonedDateTime { calendar, .. }) => calendar.clone(),
            _ => return Err(self.type_error(p, "Invalid calendar".into())),
        };
        if !quench_intl::valid_calendar(&calendar) {
            return Err(self.range_error(p, "Invalid calendar".into()));
        }
        let day = self.get_property(p, value, day_atom)?;
        let day = self.plain_date_optional_integer(p, day)?;
        let month = self.get_property(p, value, month_atom)?;
        let month = self.plain_date_optional_integer(p, month)?;
        let month_code_value = self.get_property(p, value, month_code_atom)?;
        let month_code = self.plain_date_time_month_code_from_value(p, month_code_value)?;
        let year = self.get_property(p, value, year_atom)?;
        let year = self.plain_date_optional_integer(p, year)?;
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
        let constrain = self.plain_date_overflow(p, options)?;
        let Some(day) = day else {
            return Err(self.type_error(p, "Missing PlainDate field".into()));
        };
        let year = self.resolve_calendar_year(
            p,
            &calendar,
            year,
            era.as_deref(),
            era_year,
        )?;
        let month_code = month_code
            .map(|month| self.calendarized_month_code(p, month, &calendar, year))
            .transpose()?;
        let month = match (month, month_code) {
            (Some(month), Some(month_code)) if month != month_code => {
                return Err(self.range_error(p, "month and monthCode must agree".into()));
            }
            (Some(month), _) => month,
            (None, Some(month_code)) => month_code,
            (None, None) => return Err(self.type_error(p, "Missing PlainDate field".into())),
        };
        if month < 1 || day < 1 {
            return Err(self.range_error(p, "Invalid PlainDate".into()));
        }
        let iso = quench_intl::calendar_date_to_iso_with_overflow(
            year,
            month as u32,
            day as u32,
            &calendar,
            constrain,
        )
        .ok_or_else(|| self.range_error(p, "Invalid PlainDate".into()))?;
        let date = checked_iso_date(iso.0, iso.1 as i32, iso.2 as i32)
            .ok_or_else(|| self.range_error(p, "Invalid PlainDate".into()))?;
        Ok((date, calendar))
    }

    pub(super) fn plain_date_month_code(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        calendar: &str,
        year: i32,
        constrain: bool,
    ) -> Result<i32, JsError> {
        let code = self.temporal_month_code_to_string(p, value)?;
        self.parse_plain_date_month_code(p, &code, calendar, year, constrain)
    }

    pub(super) fn temporal_month_code_to_string(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<String, JsError> {
        Ok(self.to_string(p, value)?.to_string())
    }

    pub(super) fn parse_plain_date_month_code(
        &mut self,
        p: &ResidualProgram,
        code: &str,
        calendar: &str,
        year: i32,
        constrain: bool,
    ) -> Result<i32, JsError> {
        let parsed = parse_iso_month_code_syntax(code)
            .ok_or_else(|| self.range_error(p, "Invalid monthCode".into()))?;
        if matches!(calendar, "iso8601" | "gregory") {
            return parse_iso_month_code(code)
                .ok_or_else(|| self.range_error(p, "Invalid monthCode".into()));
        }
        let canonical_code = if parsed >= ISO_LEAP_MONTH_CODE_OFFSET {
            format!("M{:02}L", parsed - ISO_LEAP_MONTH_CODE_OFFSET)
        } else {
            format!("M{parsed:02}")
        };
        quench_intl::calendar_month_from_code(year, &canonical_code, calendar)
            .or_else(|| {
                if !constrain || !canonical_code.ends_with('L') {
                    return None;
                }
                let ordinary_code = if calendar == "hebrew" && canonical_code == "M05L" {
                    HEBREW_LEAP_MONTH_FALLBACK
                } else {
                    canonical_code.trim_end_matches('L')
                };
                quench_intl::calendar_month_from_code(year, ordinary_code, calendar)
            })
            .or_else(|| {
                (!canonical_code.ends_with('L') && parsed <= ISO_MONTHS_PER_YEAR as i32)
                    .then_some(parsed as u32)
            })
            .map(|month| month as i32)
            .ok_or_else(|| self.range_error(p, "Invalid monthCode".into()))
    }

    pub(super) fn calendarized_month_code(
        &mut self,
        p: &ResidualProgram,
        code: i32,
        calendar: &str,
        year: i32,
    ) -> Result<i32, JsError> {
        let code = if code >= ISO_LEAP_MONTH_CODE_OFFSET {
            format!("M{:02}L", code - ISO_LEAP_MONTH_CODE_OFFSET)
        } else {
            format!("M{code:02}")
        };
        self.parse_plain_date_month_code(p, &code, calendar, year, false)
    }

    pub(super) fn resolve_calendar_year(
        &mut self,
        p: &ResidualProgram,
        calendar: &str,
        year: Option<i32>,
        era: Option<&str>,
        era_year: Option<i32>,
    ) -> Result<i32, JsError> {
        if !quench_intl::calendar_uses_eras(calendar) {
            return year.ok_or_else(|| self.type_error(p, "Missing year".into()));
        }
        let era_year = match (era, era_year) {
            (Some(era), Some(era_year)) => Some(
                quench_intl::calendar_year_from_era(era, era_year, calendar)
                    .ok_or_else(|| self.range_error(p, "Invalid era".into()))?,
            ),
            (Some(_), None) | (None, Some(_)) => {
                return Err(self.type_error(p, "era and eraYear must be provided together".into()));
            }
            (None, None) => None,
        };
        match (year, era_year) {
            (Some(year), Some(era_year)) if year != era_year => {
                Err(self.range_error(p, "Conflicting year and era fields".into()))
            }
            (Some(year), _) | (None, Some(year)) => Ok(year),
            (None, None) => Err(self.type_error(p, "Missing year".into())),
        }
    }

    pub(super) fn plain_date_optional_integer(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<Option<i32>, JsError> {
        if value.is_undefined() {
            return Ok(None);
        }
        self.plain_date_integer(p, value).map(Some)
    }

    fn temporal_plain_date_compare(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let constructor = self.native_value(Native::TemporalPlainDate);
        let left_args = [args.first().copied().unwrap_or(Value::UNDEFINED)];
        let right_args = [args.get(1).copied().unwrap_or(Value::UNDEFINED)];
        let left = self.temporal_plain_date_from(p, constructor, &left_args)?;
        let right = self.temporal_plain_date_from(p, constructor, &right_args)?;
        let left = self.temporal_plain_date_slots(p, left)?;
        let right = self.temporal_plain_date_slots(p, right)?;
        let ordering = (left.0, left.1, left.2).cmp(&(right.0, right.1, right.2));
        Ok(Value::number(match ordering {
            std::cmp::Ordering::Less => -1.0,
            std::cmp::Ordering::Equal => 0.0,
            std::cmp::Ordering::Greater => 1.0,
        }))
    }

    fn temporal_plain_date_equals(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let left = self.temporal_plain_date_slots(p, this)?;
        let right_value =
            self.temporal_plain_date_from(p, self.native_value(Native::TemporalPlainDate), args)?;
        let right = self.temporal_plain_date_slots(p, right_value)?;
        Ok(if left == right {
            Value::TRUE
        } else {
            Value::FALSE
        })
    }

    pub(super) fn temporal_plain_date_slots(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<(i32, u32, u32, String), JsError> {
        match self.heap.get(value) {
            Some(Cell::TemporalPlainDate {
                year,
                month,
                day,
                calendar,
                ..
            }) => Ok((*year, *month, *day, calendar.clone())),
            _ => Err(self.type_error(
                p,
                "Temporal.PlainDate method called on incompatible receiver".into(),
            )),
        }
    }
}

pub(super) fn iso_day_of_week(date: IsoDate) -> u32 {
    let weekday =
        (days_from_iso_date(date) + ISO_UNIX_EPOCH_WEEKDAY - 1).rem_euclid(ISO_DAYS_PER_WEEK) + 1;
    weekday as u32
}

pub(super) fn iso_day_of_year(date: IsoDate) -> u32 {
    (ISO_JANUARY..date.month)
        .filter_map(|month| iso_days_in_month(date.year, month as i32))
        .sum::<i32>() as u32
        + date.day
}

pub(super) fn iso_days_in_year(year: i32) -> u32 {
    (ISO_JANUARY..=ISO_MONTHS_PER_YEAR as u32)
        .filter_map(|month| iso_days_in_month(year, month as i32))
        .sum::<i32>() as u32
}

pub(super) fn iso_is_leap_year(year: i32) -> bool {
    iso_days_in_month(year, ISO_FEBRUARY) == Some(29)
}

fn iso_weeks_in_year(year: i32) -> i64 {
    let january_first = IsoDate {
        year,
        month: ISO_JANUARY,
        day: ISO_JANUARY_FIRST,
    };
    let weekday = i64::from(iso_day_of_week(january_first));
    if weekday == ISO_THURSDAY || (weekday == ISO_WEDNESDAY && iso_is_leap_year(year)) {
        ISO_LONG_WEEKS_PER_YEAR
    } else {
        ISO_COMMON_WEEKS_PER_YEAR
    }
}

pub(super) fn temporal_iso_week(date: IsoDate, calendar: &str) -> Option<(i32, i32)> {
    if calendar != "iso8601" {
        return None;
    }
    let week = (i64::from(iso_day_of_year(date)) - i64::from(iso_day_of_week(date))
        + ISO_WEEK_NUMBER_ADJUSTMENT)
        / ISO_DAYS_PER_WEEK;
    if week < 1 {
        Some((
            (week + iso_weeks_in_year(date.year - 1)) as i32,
            date.year - 1,
        ))
    } else if week > iso_weeks_in_year(date.year) {
        Some((1, date.year + 1))
    } else {
        Some((week as i32, date.year))
    }
}

pub(super) fn checked_iso_date(year: i32, month: i32, day: i32) -> Option<IsoDate> {
    if !(MIN_ISO_YEAR..=MAX_ISO_YEAR).contains(&year) {
        return None;
    }
    let month = u32::try_from(month).ok()?;
    let day = u32::try_from(day).ok()?;
    if !(1..=ISO_MONTHS_PER_YEAR as u32).contains(&month)
        || !(1..=iso_days_in_month(year, month as i32)? as u32).contains(&day)
    {
        return None;
    }
    let date = IsoDate { year, month, day };
    iso_date_in_range(date).then_some(date)
}

pub(super) fn iso_days_in_month(year: i32, month: i32) -> Option<i32> {
    quench_temporal::days_in_month(year, u32::try_from(month).ok()?).map(|days| days as i32)
}

pub(super) fn shift_iso_months(date: IsoDate, delta: i128) -> Option<IsoDate> {
    let month_index = i128::from(date.year) * i128::from(ISO_MONTHS_PER_YEAR)
        + i128::from(date.month - MONTHS_BEFORE_ISO_YEAR as u32)
        + delta;
    let year = i32::try_from(month_index.div_euclid(i128::from(ISO_MONTHS_PER_YEAR))).ok()?;
    let month = u32::try_from(month_index.rem_euclid(i128::from(ISO_MONTHS_PER_YEAR))).ok()? + 1;
    let day = date.day.min(iso_days_in_month(year, month as i32)? as u32);
    checked_iso_date(year, month as i32, day as i32)
}

pub(super) fn shift_iso_days(date: IsoDate, days: i64) -> Option<IsoDate> {
    let absolute_day = days_from_iso_date(date).checked_add(days)?;
    let shifted = iso_date_from_days(absolute_day)?;
    iso_date_in_range(shifted).then_some(shifted)
}

fn iso_date_in_range(date: IsoDate) -> bool {
    (date.year, date.month, date.day) >= (MIN_ISO_YEAR, 4, 19)
        && (date.year, date.month, date.day) <= (MAX_ISO_YEAR, 9, 13)
}

pub(super) fn days_from_iso_date(date: IsoDate) -> i64 {
    quench_temporal::days_from_civil(date.into())
}

fn iso_date_from_days(days: i64) -> Option<IsoDate> {
    quench_temporal::civil_from_days(days).map(Into::into)
}

impl From<IsoDate> for quench_temporal::IsoDate {
    fn from(date: IsoDate) -> Self {
        Self {
            year: date.year,
            month: date.month,
            day: date.day,
        }
    }
}

impl From<quench_temporal::IsoDate> for IsoDate {
    fn from(date: quench_temporal::IsoDate) -> Self {
        Self {
            year: date.year,
            month: date.month,
            day: date.day,
        }
    }
}

pub(super) fn format_iso_date(year: i32, month: u32, day: u32) -> String {
    let year = match year {
        0..=MAX_BASIC_ISO_YEAR => format!("{year:0width$}", width = BASIC_ISO_YEAR_DIGITS),
        year if year < 0 => format!(
            "-{:0width$}",
            year.unsigned_abs(),
            width = EXTENDED_ISO_YEAR_DIGITS
        ),
        year => format!("+{year:0width$}", width = EXTENDED_ISO_YEAR_DIGITS),
    };
    format!(
        "{year}-{month:0width$}-{day:0width$}",
        width = ISO_MONTH_DAY_DIGITS
    )
}
