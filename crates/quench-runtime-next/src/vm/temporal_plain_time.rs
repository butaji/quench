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
        self.set_builtin_named(p, constructor, "from", Native::TemporalPlainTimeFrom)?;
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
        args: &[Value],
    ) -> Result<Value, JsError> {
        if native == Native::TemporalPlainTime {
            return Err(self.type_error(p, "Temporal.PlainTime requires new".into()));
        }
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        if !self.is_string(value) {
            return Err(self.type_error(p, "Invalid PlainTime".into()));
        }
        let time = super::temporal_plain_date_time_conversion::parse_time_string(self, p, value)?;
        self.temporal_plain_time_object(time)
    }

    fn temporal_plain_time_object(&mut self, time: [i32; 6]) -> Result<Value, JsError> {
        let object = self.object();
        for (name, value) in PLAIN_TIME_FIELDS.into_iter().zip(time) {
            let atom = self.intern_atom(name);
            self.set_property(object, atom, Value::number(f64::from(value)))?;
        }
        Ok(object)
    }
}
