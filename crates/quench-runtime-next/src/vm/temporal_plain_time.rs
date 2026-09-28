use super::*;

const PLAIN_TIME_FIELDS: [&str; 6] = [
    "hour",
    "minute",
    "second",
    "millisecond",
    "microsecond",
    "nanosecond",
];
const DEFAULT_DIFFERENCE_LARGEST_UNIT: &str = "hour";
const DEFAULT_DIFFERENCE_SMALLEST_UNIT: &str = "nanosecond";
const DEFAULT_DIFFERENCE_ROUNDING_MODE: &str = "trunc";
const PLAIN_TIME_FRACTIONAL_SECOND_DIGITS: usize = 9;
const PLAIN_TIME_TIME_COMPONENT_WIDTH: usize = 2;
const PLAIN_TIME_MINIMUM: i32 = 0;
const PLAIN_TIME_HOUR_LIMIT: i32 = 23;
const PLAIN_TIME_MINUTE_SECOND_LIMIT: i32 = 59;
const PLAIN_TIME_SUBSECOND_LIMIT: i32 = 999;
const PLAIN_TIME_WITH_FIELDS: [(&str, i32); 6] = [
    ("hour", PLAIN_TIME_HOUR_LIMIT),
    ("microsecond", PLAIN_TIME_SUBSECOND_LIMIT),
    ("millisecond", PLAIN_TIME_SUBSECOND_LIMIT),
    ("minute", PLAIN_TIME_MINUTE_SECOND_LIMIT),
    ("nanosecond", PLAIN_TIME_SUBSECOND_LIMIT),
    ("second", PLAIN_TIME_MINUTE_SECOND_LIMIT),
];
const PLAIN_TIME_SMALLEST_UNITS: [&str; 5] = [
    "minute",
    "second",
    "millisecond",
    "microsecond",
    "nanosecond",
];
const PLAIN_TIME_ROUNDING_MODES: [&str; 9] = [
    "ceil",
    "floor",
    "expand",
    "trunc",
    "halfCeil",
    "halfFloor",
    "halfExpand",
    "halfTrunc",
    "halfEven",
];

struct PlainTimeDifferenceOptions {
    largest_unit: String,
    smallest_unit: String,
    increment: f64,
    rounding_mode: String,
}

