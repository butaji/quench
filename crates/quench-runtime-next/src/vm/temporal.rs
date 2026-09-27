use super::*;

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
const DURATION_TIME_NANOSECOND_SCALES: [i128; 7] = [
    86_400_000_000_000,
    3_600_000_000_000,
    60_000_000_000,
    1_000_000_000,
    1_000_000,
    1_000,
    1,
];

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
        ] {
            self.set_builtin_named(p, prototype, name, native)?;
        }
        self.set_builtin_value_named(temporal, "Duration", duration)?;
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

    fn validate_duration_fields(
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
                    return Ok(Value::number(0.0));
                }
                if left[..3].iter().chain(right[..3].iter()).any(|value| *value != 0.0)
                    && relative_to.is_undefined()
                {
                    return Err(self.range_error(p, "relativeTo is required to compare calendar units".into()));
                }
                let left = self.duration_time_nanos(&left);
                let right = self.duration_time_nanos(&right);
                Ok(Value::number(if left < right { -1.0 } else if left > right { 1.0 } else { 0.0 }))
            }
            Native::TemporalDurationToString | Native::TemporalDurationToJSON => {
                let fields = self.duration_fields(p, this)?;
                let text = format_duration(&fields);
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

    fn duration_record(&mut self, p: &ResidualProgram, value: Value) -> Result<[f64; 10], JsError> {
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

fn format_duration(fields: &[f64; 10]) -> String {
    let sign = fields.iter().find(|value| **value != 0.0).map_or(1.0, |value| value.signum());
    let values = fields.map(|value| value.abs());
    let mut result = if sign < 0.0 { "-P".to_owned() } else { "P".to_owned() };
    for (index, suffix) in [(0, "Y"), (1, "M"), (2, "W"), (3, "D")] {
        if values[index] != 0.0 {
            result.push_str(&format_number(values[index]));
            result.push_str(suffix);
        }
    }
    let time = values[4..].iter().any(|value| *value != 0.0);
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
        if nanos != 0 || values[6] != 0.0 {
            let seconds = nanos / 1_000_000_000;
            let fraction = nanos % 1_000_000_000;
            result.push_str(&seconds.to_string());
            if fraction != 0 {
                let fraction = format!("{fraction:09}").trim_end_matches('0').to_owned();
                result.push('.');
                result.push_str(&fraction);
            }
            result.push('S');
        }
    }
    if result == "P" || result == "-P" { format!("{result}T0S") } else { result }
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
