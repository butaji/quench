use super::*;
use chrono::{Datelike, NaiveDate};

const DURATION_FIELDS: [&str; 10] = [
    "years",
    "months",
    "weeks",
    "days",
    "hours",
    "minutes",
    "seconds",
    "milliseconds",
    "microseconds",
    "nanoseconds",
];
const DURATION_DATE_FIELD_LIMIT: f64 = 4_294_967_295.0;
const DURATION_TOTAL_TIME_LIMIT_NANOS: i128 = 9_007_199_254_740_991_i128 * 1_000_000_000 + 999_999_999;
const DURATION_TOTAL_DECIMAL_DIGITS: usize = 32;
const DURATION_ROUNDING_INCREMENT_LIMIT: f64 = 1_000_000_000.0;
const DURATION_ROUNDING_MODE_NAMES: [&str; 9] = [
    "ceil", "floor", "expand", "trunc", "halfCeil", "halfFloor", "halfExpand",
    "halfTrunc", "halfEven",
];
const DURATION_ROUNDING_INCREMENT_LIMITS: [f64; 6] = [24.0, 60.0, 60.0, 1_000.0, 1_000.0, 1_000.0];
const DURATION_TIME_NANOSECOND_SCALES: [i128; 7] = [
    86_400_000_000_000,
    3_600_000_000_000,
    60_000_000_000,
    1_000_000_000,
    1_000_000,
    1_000,
    1,
];
const NANOS_PER_DAY: i128 = 86_400_000_000_000;

fn parse_relative_date(text: &str) -> Option<NaiveDate> {
    let date = text.split(['T', 't', '[', ' ']).next()?;
    if date.len() == 8 && date.bytes().all(|byte| byte.is_ascii_digit()) {
        return NaiveDate::parse_from_str(date, "%Y%m%d").ok();
    }
    NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()
}

fn duration_relative_nanoseconds(start: NaiveDate, fields: &[f64; 10]) -> Option<i128> {
    let mut target = shift_relative_months(start, fields[0] as i128 * 12)?;
    target = shift_relative_months(target, fields[1] as i128)?;
    let calendar_days = fields[2] as i128 * 7 + fields[3] as i128;
    target = target.checked_add_signed(chrono::Duration::days(i64::try_from(calendar_days).ok()?))?;
    let elapsed_days = i128::from((target - start).num_days());
    let elapsed_time = fields[4..]
        .iter()
        .zip(DURATION_TIME_NANOSECOND_SCALES[1..].iter())
        .map(|(value, scale)| *value as i128 * scale)
        .sum::<i128>();
    elapsed_days.checked_mul(NANOS_PER_DAY)?.checked_add(elapsed_time)
}

pub(super) fn shift_relative_months(date: NaiveDate, months: i128) -> Option<NaiveDate> {
    let month_index = i128::from(date.year()) * 12 + i128::from(date.month0()) + months;
    let year = i32::try_from(month_index.div_euclid(12)).ok()?;
    let month = u32::try_from(month_index.rem_euclid(12)).ok()? + 1;
    let first_of_next_month = if month == 12 {
        NaiveDate::from_ymd_opt(year.checked_add(1)?, 1, 1)?
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)?
    };
    let last_day = (first_of_next_month - chrono::Duration::days(1)).day();
    NaiveDate::from_ymd_opt(year, month, date.day().min(last_day))
}