impl<H: Host> Vm<H> {
    pub(super) fn install_temporal_plain_time(
        &mut self,
        p: &ResidualProgram,
        temporal: Value,
    ) -> Result<(), JsError> {
        let constructor =
            self.native_with_realm(Native::TemporalPlainTime, temporal, self.realm.globals);
        self.set_builtin_function_name(constructor, "PlainTime")?;
        let prototype = self.object();
        self.temporal_plain_time_proto = prototype;
        self.set_builtin_value_named(constructor, "prototype", prototype)?;
        self.set_builtin_named(p, constructor, "compare", Native::TemporalPlainTimeCompare)?;
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
        self.set_builtin_named(p, constructor, "from", Native::TemporalPlainTimeFrom)?;
        for (name, native) in [
            ("add", Native::TemporalPlainTimeAdd),
            ("subtract", Native::TemporalPlainTimeSubtract),
            ("equals", Native::TemporalPlainTimeEquals),
            ("round", Native::TemporalPlainTimeRound),
            ("until", Native::TemporalPlainTimeUntil),
            ("since", Native::TemporalPlainTimeSince),
            ("toString", Native::TemporalPlainTimeToString),
            ("toLocaleString", Native::TemporalToLocaleString),
            ("toJSON", Native::TemporalPlainTimeToJSON),
            ("with", Native::TemporalPlainTimeWith),
        ] {
            self.set_builtin_named(p, prototype, name, native)?;
        }
        for (name, native) in [
            ("hour", Native::TemporalPlainTimeHourGetter),
            ("minute", Native::TemporalPlainTimeMinuteGetter),
            ("second", Native::TemporalPlainTimeSecondGetter),
            ("millisecond", Native::TemporalPlainTimeMillisecondGetter),
            ("microsecond", Native::TemporalPlainTimeMicrosecondGetter),
            ("nanosecond", Native::TemporalPlainTimeNanosecondGetter),
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
        self.set_builtin_named(p, prototype, "valueOf", Native::TemporalPlainTimeValueOf)?;
        if let Some(symbol) = self.well_known_symbols.get("toStringTag").copied() {
            let value = self.heap.alloc(Cell::String("Temporal.PlainTime".into()));
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
        self.set_builtin_value_named(temporal, "PlainTime", constructor)
    }

    pub(super) fn temporal_plain_time_construct(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let mut time = [0; 6];
        for (index, part) in time.iter_mut().enumerate() {
            let value = args.get(index).copied().unwrap_or(Value::UNDEFINED);
            if !value.is_undefined() {
                *part = self.plain_date_integer(p, value)?;
            }
        }
        self.validate_plain_date_time_time(p, &time)?;
        self.temporal_plain_time_object(time)
    }

    pub(super) fn temporal_plain_time_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if native == Native::TemporalPlainTimeValueOf {
            return Err(self.type_error(p, "Cannot convert PlainTime to a number".into()));
        }
        if matches!(
            native,
            Native::TemporalPlainTimeAdd
                | Native::TemporalPlainTimeSubtract
                | Native::TemporalPlainTimeEquals
                | Native::TemporalPlainTimeRound
                | Native::TemporalPlainTimeUntil
                | Native::TemporalPlainTimeSince
                | Native::TemporalPlainTimeToString
                | Native::TemporalPlainTimeToJSON
                | Native::TemporalPlainTimeWith
                | Native::TemporalPlainTimeHourGetter
                | Native::TemporalPlainTimeMinuteGetter
                | Native::TemporalPlainTimeSecondGetter
                | Native::TemporalPlainTimeMillisecondGetter
                | Native::TemporalPlainTimeMicrosecondGetter
                | Native::TemporalPlainTimeNanosecondGetter
        ) && !self.temporal_plain_time_has_brand(this)
        {
            return Err(self.type_error(p, "Not a PlainTime".into()));
        }
        if native == Native::TemporalPlainTimeCompare {
            let values = [
                args.first().copied().unwrap_or(Value::UNDEFINED),
                args.get(1).copied().unwrap_or(Value::UNDEFINED),
            ];
            if values.iter().any(|value| value.is_undefined()) {
                return Err(self.type_error(p, "Invalid PlainTime".into()));
            }
            let left =
                super::temporal_plain_date_time_conversion::to_time(self, p, values[0])?;
            let right =
                super::temporal_plain_date_time_conversion::to_time(self, p, values[1])?;
            let ordering = left.cmp(&right);
            return Ok(Value::number(match ordering {
                std::cmp::Ordering::Less => -1.0,
                std::cmp::Ordering::Equal => 0.0,
                std::cmp::Ordering::Greater => 1.0,
            }));
        }
        if native == Native::TemporalPlainTime {
            return Err(self.type_error(p, "Temporal.PlainTime requires new".into()));
        }
        let getter_name = match native {
            Native::TemporalPlainTimeHourGetter => Some("hour"),
            Native::TemporalPlainTimeMinuteGetter => Some("minute"),
            Native::TemporalPlainTimeSecondGetter => Some("second"),
            Native::TemporalPlainTimeMillisecondGetter => Some("millisecond"),
            Native::TemporalPlainTimeMicrosecondGetter => Some("microsecond"),
            Native::TemporalPlainTimeNanosecondGetter => Some("nanosecond"),
            _ => None,
        };
        if let Some(name) = getter_name {
            let time = super::temporal_plain_date_time_conversion::to_time(self, p, this)?;
            let index = PLAIN_TIME_FIELDS
                .iter()
                .position(|field| *field == name)
                .expect("PlainTime getter is a time field");
            return Ok(Value::number(f64::from(time[index])));
        }
        if matches!(native, Native::TemporalPlainTimeAdd | Native::TemporalPlainTimeSubtract) {
            let duration = self.duration_record(
                p,
                args.first().copied().unwrap_or(Value::UNDEFINED),
            )?;
            self.validate_duration_fields(p, &duration)?;
            let direction = if native == Native::TemporalPlainTimeSubtract {
                -1_i128
            } else {
                1_i128
            };
            let duration_nanoseconds =
                duration[super::temporal_date_arithmetic::DURATION_DAYS_FIELD] as i128
                    * super::temporal_date_arithmetic::NANOS_PER_DAY
                    + duration[super::temporal_date_arithmetic::DURATION_HOURS_FIELD..]
                    .iter()
                    .zip(super::temporal_date_arithmetic::TIME_UNIT_NANOSECOND_SCALES)
                    .map(|(value, scale)| *value as i128 * scale)
                    .sum::<i128>();
            let time = super::temporal_plain_date_time_conversion::to_time(self, p, this)?;
            let time_nanoseconds = time
                .iter()
                .zip(super::temporal_date_arithmetic::TIME_UNIT_NANOSECOND_SCALES)
                .map(|(value, scale)| i128::from(*value) * scale)
                .sum::<i128>();
            let total = (time_nanoseconds + duration_nanoseconds * direction)
                .rem_euclid(super::temporal_date_arithmetic::NANOS_PER_DAY);
            let mut time = [0_i32; 6];
            let mut remainder = total;
            for (index, scale) in super::temporal_date_arithmetic::TIME_UNIT_NANOSECOND_SCALES
                .into_iter()
                .enumerate()
            {
                time[index] = (remainder / scale) as i32;
                remainder %= scale;
            }
            return self.temporal_plain_time_object(time);
        }
        if native == Native::TemporalPlainTimeEquals {
            let value = args.first().copied().unwrap_or(Value::UNDEFINED);
            if value.is_undefined() {
                return Err(self.type_error(p, "Invalid PlainTime".into()));
            }
            let time = super::temporal_plain_date_time_conversion::to_time(self, p, this)?;
            let other = super::temporal_plain_date_time_conversion::to_time(self, p, value)?;
            return Ok(if time == other { Value::TRUE } else { Value::FALSE });
        }
        if matches!(native, Native::TemporalPlainTimeUntil | Native::TemporalPlainTimeSince) {
            return self.temporal_plain_time_difference(p, native, this, args);
        }
        if matches!(native, Native::TemporalPlainTimeToString | Native::TemporalPlainTimeToJSON) {
            let options = if native == Native::TemporalPlainTimeToJSON {
                Value::UNDEFINED
            } else {
                args.first().copied().unwrap_or(Value::UNDEFINED)
            };
            return self.temporal_plain_time_to_string(p, this, options);
        }
        if native == Native::TemporalPlainTimeWith {
            return self.temporal_plain_time_with(p, this, args);
        }
        if native == Native::TemporalPlainTimeRound {
            let options = args.first().copied().unwrap_or(Value::UNDEFINED);
            let parsed = super::temporal_instant_round::read_options(self, p, options)?;
            let (_, scale) = super::temporal_instant_round::parse_unit(
                self,
                p,
                parsed.smallest_unit.as_deref(),
                super::temporal_instant_round::RoundingDomain::PlainTime,
            )?;
            let increment = super::temporal_instant_round::validate_increment(
                self,
                p,
                parsed.increment,
                scale,
                super::temporal_instant_round::RoundingDomain::PlainTime,
            )?;
            let mode = super::temporal_instant_round::validate_mode(
                self,
                p,
                parsed.rounding_mode.as_deref(),
            )?;
            let time = super::temporal_plain_date_time_conversion::to_time(self, p, this)?;
            let total = time
                .iter()
                .zip(super::temporal_date_arithmetic::TIME_UNIT_NANOSECOND_SCALES)
                .map(|(value, scale)| i128::from(*value) * scale)
                .sum::<i128>();
            let quantum = scale * increment;
            let rounded = (super::temporal_zoned_date_time::round_temporal_nanoseconds(
                total, quantum, mode,
            ) * quantum)
                .rem_euclid(super::temporal_date_arithmetic::NANOS_PER_DAY);
            let mut time = [0_i32; 6];
            let mut remainder = rounded;
            for (index, scale) in super::temporal_date_arithmetic::TIME_UNIT_NANOSECOND_SCALES
                .into_iter()
                .enumerate()
            {
                time[index] = (remainder / scale) as i32;
                remainder %= scale;
            }
            return self.temporal_plain_time_object(time);
        }
        if native == Native::TemporalPlainTimeFrom {
            let value = args.first().copied().unwrap_or(Value::UNDEFINED);
            if value.is_undefined() {
                return Err(self.type_error(p, "Invalid time".into()));
            }
            let options = args.get(1).copied().unwrap_or(Value::UNDEFINED);
            let time = super::temporal_plain_date_time_conversion::to_time_with_options(
                self, p, value, options,
            )?;
            return self.temporal_plain_time_object(time);
        }
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        if !self.is_string(value) {
            return Err(self.type_error(p, "Invalid PlainTime".into()));
        }
        let time = super::temporal_plain_date_time_conversion::parse_time_string(self, p, value)?;
        self.temporal_plain_time_object(time)
    }

    pub(super) fn temporal_plain_time_object(&mut self, time: [i32; 6]) -> Result<Value, JsError> {
        let object = self.heap.alloc(Cell::Object(Self::empty_object(
            self.temporal_plain_time_proto,
        )));
        for (name, value) in PLAIN_TIME_FIELDS.into_iter().zip(time) {
            let atom = self.intern_atom(name);
            self.set_property(object, atom, Value::number(f64::from(value)))?;
        }
        Ok(object)
    }

    fn temporal_plain_time_has_brand(&self, value: Value) -> bool {
        let Some(object) = self.object_data(value) else {
            return false;
        };
        let mut prototype = object.proto;
        while !prototype.is_null() {
            if prototype == self.temporal_plain_time_proto {
                return true;
            }
            let Some(object) = self.object_data(prototype) else {
                return false;
            };
            prototype = object.proto;
        }
        false
    }

    fn temporal_plain_time_difference(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let other = args.first().copied().unwrap_or(Value::UNDEFINED);
        if other.is_undefined() {
            return Err(self.type_error(p, "Invalid PlainTime".into()));
        }
        let receiver_time = super::temporal_plain_date_time_conversion::to_time(self, p, this)?;
        let other_time = super::temporal_plain_date_time_conversion::to_time(self, p, other)?;
        let receiver_nanos = receiver_time
            .iter()
            .zip(super::temporal_date_arithmetic::TIME_UNIT_NANOSECOND_SCALES)
            .map(|(value, scale)| i128::from(*value) * scale)
            .sum::<i128>();
        let other_nanos = other_time
            .iter()
            .zip(super::temporal_date_arithmetic::TIME_UNIT_NANOSECOND_SCALES)
            .map(|(value, scale)| i128::from(*value) * scale)
            .sum::<i128>();
        let direction = if native == Native::TemporalPlainTimeSince {
            -1_i128
        } else {
            1_i128
        };
        let delta = (other_nanos - receiver_nanos) * direction;
        let options = self.temporal_plain_time_difference_options(
            p,
            args.get(1).copied().unwrap_or(Value::UNDEFINED),
        )?;
        let (largest_unit, largest_scale) = super::temporal_instant_round::parse_unit(
            self,
            p,
            Some(&options.largest_unit),
            super::temporal_instant_round::RoundingDomain::PlainTime,
        )?;
        let (smallest_unit, smallest_scale) = super::temporal_instant_round::parse_unit(
            self,
            p,
            Some(&options.smallest_unit),
            super::temporal_instant_round::RoundingDomain::PlainTime,
        )?;
        if largest_unit == "day" || smallest_unit == "day" || largest_scale < smallest_scale {
            return Err(self.range_error(p, "Invalid time unit relationship".into()));
        }
        let increment = super::temporal_instant_round::validate_increment(
            self,
            p,
            Some(options.increment),
            smallest_scale,
            super::temporal_instant_round::RoundingDomain::PlainTime,
        )?;
        let rounding_mode =
            super::temporal_instant_round::validate_mode(self, p, Some(&options.rounding_mode))?;
        let quantum = smallest_scale * increment;
        let rounded = super::temporal_zoned_date_time::round_temporal_nanoseconds(
            delta,
            quantum,
            rounding_mode,
        ) * quantum;
        let units = super::temporal_date_arithmetic::TIME_UNIT_NANOSECOND_SCALES;
        let largest_index = units
            .iter()
            .position(|scale| *scale == largest_scale)
            .ok_or_else(|| self.range_error(p, "Invalid largestUnit".into()))?;
        let smallest_index = units
            .iter()
            .position(|scale| *scale == smallest_scale)
            .ok_or_else(|| self.range_error(p, "Invalid smallestUnit".into()))?;
        let mut duration = [0.0; 10];
        let mut remainder = rounded;
        for (index, scale) in units
            .into_iter()
            .enumerate()
            .skip(largest_index)
            .take(smallest_index - largest_index + 1)
        {
            duration[super::temporal_date_arithmetic::DURATION_HOURS_FIELD + index] =
                (remainder / scale) as f64;
            remainder %= scale;
        }
        self.make_temporal_duration(p, duration)
    }

    fn temporal_plain_time_difference_options(
        &mut self,
        p: &ResidualProgram,
        options: Value,
    ) -> Result<PlainTimeDifferenceOptions, JsError> {
        let mut largest_unit = DEFAULT_DIFFERENCE_LARGEST_UNIT.to_owned();
        let mut smallest_unit = DEFAULT_DIFFERENCE_SMALLEST_UNIT.to_owned();
        let mut increment = 1.0;
        let mut rounding_mode = DEFAULT_DIFFERENCE_ROUNDING_MODE.to_owned();
        if !options.is_undefined() {
            if !self.is_object_like(options) {
                return Err(self.type_error(p, "Invalid options".into()));
            }
            let largest_atom = self.intern_atom("largestUnit");
            let largest_value = self.get_property(p, options, largest_atom)?;
            if !largest_value.is_undefined() {
                largest_unit = self.to_string(p, largest_value)?.to_string();
            }
            let increment_atom = self.intern_atom("roundingIncrement");
            let increment_value = self.get_property(p, options, increment_atom)?;
            if !increment_value.is_undefined() {
                increment = self.to_number(p, increment_value)?;
            }
            let rounding_atom = self.intern_atom("roundingMode");
            let rounding_value = self.get_property(p, options, rounding_atom)?;
            if !rounding_value.is_undefined() {
                rounding_mode = self.to_string(p, rounding_value)?.to_string();
            }
            let smallest_atom = self.intern_atom("smallestUnit");
            let smallest_value = self.get_property(p, options, smallest_atom)?;
            if !smallest_value.is_undefined() {
                smallest_unit = self.to_string(p, smallest_value)?.to_string();
            }
        }
        if largest_unit == "auto" {
            largest_unit = DEFAULT_DIFFERENCE_LARGEST_UNIT.to_owned();
        }
        largest_unit = largest_unit
            .strip_suffix('s')
            .unwrap_or(&largest_unit)
            .to_owned();
        smallest_unit = smallest_unit
            .strip_suffix('s')
            .unwrap_or(&smallest_unit)
            .to_owned();
        Ok(PlainTimeDifferenceOptions {
            largest_unit,
            smallest_unit,
            increment,
            rounding_mode,
        })
    }

    fn temporal_plain_time_to_string(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        options: Value,
    ) -> Result<Value, JsError> {
        let mut fractional_digits = None;
        let mut smallest_unit = None;
        let mut rounding_mode = "trunc".to_owned();
        if !options.is_undefined() {
            if !self.is_object_like(options) {
                return Err(self.type_error(p, "Invalid string options".into()));
            }
            let fractional_atom = self.intern_atom("fractionalSecondDigits");
            let fractional_value = self.get_property(p, options, fractional_atom)?;
            if !fractional_value.is_undefined() {
                if fractional_value.as_number().is_some() {
                    let number = self.to_number(p, fractional_value)?.floor();
                    if !(0.0..=PLAIN_TIME_FRACTIONAL_SECOND_DIGITS as f64).contains(&number) {
                        return Err(self.range_error(p, "Invalid fractionalSecondDigits".into()));
                    }
                    fractional_digits = Some(number as usize);
                } else {
                    let text = self.to_string(p, fractional_value)?.to_string();
                    if text != "auto" {
                        return Err(self.range_error(p, "Invalid fractionalSecondDigits".into()));
                    }
                }
            }
            let rounding_atom = self.intern_atom("roundingMode");
            let rounding_value = self.get_property(p, options, rounding_atom)?;
            if !rounding_value.is_undefined() {
                rounding_mode = self.to_string(p, rounding_value)?.to_string();
            }
            let smallest_atom = self.intern_atom("smallestUnit");
            let smallest_value = self.get_property(p, options, smallest_atom)?;
            if !smallest_value.is_undefined() {
                let unit = self.to_string(p, smallest_value)?.to_string();
                let unit = unit.strip_suffix('s').unwrap_or(&unit);
                if !PLAIN_TIME_SMALLEST_UNITS.contains(&unit) {
                    return Err(self.range_error(p, "Invalid smallestUnit".into()));
                }
                smallest_unit = Some(unit.to_owned());
            }
        }
        if !PLAIN_TIME_ROUNDING_MODES.contains(&rounding_mode.as_str()) {
            return Err(self.range_error(p, "Invalid roundingMode".into()));
        }
        let time = super::temporal_plain_date_time_conversion::to_time(self, p, this)?;
        let mut quantum = 1_i128;
        let mut precision = None;
        let mut omit_seconds = false;
        if let Some(unit) = smallest_unit.as_deref() {
            let (index, digits) = match unit {
                "minute" => (1, 0),
                "second" => (2, 0),
                "millisecond" => (3, 3),
                "microsecond" => (4, 6),
                _ => (5, PLAIN_TIME_FRACTIONAL_SECOND_DIGITS),
            };
            quantum = super::temporal_date_arithmetic::TIME_UNIT_NANOSECOND_SCALES[index];
            precision = Some(digits);
            omit_seconds = unit == "minute";
        } else if let Some(digits) = fractional_digits {
            quantum = 10_i128.pow((PLAIN_TIME_FRACTIONAL_SECOND_DIGITS - digits) as u32);
            precision = Some(digits);
        }
        let total = time
            .iter()
            .zip(super::temporal_date_arithmetic::TIME_UNIT_NANOSECOND_SCALES)
            .map(|(value, scale)| i128::from(*value) * scale)
            .sum::<i128>();
        let rounded = (super::temporal_zoned_date_time::round_temporal_nanoseconds(
            total,
            quantum,
            &rounding_mode,
        ) * quantum)
            .rem_euclid(super::temporal_date_arithmetic::NANOS_PER_DAY);
        let hour = rounded / super::temporal_date_arithmetic::TIME_UNIT_NANOSECOND_SCALES[0];
        let minute = rounded / super::temporal_date_arithmetic::TIME_UNIT_NANOSECOND_SCALES[1]
            % 60;
        let second = rounded / super::temporal_date_arithmetic::TIME_UNIT_NANOSECOND_SCALES[2]
            % 60;
        let fractional = rounded
            % super::temporal_date_arithmetic::TIME_UNIT_NANOSECOND_SCALES[2];
        let time = if omit_seconds {
            format!(
                "{hour:0width$}:{minute:0width$}",
                width = PLAIN_TIME_TIME_COMPONENT_WIDTH,
            )
        } else {
            format!(
                "{hour:0width$}:{minute:0width$}:{second:0width$}",
                width = PLAIN_TIME_TIME_COMPONENT_WIDTH,
            )
        };
        let fraction = if omit_seconds {
            String::new()
        } else if let Some(digits) = precision {
            if digits == 0 {
                String::new()
            } else {
                let digits_text = format!(
                    "{fractional:0width$}",
                    width = PLAIN_TIME_FRACTIONAL_SECOND_DIGITS,
                );
                format!(".{}", &digits_text[..digits])
            }
        } else if fractional == 0 {
            String::new()
        } else {
            let digits_text = format!(
                "{fractional:0width$}",
                width = PLAIN_TIME_FRACTIONAL_SECOND_DIGITS,
            );
            let trimmed = digits_text.trim_end_matches('0');
            if trimmed.is_empty() {
                String::new()
            } else {
                format!(".{trimmed}")
            }
        };
        Ok(self.heap.alloc(Cell::String(format!("{time}{fraction}").into())))
    }

    fn temporal_plain_time_with(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let fields = args.first().copied().unwrap_or(Value::UNDEFINED);
        if !self.is_object_like(fields)
            || matches!(self.heap.get(fields), Some(Cell::Array { .. }))
            || self.is_temporal_object(fields)
        {
            return Err(self.type_error(p, "Invalid time-like object".into()));
        }
        for name in ["calendar", "timeZone"] {
            let key = self.intern_atom(name);
            if !self.get_property(p, fields, key)?.is_undefined() {
                return Err(self.type_error(p, format!("Invalid {name}")));
            }
        }
        let current = PLAIN_TIME_WITH_FIELDS
            .iter()
            .map(|(name, _)| {
                let key = self.intern_atom(name);
                self.get_property(p, this, key)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let replacements = PLAIN_TIME_WITH_FIELDS
            .iter()
            .map(|(name, _)| {
                let key = self.intern_atom(name);
                let value = self.get_property(p, fields, key)?;
                if value.is_undefined() {
                    Ok(Value::UNDEFINED)
                } else {
                    let number = self.to_number(p, value)?;
                    if !number.is_finite() {
                        return Err(self.range_error(p, "Invalid time".into()));
                    }
                    Ok(Value::number(number))
                }
            })
            .collect::<Result<Vec<_>, JsError>>()?;
        if replacements.iter().all(|value| value.is_undefined()) {
            return Err(self.type_error(p, "No time fields".into()));
        }
        let options = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let constrain_overflow = self.plain_date_overflow(p, options)?;
        let mut time = [0_i32; 6];
        for ((old, replacement), (name, maximum)) in current
            .iter()
            .zip(replacements.iter())
            .zip(PLAIN_TIME_WITH_FIELDS)
        {
            let value = if replacement.is_undefined() { *old } else { *replacement };
            let integer = self.plain_date_integer(p, value)?;
            let source_index = PLAIN_TIME_FIELDS
                .iter()
                .position(|field| field == &name)
                .expect("PlainTime with fields map to canonical field slots");
            if !constrain_overflow {
                if !(PLAIN_TIME_MINIMUM..=maximum).contains(&integer) {
                    return Err(self.range_error(p, "Invalid time".into()));
                }
                time[source_index] = integer;
            } else {
                time[source_index] = integer.clamp(PLAIN_TIME_MINIMUM, maximum);
            }
        }
        self.temporal_plain_time_object(time)
    }

    fn is_temporal_object(&self, value: Value) -> bool {
        self.temporal_plain_time_has_brand(value)
            || matches!(
                self.heap.get(value),
                Some(
                    Cell::TemporalPlainDate { .. }
                        | Cell::TemporalPlainDateTime { .. }
                        | Cell::TemporalPlainMonthDay { .. }
                        | Cell::TemporalPlainYearMonth { .. }
                        | Cell::TemporalZonedDateTime { .. }
                )
            )
    }
}
