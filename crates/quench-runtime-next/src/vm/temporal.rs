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
            let number = self.to_number(p, value)?;
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
        let time = fields[3..].iter().fold(0.0, |sum, value| sum + value.abs());
        if !time.is_finite() || time > 9_007_199_254_740_991.0 {
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
            Native::TemporalDurationCompare => {
                let left = self.duration_record(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let right = self.duration_record(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
                self.validate_duration_fields(p, &left)?;
                self.validate_duration_fields(p, &right)?;
                if left[..3].iter().chain(right[..3].iter()).any(|value| *value != 0.0) {
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
        let read_order = [3, 4, 8, 7, 5, 1, 9, 6, 2, 0];
        for index in read_order {
            let atom = self.intern_atom(DURATION_FIELDS[index]);
            let value = self.get_property(p, value, atom)?;
            if value.is_undefined() {
                continue;
            }
            let number = self.to_number(p, value)?;
            if !number.is_finite() || number.fract() != 0.0 {
                return Err(self.range_error(p, "Duration fields must be integral".into()));
            }
            fields[index] = number;
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
        let scales = [
            86_400_000_000_000_i128,
            3_600_000_000_000,
            60_000_000_000,
            1_000_000_000,
            1_000_000,
            1_000,
            1,
        ];
        fields[3..]
            .iter()
            .zip(scales)
            .map(|(value, scale)| *value as i128 * scale)
            .sum()
    }
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
        let nanos = values[6] as u64 * 1_000_000_000
            + values[7] as u64 * 1_000_000
            + values[8] as u64 * 1_000
            + values[9] as u64;
        if nanos != 0 || values[6] != 0.0 {
            let seconds = nanos / 1_000_000_000;
            let fraction = nanos % 1_000_000_000;
            result.push_str(&format_number(seconds as f64));
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
    let body = text.strip_prefix('P').ok_or(())?;
    let (date, time) = body.split_once('T').unwrap_or((body, ""));
    let mut fields = [0.0; 10];
    let mut parse = |section: &str, is_time: bool| -> Result<(), ()> {
        let mut rest = section;
        while !rest.is_empty() {
            let end = rest.find(|character: char| character.is_ascii_alphabetic()).ok_or(())?;
            let (number, suffix) = rest.split_at(end);
            let unit = suffix.chars().next().unwrap();
            let index = match (is_time, unit) {
                (false, 'Y') => 0, (false, 'M') => 1, (false, 'W') => 2, (false, 'D') => 3,
                (true, 'H') => 4, (true, 'M') => 5, (true, 'S') => 6,
                _ => return Err(()),
            };
            let value = number.parse::<f64>().map_err(|_| ())?;
            if !value.is_finite() || value.fract() != 0.0 && (!is_time || unit != 'S' || !suffix.ends_with('S')) {
                return Err(());
            }
            fields[index] = value;
            rest = &suffix[unit.len_utf8()..];
        }
        Ok(())
    };
    parse(date, false)?;
    parse(time, true)?;
    if negative { fields.iter_mut().for_each(|value| *value = -*value); }
    Ok(fields)
}
