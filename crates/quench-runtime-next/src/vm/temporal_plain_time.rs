use super::*;

const PLAIN_TIME_FIELDS: [&str; 6] = [
    "hour",
    "minute",
    "second",
    "millisecond",
    "microsecond",
    "nanosecond",
];

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
        ] {
            self.set_builtin_named(p, prototype, name, native)?;
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
        if matches!(
            native,
            Native::TemporalPlainTimeAdd
                | Native::TemporalPlainTimeSubtract
                | Native::TemporalPlainTimeEquals
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
            let time = super::temporal_plain_date_time_conversion::to_time(self, p, this)?;
            let other = super::temporal_plain_date_time_conversion::to_time(
                self,
                p,
                args.first().copied().unwrap_or(Value::UNDEFINED),
            )?;
            return Ok(if time == other { Value::TRUE } else { Value::FALSE });
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

    fn temporal_plain_time_object(&mut self, time: [i32; 6]) -> Result<Value, JsError> {
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
}