impl<H: Host> Vm<H> {
    pub(super) fn install_temporal(&mut self, p: &ResidualProgram) -> Result<(), JsError> {
        let temporal = self.object();
        let duration = self.native_with_realm(Native::TemporalDuration, temporal, self.realm.globals);
        self.set_builtin_function_name(duration, "Duration")?;
        let prototype = self.object();
        self.set_builtin_value_named(duration, "prototype", prototype)?;
        self.set_builtin_value_named(prototype, "constructor", duration)?;
        for (name, native) in [
            ("from", Native::TemporalDurationFrom),
            ("compare", Native::TemporalDurationCompare),
        ] {
            self.set_builtin_named(p, duration, name, native)?;
        }
        for (name, native) in DURATION_FIELDS
            .into_iter()
            .zip([
                Native::TemporalDurationYearsGetter,
                Native::TemporalDurationMonthsGetter,
                Native::TemporalDurationWeeksGetter,
                Native::TemporalDurationDaysGetter,
                Native::TemporalDurationHoursGetter,
                Native::TemporalDurationMinutesGetter,
                Native::TemporalDurationSecondsGetter,
                Native::TemporalDurationMillisecondsGetter,
                Native::TemporalDurationMicrosecondsGetter,
                Native::TemporalDurationNanosecondsGetter,
            ])
            .chain([
                ("sign", Native::TemporalDurationSignGetter),
                ("blank", Native::TemporalDurationBlankGetter),
            ])
        {
            let getter = self.native_value(native);
            self.set_builtin_function_name(getter, &format!("get {name}"))?;
            let name_atom = self.intern_atom(name);
            self.set_property(prototype, name_atom, Value::UNDEFINED)?;
            self.set_property_attributes(
                prototype,
                PropertyKey::string(name_atom),
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
            ("toString", Native::TemporalDurationToString),
            ("toJSON", Native::TemporalDurationToJSON),
            ("valueOf", Native::TemporalDurationValueOf),
            ("add", Native::TemporalDurationAdd),
            ("subtract", Native::TemporalDurationSubtract),
            ("with", Native::TemporalDurationWith),
            ("abs", Native::TemporalDurationAbs),
            ("negated", Native::TemporalDurationNegated),
            ("total", Native::TemporalDurationTotal),
            ("round", Native::TemporalDurationRound),
        ] {
            self.set_builtin_named(p, prototype, name, native)?;
        }
        self.set_builtin_value_named(temporal, "Duration", duration)?;
        self.install_temporal_plain_date(p, temporal)?;
        self.set_builtin_value_named(self.realm.globals, "Temporal", temporal)
    }

    pub(super) fn temporal_duration_construct(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let mut fields = [0.0; 10];
        for (index, field) in fields.iter_mut().enumerate() {
            let value = args.get(index).copied().unwrap_or(Value::UNDEFINED);
            let number = if value.is_undefined() {
                0.0
            } else {
                self.to_number(p, value)?
            };
            if !number.is_finite() || number.fract() != 0.0 {
                return Err(self.range_error(p, "Duration fields must be integral".into()));
            }
            *field = if number == 0.0 { 0.0 } else { number };
        }
        self.validate_duration_fields(p, &fields)?;
        Ok(self.heap.alloc(Cell::TemporalDuration {
            object: Box::new(Self::empty_object(self.object_proto)),
            fields,
        }))
    }

    pub(super) fn validate_duration_fields(
        &mut self,
        p: &ResidualProgram,
        fields: &[f64; 10],
    ) -> Result<(), JsError> {
        if fields
            .iter()
            .any(|value| !value.is_finite() || value.fract() != 0.0)
        {
            return Err(self.range_error(p, "Duration fields must be integral".into()));
        }
        let sign = fields
            .iter()
            .find(|value| **value != 0.0)
            .map_or(0.0, |value| value.signum());
        if fields
            .iter()
            .any(|value| *value != 0.0 && value.signum() != sign)
        {
            return Err(self.range_error(p, "Duration fields must have a consistent sign".into()));
        }
        if fields[..3]
            .iter()
            .any(|value| value.abs() > DURATION_DATE_FIELD_LIMIT)
            || duration_total_time_out_of_range(&fields[3..])
        {
            return Err(self.range_error(p, "Duration time is outside the supported range".into()));
        }
        Ok(())
    }

    fn duration_fields(&mut self, p: &ResidualProgram, value: Value) -> Result<[f64; 10], JsError> {
        match self.heap.get(value) {
            Some(Cell::TemporalDuration { fields, .. }) => Ok(*fields),
            _ => Err(self.type_error(p, "Temporal.Duration method called on incompatible receiver".into())),
        }
    }

    pub(super) fn temporal_duration_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::TemporalDuration => Err(self.type_error(p, "Temporal.Duration requires new".into())),
            Native::TemporalDurationFrom => {
                let input = args.first().copied().unwrap_or(Value::UNDEFINED);
                let fields = self.duration_record(p, input)?;
                self.validate_duration_fields(p, &fields)?;
                self.make_temporal_duration(p, fields)
            }
            Native::TemporalDurationAbs | Native::TemporalDurationNegated => {
                let mut fields = self.duration_fields(p, this)?;
                let negate = native == Native::TemporalDurationNegated;
                for field in &mut fields {
                    *field = if negate { -*field } else { field.abs() };
                    if *field == 0.0 {
                        *field = 0.0;
                    }
                }
                self.make_temporal_duration(p, fields)
            }
            Native::TemporalDurationTotal => self.temporal_duration_total(p, this, args),
            Native::TemporalDurationRound => self.temporal_duration_round(p, this, args),
            Native::TemporalDurationWith => {
                let mut fields = self.duration_fields(p, this)?;
                let options = args.first().copied().unwrap_or(Value::UNDEFINED);
                if !self.is_object_like(options) {
                    return Err(self.type_error(p, "Duration.with requires an object".into()));
                }
                let mut present = false;
                for index in [3, 4, 8, 7, 5, 1, 9, 6, 2, 0] {
                    let atom = self.intern_atom(DURATION_FIELDS[index]);
                    let value = self.get_property(p, options, atom)?;
                    if value.is_undefined() {
                        continue;
                    }
                    present = true;
                    fields[index] = self.to_number(p, value)?;
                }
                if !present {
                    return Err(self.type_error(p, "Duration.with requires a duration field".into()));
                }
                self.validate_duration_fields(p, &fields)?;
                self.make_temporal_duration(p, fields)
            }
            Native::TemporalDurationAdd | Native::TemporalDurationSubtract => {
                let left = self.duration_fields(p, this)?;
                let right = self.duration_record(
                    p,
                    args.first().copied().unwrap_or(Value::UNDEFINED),
                )?;
                self.validate_duration_fields(p, &right)?;
                if left[..3].iter().chain(right[..3].iter()).any(|value| *value != 0.0) {
                    return Err(self.range_error(p, "relativeTo required for calendar units".into()));
                }
                let fields = balance_duration_time(
                    &left,
                    &right,
                    native == Native::TemporalDurationSubtract,
                );
                self.validate_duration_fields(p, &fields)?;
                self.make_temporal_duration(p, fields)
            }
            Native::TemporalDurationCompare => {
                let options = args.get(2).copied().unwrap_or(Value::UNDEFINED);
                let left = self.duration_record(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let right = self.duration_record(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
                if !options.is_undefined() && !self.is_object_like(options) {
                    return Err(self.type_error(p, "Duration.compare options must be an object".into()));
                }
                let relative_to = if self.is_object_like(options) {
                    let atom = self.intern_atom("relativeTo");
                    self.get_property(p, options, atom)?
                } else {
                    Value::UNDEFINED
                };
                self.validate_duration_fields(p, &left)?;
                self.validate_duration_fields(p, &right)?;
                if left == right {
                    if !relative_to.is_undefined() {
                        self.temporal_relative_date(p, relative_to)?;
                    }
                    return Ok(Value::number(0.0));
                }
                if left[..3].iter().chain(right[..3].iter()).any(|value| *value != 0.0)
                    && relative_to.is_undefined()
                {
                    return Err(self.range_error(p, "relativeTo is required to compare calendar units".into()));
                }
                let ordering = if relative_to.is_undefined() {
                    self.duration_time_nanos(&left)
                        .cmp(&self.duration_time_nanos(&right))
                } else {
                    let date = self.temporal_relative_date(p, relative_to)?;
                    let left = duration_relative_nanoseconds(date, &left)
                        .ok_or_else(|| self.range_error(p, "Invalid relativeTo".into()))?;
                    let right = duration_relative_nanoseconds(date, &right)
                        .ok_or_else(|| self.range_error(p, "Invalid relativeTo".into()))?;
                    left.cmp(&right)
                };
                Ok(Value::number(match ordering {
                    std::cmp::Ordering::Less => -1.0,
                    std::cmp::Ordering::Equal => 0.0,
                    std::cmp::Ordering::Greater => 1.0,
                }))
            }
            Native::TemporalDurationToString | Native::TemporalDurationToJSON => {
                let fields = self.duration_fields(p, this)?;
                let text = if native == Native::TemporalDurationToString {
                    self.temporal_duration_to_string(p, &fields, args)?
                } else {
                    format_duration(&fields)
                };
                Ok(self.heap.alloc(Cell::String(JsString::from_str(&text))))
            }
            Native::TemporalDurationValueOf => Err(self.type_error(p, "Temporal.Duration.prototype.valueOf is not allowed".into())),
            Native::TemporalDurationSignGetter | Native::TemporalDurationBlankGetter
            | Native::TemporalDurationYearsGetter | Native::TemporalDurationMonthsGetter
            | Native::TemporalDurationWeeksGetter | Native::TemporalDurationDaysGetter
            | Native::TemporalDurationHoursGetter | Native::TemporalDurationMinutesGetter
            | Native::TemporalDurationSecondsGetter | Native::TemporalDurationMillisecondsGetter
            | Native::TemporalDurationMicrosecondsGetter | Native::TemporalDurationNanosecondsGetter => {
                let fields = self.duration_fields(p, this)?;
                if native == Native::TemporalDurationSignGetter {
                    let sign = fields.iter().find(|value| **value != 0.0).map_or(0.0, |value| value.signum());
                    return Ok(Value::number(sign));
                }
                if native == Native::TemporalDurationBlankGetter {
                    return Ok(if fields.iter().all(|value| *value == 0.0) { Value::TRUE } else { Value::FALSE });
                }
                let index = match native {
                    Native::TemporalDurationYearsGetter => 0,
                    Native::TemporalDurationMonthsGetter => 1,
                    Native::TemporalDurationWeeksGetter => 2,
                    Native::TemporalDurationDaysGetter => 3,
                    Native::TemporalDurationHoursGetter => 4,
                    Native::TemporalDurationMinutesGetter => 5,
                    Native::TemporalDurationSecondsGetter => 6,
                    Native::TemporalDurationMillisecondsGetter => 7,
                    Native::TemporalDurationMicrosecondsGetter => 8,
                    Native::TemporalDurationNanosecondsGetter => 9,
                    _ => unreachable!(),
                };
                Ok(Value::number(fields[index]))
            }
            _ => unreachable!("not a Temporal.Duration native"),
        }
    }

    pub(super) fn duration_record(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<[f64; 10], JsError> {
        if let Some(Cell::TemporalDuration { fields, .. }) = self.heap.get(value) {
            return Ok(*fields);
        }
        if let Some(Cell::String(text)) = self.heap.get(value) {
            return parse_duration_string(text.host_string())
                .map_err(|()| self.range_error(p, "Invalid duration string".into()));
        }
        if !self.is_object_like(value) {
            return Err(self.type_error(p, "Duration-like value must be an object or string".into()));
        }
        let mut fields = [0.0; 10];
        let mut present = false;
        let read_order = [3, 4, 8, 7, 5, 1, 9, 6, 2, 0];
        for index in read_order {
            let atom = self.intern_atom(DURATION_FIELDS[index]);
            let value = self.get_property(p, value, atom)?;
            if value.is_undefined() {
                continue;
            }
            present = true;
            let number = self.to_number(p, value)?;
            fields[index] = number;
        }
        if !present {
            return Err(self.type_error(p, "Duration requires at least one field".into()));
        }
        Ok(fields)
    }

    fn make_temporal_duration(
        &mut self,
        p: &ResidualProgram,
        fields: [f64; 10],
    ) -> Result<Value, JsError> {
        let temporal_atom = self.intern_atom("Temporal");
        let temporal = self.get_property(p, self.realm.globals, temporal_atom)?;
        let duration_atom = self.intern_atom("Duration");
        let constructor = self.get_property(p, temporal, duration_atom)?;
        let prototype_atom = self.intern_atom("prototype");
        let prototype = self.get_property(p, constructor, prototype_atom)?;
        let value = self.heap.alloc(Cell::TemporalDuration {
            object: Box::new(Self::empty_object(self.object_proto)),
            fields,
        });
        if let Some(Cell::TemporalDuration { object, .. }) = self.heap.get_mut(value) {
            object.proto = prototype;
        }
        Ok(value)
    }

    fn duration_time_nanos(&self, fields: &[f64; 10]) -> i128 {
        fields[3..]
            .iter()
            .zip(DURATION_TIME_NANOSECOND_SCALES)
            .map(|(value, scale)| *value as i128 * scale)
            .sum()
    }

    fn temporal_relative_date(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<NaiveDate, JsError> {
        if let Some(Cell::String(text)) = self.heap.get(value) {
            return parse_relative_date(text.host_string())
                .ok_or_else(|| self.range_error(p, "Invalid relativeTo".into()));
        }
        if let Some(Cell::TemporalPlainDate {
            year, month, day, ..
        }) = self.heap.get(value)
        {
            return NaiveDate::from_ymd_opt(*year, *month, *day)
                .ok_or_else(|| self.range_error(p, "Invalid relativeTo".into()));
        }
        if !self.is_object_like(value) {
            return Err(self.type_error(p, "Invalid relativeTo".into()));
        }
        let year_atom = self.intern_atom("year");
        let month_atom = self.intern_atom("month");
        let day_atom = self.intern_atom("day");
        let year = self.get_property(p, value, year_atom)?;
        let month = self.get_property(p, value, month_atom)?;
        let day = self.get_property(p, value, day_atom)?;
        let year = self.to_number(p, year)?;
        let month = self.to_number(p, month)?;
        let day = self.to_number(p, day)?;
        if !year.is_finite()
            || !month.is_finite()
            || !day.is_finite()
            || year.fract() != 0.0
            || month.fract() != 0.0
            || day.fract() != 0.0
        {
            return Err(self.range_error(p, "Invalid relativeTo".into()));
        }
        NaiveDate::from_ymd_opt(year as i32, month as u32, day as u32)
            .ok_or_else(|| self.range_error(p, "Invalid relativeTo".into()))
    }

    fn temporal_duration_total(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let fields = self.duration_fields(p, this)?;
        let options = args.first().copied().unwrap_or(Value::UNDEFINED);
        let (unit, relative_to) = match self.heap.get(options) {
            Some(Cell::String(text)) => (text.to_string(), Value::UNDEFINED),
            _ if self.is_object_like(options) => {
                let relative_to_atom = self.intern_atom("relativeTo");
                let relative_to = self.get_property(p, options, relative_to_atom)?;
                let unit_atom = self.intern_atom("unit");
                let unit = self.get_property(p, options, unit_atom)?;
                if unit.is_undefined() {
                    return Err(self.range_error(p, "unit is required".into()));
                }
                (self.to_string(p, unit)?.to_string(), relative_to)
            }
            _ => return Err(self.type_error(p, "Options must be an object or unit string".into())),
        };
        let unit = unit.strip_suffix('s').unwrap_or(&unit);
        let index = DURATION_FIELDS
            .iter()
            .position(|field| field.strip_suffix('s').unwrap_or(field) == unit)
            .ok_or_else(|| self.range_error(p, "Invalid unit".into()))?;
        if (index <= 2 || fields[..3].iter().any(|value| *value != 0.0))
            && relative_to.is_undefined()
        {
            return Err(self.range_error(p, "relativeTo required".into()));
        }
        if index <= 2 || fields[..3].iter().any(|value| *value != 0.0) {
            return Err(self.range_error(p, "relativeTo required for calendar units".into()));
        }
        if !relative_to.is_undefined()
            && !matches!(self.heap.get(relative_to), Some(Cell::String(_)))
            && !self.is_object_like(relative_to)
        {
            return Err(self.type_error(p, "relativeTo must be a string or object".into()));
        }
        let divisor = DURATION_TIME_NANOSECOND_SCALES[index - 3];
        Ok(Value::number(divide_duration_nanos(
            self.duration_time_nanos(&fields),
            divisor,
        )))
    }

    fn temporal_duration_to_string(
        &mut self,
        p: &ResidualProgram,
        fields: &[f64; 10],
        args: &[Value],
    ) -> Result<String, JsError> {
        let options = args.first().copied().unwrap_or(Value::UNDEFINED);
        if options.is_undefined() {
            return Ok(format_duration(fields));
        }
        if !self.is_object_like(options) {
            return Err(self.type_error(p, "Options must be an object".into()));
        }
        let fractional_atom = self.intern_atom("fractionalSecondDigits");
        let fractional = self.get_property(p, options, fractional_atom)?;
        let digits = self.duration_fractional_digits(p, fractional)?;
        let rounding_atom = self.intern_atom("roundingMode");
        let rounding = self.get_property(p, options, rounding_atom)?;
        let rounding_mode = self.duration_rounding_mode(p, rounding, "trunc")?;
        let smallest_atom = self.intern_atom("smallestUnit");
        let smallest = self.get_property(p, options, smallest_atom)?;
        let digits = self
            .duration_smallest_unit_digits(p, smallest)?
            .or(digits);
        let rounded = round_duration_for_string(fields, digits, &rounding_mode);
        self.validate_duration_fields(p, &rounded)?;
        Ok(format_duration_with_digits(&rounded, digits))
    }

    fn duration_fractional_digits(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<Option<usize>, JsError> {
        if value.is_undefined() {
            return Ok(None);
        }
        if value.as_number().is_some() {
            let number = self.to_number(p, value)?.floor();
            if !number.is_finite() || !(0.0..=9.0).contains(&number) {
                return Err(self.range_error(p, "Invalid fractionalSecondDigits".into()));
            }
            return Ok(Some(number as usize));
        }
        let text = self.to_string(p, value)?.to_string();
        if text == "auto" {
            Ok(None)
        } else {
            Err(self.range_error(p, "Invalid fractionalSecondDigits".into()))
        }
    }

    fn duration_rounding_mode(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        default: &str,
    ) -> Result<String, JsError> {
        if value.is_undefined() {
            return Ok(default.to_owned());
        }
        let mode = self.to_string(p, value)?.to_string();
        if DURATION_ROUNDING_MODE_NAMES.contains(&mode.as_str()) {
            Ok(mode)
        } else {
            Err(self.range_error(p, "Invalid roundingMode".into()))
        }
    }

    fn duration_smallest_unit_digits(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<Option<usize>, JsError> {
        if value.is_undefined() {
            return Ok(None);
        }
        let unit = self.to_string(p, value)?.to_string();
        let digits = match unit.as_str() {
            "second" | "seconds" => 0,
            "millisecond" | "milliseconds" => 3,
            "microsecond" | "microseconds" => 6,
            "nanosecond" | "nanoseconds" => 9,
            _ => return Err(self.range_error(p, "Invalid smallestUnit".into())),
        };
        Ok(Some(digits))
    }

    fn temporal_duration_round(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let fields = self.duration_fields(p, this)?;
        let options = args.first().copied().unwrap_or(Value::UNDEFINED);
        let (smallest, largest, increment, mode, relative_to, explicit_unit) =
            self.duration_round_options(p, options)?;
        let smallest = smallest.ok_or_else(|| self.range_error(p, "smallestUnit is required".into()))?;
        if !explicit_unit {
            return Err(self.range_error(p, "largestUnit or smallestUnit is required".into()));
        }
        let largest = largest.unwrap_or_else(|| {
            (0..10)
                .find(|index| fields[*index] != 0.0)
                .unwrap_or(smallest)
                .min(smallest)
        });
        if largest > smallest {
            return Err(self.range_error(p, "largestUnit must not be smaller than smallestUnit".into()));
        }
        if (smallest <= 2 || largest <= 2 || fields[..3].iter().any(|value| *value != 0.0))
            && relative_to.is_undefined()
        {
            return Err(self.range_error(p, "relativeTo required for calendar units".into()));
        }
        if smallest <= 2 || largest <= 2 || fields[..3].iter().any(|value| *value != 0.0) {
            return Err(self.range_error(p, "relativeTo required for calendar rounding".into()));
        }
        let nanos = self.duration_time_nanos(&fields);
        let quantum = DURATION_TIME_NANOSECOND_SCALES[smallest - 3] * increment as i128;
        let rounded_units = round_duration_integer(nanos, quantum, &mode);
        let mut remainder = (rounded_units * quantum).unsigned_abs();
        let mut result = [0.0; 10];
        let sign = rounded_units.signum();
        for unit in largest.max(3)..=smallest {
            let scale = DURATION_TIME_NANOSECOND_SCALES[unit - 3] as u128;
            let component = remainder / scale;
            result[unit] = component as f64 * sign as f64;
            remainder %= scale;
        }
        result.iter_mut().for_each(|value| {
            if *value == 0.0 {
                *value = 0.0;
            }
        });
        self.validate_duration_fields(p, &result)?;
        self.make_temporal_duration(p, result)
    }

    fn duration_round_options(
        &mut self,
        p: &ResidualProgram,
        options: Value,
    ) -> Result<(Option<usize>, Option<usize>, i128, String, Value, bool), JsError> {
        let is_string = matches!(self.heap.get(options), Some(Cell::String(_)));
        if !is_string && !self.is_object_like(options) {
            return Err(self.type_error(p, "Options must be an object or string".into()));
        }
        let (smallest_text, largest_text, relative_to, increment, mode) = if is_string {
            (
                Some(self.to_string(p, options)?.to_string()),
                None,
                Value::UNDEFINED,
                1.0,
                "halfExpand".to_owned(),
            )
        } else {
            let largest_atom = self.intern_atom("largestUnit");
            let largest = self.get_property(p, options, largest_atom)?;
            let largest = if largest.is_undefined() {
                None
            } else {
                Some(self.to_string(p, largest)?.to_string())
            };
            let relative_atom = self.intern_atom("relativeTo");
            let relative = self.get_property(p, options, relative_atom)?;
            let increment_atom = self.intern_atom("roundingIncrement");
            let increment = self.get_property(p, options, increment_atom)?;
            let increment = if increment.is_undefined() {
                1.0
            } else {
                self.to_number(p, increment)?.trunc()
            };
            let mode_atom = self.intern_atom("roundingMode");
            let mode = self.get_property(p, options, mode_atom)?;
            let mode = if mode.is_undefined() {
                "halfExpand".to_owned()
            } else {
                self.to_string(p, mode)?.to_string()
            };
            let smallest_atom = self.intern_atom("smallestUnit");
            let smallest = self.get_property(p, options, smallest_atom)?;
            let smallest = if smallest.is_undefined() {
                None
            } else {
                Some(self.to_string(p, smallest)?.to_string())
            };
            (smallest, largest, relative, increment, mode)
        };
        let largest = match largest_text.as_deref() {
            Some("auto") | None => None,
            Some(text) => Some(parse_duration_unit(text).ok_or_else(|| {
                self.range_error(p, "Invalid largestUnit".into())
            })?),
        };
        let explicit_unit = smallest_text.is_some() || largest_text.is_some();
        let smallest = if let Some(text) = smallest_text.as_deref() {
            Some(parse_duration_unit(text).ok_or_else(|| {
                self.range_error(p, "Invalid smallestUnit".into())
            })?)
        } else if largest.is_some_and(|index| index <= 2) {
            largest
        } else {
            Some(9)
        };
        if !increment.is_finite() || increment <= 0.0 || increment > DURATION_ROUNDING_INCREMENT_LIMIT {
            return Err(self.range_error(p, "Invalid roundingIncrement".into()));
        }
        if let Some(index) = smallest.filter(|index| *index >= 4) {
            let maximum = DURATION_ROUNDING_INCREMENT_LIMITS[index - 4];
            if increment >= maximum || maximum % increment != 0.0 {
                return Err(self.range_error(p, "Invalid roundingIncrement".into()));
            }
        }
        if !DURATION_ROUNDING_MODE_NAMES.contains(&mode.as_str()) {
            return Err(self.range_error(p, "Invalid roundingMode".into()));
        }
        if !relative_to.is_undefined()
            && !matches!(self.heap.get(relative_to), Some(Cell::String(_)))
            && !self.is_object_like(relative_to)
        {
            return Err(self.type_error(p, "relativeTo must be a string or object".into()));
        }
        Ok((smallest, largest, increment as i128, mode, relative_to, explicit_unit))
    }
}

fn duration_total_time_out_of_range(values: &[f64]) -> bool {
    let total = values
        .iter()
        .zip(DURATION_TIME_NANOSECOND_SCALES)
        .try_fold(0_i128, |total, (value, scale)| {
            let value = *value as i128;
            total.checked_add(value.checked_mul(scale)?)
        });
    total.is_none_or(|total| total.unsigned_abs() > DURATION_TOTAL_TIME_LIMIT_NANOS as u128)
}

fn balance_duration_time(
    left: &[f64; 10],
    right: &[f64; 10],
    subtract: bool,
) -> [f64; 10] {
    let direction = if subtract { -1_i128 } else { 1_i128 };
    let mut result = [0.0; 10];
    for (index, value) in result.iter_mut().enumerate().take(3) {
        *value = left[index];
    }
    let days = left[3] + direction as f64 * right[3];
    let total = left[4..]
        .iter()
        .zip(&right[4..])
        .zip(DURATION_TIME_NANOSECOND_SCALES[1..].iter().copied())
        .map(|((left, right), scale)| (*left as i128 + direction * *right as i128) * scale)
        .sum::<i128>();
    let sign = total.signum();
    let mut remainder = total.unsigned_abs();
    let day_nanos = DURATION_TIME_NANOSECOND_SCALES[0] as u128;
    let largest = if days != 0.0 {
        3
    } else {
        (4..10)
            .find(|index| left[*index] + direction as f64 * right[*index] != 0.0)
            .unwrap_or(9)
    };
    if largest == 3 {
        result[3] = days + sign as f64 * (remainder / day_nanos) as f64;
        remainder %= day_nanos;
    } else {
        result[3] = days;
    }
    for index in largest.max(4)..10 {
        let scale = DURATION_TIME_NANOSECOND_SCALES[index - 3] as u128;
        result[index] = (remainder / scale) as f64 * sign as f64;
        remainder %= scale;
    }
    result.iter_mut().for_each(|value| {
        if *value == 0.0 {
            *value = 0.0;
        }
    });
    result
}

fn divide_duration_nanos(nanos: i128, divisor: i128) -> f64 {
    let negative = nanos < 0;
    let absolute = nanos.unsigned_abs();
    let divisor = divisor as u128;
    let whole = absolute / divisor;
    let mut remainder = absolute % divisor;
    if remainder == 0 {
        return if negative { -(whole as f64) } else { whole as f64 };
    }
    let mut decimal = format!("{whole}.");
    for _ in 0..DURATION_TOTAL_DECIMAL_DIGITS {
        if remainder == 0 {
            break;
        }
        remainder *= 10;
        decimal.push(char::from(b'0' + (remainder / divisor) as u8));
        remainder %= divisor;
    }
    let value = decimal.parse::<f64>().unwrap_or(f64::INFINITY);
    if negative { -value } else { value }
}

fn parse_duration_unit(unit: &str) -> Option<usize> {
    let unit = unit.strip_suffix('s').unwrap_or(unit);
    DURATION_FIELDS
        .iter()
        .position(|field| field.strip_suffix('s').unwrap_or(field) == unit)
}

fn round_duration_integer(value: i128, quantum: i128, mode: &str) -> i128 {
    let sign = value.signum();
    let absolute = value.unsigned_abs();
    let quantum = quantum as u128;
    let mut units = absolute / quantum;
    let remainder = absolute % quantum;
    let increment = match mode {
        "ceil" => sign > 0 && remainder != 0,
        "floor" => sign < 0 && remainder != 0,
        "expand" => remainder != 0,
        "trunc" => false,
        "halfEven" => remainder * 2 > quantum || remainder * 2 == quantum && units % 2 != 0,
        "halfCeil" => remainder * 2 >= quantum && sign > 0 || remainder * 2 > quantum && sign < 0,
        "halfFloor" => remainder * 2 > quantum && sign > 0 || remainder * 2 >= quantum && sign < 0,
        "halfTrunc" => remainder * 2 > quantum,
        _ => remainder * 2 >= quantum,
    };
    if increment {
        units += 1;
    }
    units as i128 * sign
}

fn format_duration(fields: &[f64; 10]) -> String {
    format_duration_with_digits(fields, None)
}

fn format_duration_with_digits(fields: &[f64; 10], fractional_digits: Option<usize>) -> String {
    let sign = fields.iter().find(|value| **value != 0.0).map_or(1.0, |value| value.signum());
    let values = fields.map(|value| value.abs());
    let mut result = if sign < 0.0 { "-P".to_owned() } else { "P".to_owned() };
    for (index, suffix) in [(0, "Y"), (1, "M"), (2, "W"), (3, "D")] {
        if values[index] != 0.0 {
            result.push_str(&format_number(values[index]));
            result.push_str(suffix);
        }
    }
    let time = values[4..].iter().any(|value| *value != 0.0) || fractional_digits.is_some();
    if time {
        result.push('T');
        for (index, suffix) in [(4, "H"), (5, "M")] {
            if values[index] != 0.0 {
                result.push_str(&format_number(values[index]));
                result.push_str(suffix);
            }
        }
        let nanos = fields[6..]
            .iter()
            .zip([1_000_000_000_i128, 1_000_000, 1_000, 1])
            .map(|(value, scale)| (*value as i128).abs() * scale)
            .sum::<i128>();
        if nanos != 0 || values[6] != 0.0 || fractional_digits.is_some() {
            let seconds = nanos / 1_000_000_000;
            let fraction = nanos % 1_000_000_000;
            result.push_str(&seconds.to_string());
            if fraction != 0 || fractional_digits.is_some_and(|digits| digits > 0) {
                let fraction = format!("{fraction:09}");
                let fraction = if let Some(digits) = fractional_digits {
                    fraction[..digits].to_owned()
                } else {
                    fraction.trim_end_matches('0').to_owned()
                };
                result.push('.');
                result.push_str(&fraction);
            }
            result.push('S');
        }
    }
    if result == "P" || result == "-P" { format!("{result}T0S") } else { result }
}

fn round_duration_for_string(
    fields: &[f64; 10],
    digits: Option<usize>,
    rounding_mode: &str,
) -> [f64; 10] {
    let Some(digits) = digits else {
        return *fields;
    };
    let total = fields[4..]
        .iter()
        .zip(DURATION_TIME_NANOSECOND_SCALES[1..].iter().copied())
        .map(|(value, scale)| *value as i128 * scale)
        .sum::<i128>();
    let quantum = 10_i128.pow((9 - digits) as u32);
    let rounded = round_duration_integer(total, quantum, rounding_mode) * quantum;
    let original_top = (4..=9).find(|index| fields[*index] != 0.0).unwrap_or(6);
    let requested_top = match digits {
        0 => 6,
        1..=3 => 7,
        4..=6 => 8,
        _ => 9,
    };
    let top = if fields[3] != 0.0 {
        3
    } else {
        original_top.min(requested_top)
    };
    let mut result = *fields;
    result[4..].fill(0.0);
    let sign = rounded.signum();
    let mut remainder = rounded.unsigned_abs();
    if top == 3 {
        let day_scale = DURATION_TIME_NANOSECOND_SCALES[0] as u128;
        result[3] += sign as f64 * (remainder / day_scale) as f64;
        remainder %= day_scale;
    }
    for index in top.max(4)..10 {
        let scale = DURATION_TIME_NANOSECOND_SCALES[index - 3] as u128;
        result[index] = sign as f64 * (remainder / scale) as f64;
        remainder %= scale;
    }
    result.iter_mut().for_each(|value| {
        if *value == 0.0 {
            *value = 0.0;
        }
    });
    result
}

fn format_number(number: f64) -> String {
    if number.fract() == 0.0 { format!("{number:.0}") } else { number.to_string() }
}

fn parse_duration_string(text: &str) -> Result<[f64; 10], ()> {
    let (negative, text) = match text.strip_prefix('-') {
        Some(text) => (true, text),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let body = text
        .strip_prefix('P')
        .or_else(|| text.strip_prefix('p'))
        .ok_or(())?;
    let (date, time) = body.split_once(['T', 't']).unwrap_or((body, ""));
    let mut fields = [0.0; 10];
    let date_seen = parse_duration_section(date, false, &mut fields)?;
    let time_seen = parse_duration_section(time, true, &mut fields)?;
    if !date_seen && !time_seen {
        return Err(());
    }
    if negative {
        fields.iter_mut().for_each(|value| {
            *value = if *value == 0.0 { 0.0 } else { -*value };
        });
    }
    Ok(fields)
}

fn parse_duration_section(section: &str, time: bool, fields: &mut [f64; 10]) -> Result<bool, ()> {
    let mut rest = section;
    let mut seen = false;
    while !rest.is_empty() {
        let end = rest
            .char_indices()
            .find_map(|(index, character)| character.is_ascii_alphabetic().then_some(index))
            .ok_or(())?;
        let (number, suffix) = rest.split_at(end);
        let unit = suffix.chars().next().ok_or(())?;
        validate_duration_number(number, suffix, time, unit)?;
        let (whole, fraction) = parse_duration_number(number)?;
        let index = match (time, unit.to_ascii_uppercase()) {
            (false, 'Y') => 0,
            (false, 'M') => 1,
            (false, 'W') => 2,
            (false, 'D') => 3,
            (true, 'H') => 4,
            (true, 'M') => 5,
            (true, 'S') => 6,
            _ => return Err(()),
        };
        fields[index] += whole;
        if time && fraction != 0.0 {
            add_fractional_time(fields, index, fraction);
        }
        seen = true;
        rest = &suffix[unit.len_utf8()..];
    }
    Ok(seen)
}

fn validate_duration_number(number: &str, suffix: &str, time: bool, unit: char) -> Result<(), ()> {
    let separators = number.matches(['.', ',']).count();
    let digits = number.split(['.', ',']).collect::<Vec<_>>();
    if number.is_empty()
        || !number
            .chars()
            .all(|character| character.is_ascii_digit() || matches!(character, '.' | ','))
        || separators > 1
        || digits.first().is_some_and(|part| part.is_empty())
        || digits.get(1).is_some_and(|part| part.is_empty())
        || separators != 0 && (!time || !suffix[unit.len_utf8()..].is_empty())
        || unit.eq_ignore_ascii_case(&'S') && digits.get(1).is_some_and(|part| part.len() > 9)
    {
        return Err(());
    }
    Ok(())
}

fn parse_duration_number(number: &str) -> Result<(f64, f64), ()> {
    let (whole, fraction) = number.split_once(['.', ',']).unwrap_or((number, ""));
    let whole = whole.parse::<f64>().map_err(|_| ())?;
    let fraction = if fraction.is_empty() {
        0.0
    } else {
        let scale = 10_f64.powi(fraction.len() as i32);
        fraction.parse::<f64>().map_err(|_| ())? / scale
    };
    Ok((whole, fraction))
}

fn add_fractional_time(fields: &mut [f64; 10], index: usize, fraction: f64) {
    const TIME_UNIT_NANOSECOND_SCALES: [f64; 3] = [3_600.0, 60.0, 1.0];
    const NANOS_PER_SECOND: f64 = 1_000_000_000.0;
    let mut nanos = (fraction * TIME_UNIT_NANOSECOND_SCALES[index - 4] * NANOS_PER_SECOND).round() as i64;
    if index == 4 {
        fields[5] += (nanos / 60_000_000_000) as f64;
        nanos %= 60_000_000_000;
    }
    fields[6] += (nanos / 1_000_000_000) as f64;
    nanos %= 1_000_000_000;
    fields[7] += (nanos / 1_000_000) as f64;
    fields[8] += (nanos / 1_000 % 1_000) as f64;
    fields[9] += (nanos % 1_000) as f64;
}
