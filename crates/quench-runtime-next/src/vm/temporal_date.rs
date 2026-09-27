use super::*;
const ISO_MONTHS_PER_YEAR: i32 = 12;
const MIN_ISO_YEAR: i32 = -271_821;
const MAX_ISO_YEAR: i32 = 275_760;
const MAX_BASIC_ISO_YEAR: i32 = 9_999;
const BASIC_ISO_YEAR_DIGITS: usize = 4;
const EXTENDED_ISO_YEAR_DIGITS: usize = 6;
const ISO_MONTH_DAY_DIGITS: usize = 2;
const ISO_MONTH_CODE_DIGITS: usize = 2;
const MONTHS_BEFORE_ISO_YEAR: i32 = 1;

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
            ("daysInMonth", Native::TemporalPlainDateDaysInMonthGetter),
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
        let calendar = calendar.to_ascii_lowercase();
        if !matches!(calendar.as_str(), "iso8601" | "gregory") {
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
        match native {
            Native::TemporalPlainDate => {
                Err(self.type_error(p, "Temporal.PlainDate requires new".into()))
            }
            Native::TemporalPlainDateFrom => self.temporal_plain_date_from(p, this, args),
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
            Native::TemporalPlainDateToJSON | Native::TemporalPlainDateToLocaleString => {
                let (year, month, day, _) = self.temporal_plain_date_slots(p, this)?;
                Ok(self
                    .heap
                    .alloc(Cell::String(format_iso_date(year, month, day).into())))
            }
            Native::TemporalPlainDateCalendarIdGetter
            | Native::TemporalPlainDateYearGetter
            | Native::TemporalPlainDateMonthGetter
            | Native::TemporalPlainDateMonthCodeGetter
            | Native::TemporalPlainDateDayGetter
            | Native::TemporalPlainDateDaysInMonthGetter => {
                let (year, month, day, calendar) = self.temporal_plain_date_slots(p, this)?;
                Ok(match native {
                    Native::TemporalPlainDateCalendarIdGetter => {
                        self.heap.alloc(Cell::String(calendar.into()))
                    }
                    Native::TemporalPlainDateYearGetter => Value::number(f64::from(year)),
                    Native::TemporalPlainDateMonthGetter => Value::number(f64::from(month)),
                    Native::TemporalPlainDateMonthCodeGetter => self.heap.alloc(Cell::String(
                        format!("M{month:0width$}", width = ISO_MONTH_DAY_DIGITS).into(),
                    )),
                    Native::TemporalPlainDateDaysInMonthGetter => Value::number(f64::from(
                        iso_days_in_month(year, month as i32).unwrap_or(31),
                    )),
                    _ => Value::number(f64::from(day)),
                })
            }
            _ => unreachable!("not a Temporal.PlainDate native"),
        }
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
        let (date, calendar) = if let Some(Cell::String(text)) = self.heap.get(value) {
            let text = text.host_string().to_owned();
            let _ = self.plain_date_overflow(p, options)?;
            let (date, calendar) = temporal_date_parse::parse_plain_date_string(&text)
                .ok_or_else(|| self.range_error(p, "Invalid calendar".into()))?;
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

    fn temporal_plain_date_to_string(
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
        let calendar = if calendar.is_undefined() {
            "iso8601".to_owned()
        } else if matches!(self.heap.get(calendar), Some(Cell::String(_))) {
            let value = self.to_string(p, calendar)?.to_string();
            temporal_date_parse::parse_calendar_identifier(&value)
                .ok_or_else(|| self.range_error(p, "Invalid calendar".into()))?
        } else {
            return Err(self.type_error(p, "Invalid calendar".into()));
        };
        if !matches!(calendar.as_str(), "iso8601" | "gregory") {
            return Err(self.range_error(p, "Invalid calendar".into()));
        }
        let day = self.get_property(p, value, day_atom)?;
        let day = self.plain_date_optional_integer(p, day)?;
        let month = self.get_property(p, value, month_atom)?;
        let month = self.plain_date_optional_integer(p, month)?;
        let month_code_value = self.get_property(p, value, month_code_atom)?;
        let month_code = if month_code_value.is_undefined() {
            None
        } else {
            Some(self.plain_date_month_code(p, month_code_value)?)
        };
        let year = self.get_property(p, value, year_atom)?;
        let year = self.plain_date_optional_integer(p, year)?;
        let constrain = self.plain_date_overflow(p, options)?;
        let (Some(day), Some(year)) = (day, year) else {
            return Err(self.type_error(p, "Missing PlainDate field".into()));
        };
        let month = match (month, month_code) {
            (Some(month), Some(month_code)) if month != month_code => {
                return Err(self.range_error(p, "month and monthCode must agree".into()));
            }
            (Some(month), _) => month,
            (None, Some(month_code)) => month_code,
            (None, None) => return Err(self.type_error(p, "Missing PlainDate field".into())),
        };
        let (month, day) = if constrain {
            let month = month.clamp(1, ISO_MONTHS_PER_YEAR);
            let last_day = iso_days_in_month(year, month).unwrap_or(31);
            (month, day.clamp(1, last_day))
        } else {
            (month, day)
        };
        let date = checked_iso_date(year, month, day)
            .ok_or_else(|| self.range_error(p, "Invalid PlainDate".into()))?;
        Ok((date, calendar))
    }

    pub(super) fn plain_date_month_code(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<i32, JsError> {
        let code = self.to_string(p, value)?.to_string();
        let month = code
            .strip_prefix('M')
            .filter(|month| {
                month.len() == ISO_MONTH_CODE_DIGITS
                    && month.bytes().all(|byte| byte.is_ascii_digit())
            })
            .and_then(|month| month.parse::<i32>().ok())
            .filter(|month| (1..=ISO_MONTHS_PER_YEAR).contains(month));
        month.ok_or_else(|| self.range_error(p, "Invalid monthCode".into()))
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

fn format_iso_date(year: i32, month: u32, day: u32) -> String {
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
