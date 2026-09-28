use super::*;
use crate::host::{CapabilityId, HostContext};

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
const DURATION_TOTAL_TIME_LIMIT_NANOS: i128 =
    9_007_199_254_740_991_i128 * 1_000_000_000 + 999_999_999;
const DURATION_TOTAL_DECIMAL_DIGITS: usize = 32;
const DURATION_ROUNDING_INCREMENT_LIMIT: f64 = 1_000_000_000.0;
const DURATION_DAYS_PER_WEEK: i128 = 7;
const DURATION_ROUNDING_MODE_NAMES: [&str; 9] = [
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
const NOW_NANOSECONDS_PER_MILLISECOND: i128 = 1_000_000;
impl<H: Host> Vm<H> {
    pub(super) fn install_temporal(&mut self, p: &ResidualProgram) -> Result<(), JsError> {
        let temporal = self.object();
        let duration =
            self.native_with_realm(Native::TemporalDuration, temporal, self.realm.globals);
        self.set_builtin_function_name(duration, "Duration")?;
        let prototype = self.object();
        self.set_builtin_value_named(duration, "prototype", prototype)?;
        self.lock_temporal_constructor_prototype(duration)?;
        self.set_builtin_value_named(prototype, "constructor", duration)?;
        self.install_temporal_to_string_tag(prototype, "Temporal.Duration")?;
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
            ("toLocaleString", Native::TemporalToLocaleString),
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
        self.install_temporal_to_string_tag(temporal, "Temporal")?;
        self.install_temporal_now(p, temporal)?;
        self.install_temporal_plain_time(p, temporal)?;
        self.install_temporal_plain_date(p, temporal)?;
        self.install_temporal_calendar_projections(p, temporal)?;
        self.install_temporal_plain_date_time(p, temporal)?;
        self.install_temporal_instant(p, temporal)?;
        self.install_temporal_zoned_date_time(p, temporal)?;
        self.set_builtin_value_named(self.realm.globals, "Temporal", temporal)
    }

    fn install_temporal_to_string_tag(
        &mut self,
        object: Value,
        tag: &str,
    ) -> Result<(), JsError> {
        let symbol = self
            .well_known_symbols
            .get("toStringTag")
            .copied()
            .ok_or_else(|| JsError("Symbol.toStringTag is not initialized".into()))?;
        let value = self.heap.alloc(Cell::String(tag.into()));
        self.set_symbol_property(object, symbol, value)?;
        self.set_property_attributes(
            object,
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
        Ok(())
    }

    fn lock_temporal_constructor_prototype(
        &mut self,
        constructor: Value,
    ) -> Result<(), JsError> {
        let prototype = self.intern_atom("prototype");
        self.set_property_attributes(
            constructor,
            PropertyKey::string(prototype),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: false,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        Ok(())
    }

    fn install_temporal_now(
        &mut self,
        p: &ResidualProgram,
        temporal: Value,
    ) -> Result<(), JsError> {
        let now = self.object();
        for (name, native) in [
            ("instant", Native::TemporalNowInstant),
            ("plainDateISO", Native::TemporalNowPlainDateISO),
            ("plainDateTimeISO", Native::TemporalNowPlainDateTimeISO),
            ("plainTimeISO", Native::TemporalNowPlainTimeISO),
            ("timeZoneId", Native::TemporalNowTimeZoneId),
            ("zonedDateTimeISO", Native::TemporalNowZonedDateTimeISO),
        ] {
            self.set_builtin_named(p, now, name, native)?;
        }
        if let Some(symbol) = self.well_known_symbols.get("toStringTag").copied() {
            let tag = self.heap.alloc(Cell::String("Temporal.Now".into()));
            self.set_symbol_property(now, symbol, tag)?;
            self.set_property_attributes(
                now,
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
        self.set_builtin_value_named(temporal, "Now", now)
    }

    pub(super) fn temporal_now_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if native == Native::TemporalNowTimeZoneId {
            return Ok(self.heap.alloc(Cell::String("UTC".into())));
        }
        let milliseconds = HostContext::new(&mut self.host).invoke(CapabilityId::ClockMillis, None);
        if !milliseconds.is_finite() {
            return Err(self.range_error(p, "Invalid current time".into()));
        }
        let epoch_nanoseconds = milliseconds.trunc() as i128 * NOW_NANOSECONDS_PER_MILLISECOND;
        if native == Native::TemporalNowInstant {
            let constructor = self.temporal_instant_constructor(p)?;
            return self.make_temporal_instant(p, epoch_nanoseconds, constructor);
        }
        let timezone = match args.first().copied().unwrap_or(Value::UNDEFINED) {
            value if value.is_undefined() => "UTC".to_owned(),
            value => self.temporal_timezone_id(p, value)?,
        };
        let fields = super::temporal_zoned_date_time::zoned_date_time_fields(
            epoch_nanoseconds,
            &timezone,
        )
        .ok_or_else(|| self.range_error(p, "Invalid current time".into()))?;
        let date = (fields[0], fields[1], fields[2]);
        let time = [fields[3], fields[4], fields[5], fields[6], fields[7], fields[8]];
        match native {
            Native::TemporalNowPlainDateISO => {
                let constructor = self.temporal_plain_date_constructor(p)?;
                let args = [
                    Value::number(f64::from(date.0)),
                    Value::number(f64::from(date.1)),
                    Value::number(f64::from(date.2)),
                ];
                self.temporal_plain_date_construct(p, &args, constructor)
            }
            Native::TemporalNowPlainDateTimeISO => {
                let constructor = self.temporal_plain_date_time_constructor(p)?;
                let args = [
                    Value::number(f64::from(date.0)),
                    Value::number(f64::from(date.1)),
                    Value::number(f64::from(date.2)),
                    Value::number(f64::from(time[0])),
                    Value::number(f64::from(time[1])),
                    Value::number(f64::from(time[2])),
                    Value::number(f64::from(time[3])),
                    Value::number(f64::from(time[4])),
                    Value::number(f64::from(time[5])),
                ];
                self.temporal_plain_date_time_construct(p, &args, constructor)
            }
            Native::TemporalNowPlainTimeISO => {
                let args = time.map(|value| Value::number(f64::from(value)));
                self.temporal_plain_time_construct(p, &args)
            }
            Native::TemporalNowZonedDateTimeISO => {
                let temporal_key = self.intern_atom("Temporal");
                let temporal = self.get_property(p, self.realm.globals, temporal_key)?;
                let constructor_key = self.intern_atom("ZonedDateTime");
                let constructor = self.get_property(p, temporal, constructor_key)?;
                let args = [
                    self.heap.alloc(Cell::BigInt(epoch_nanoseconds.to_string().into())),
                    self.heap.alloc(Cell::String(timezone.into())),
                ];
                self.temporal_zoned_date_time_construct(p, &args, constructor)
            }
            _ => Err(self.type_error(p, "Invalid Temporal.Now method".into())),
        }
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
            _ => Err(self.type_error(
                p,
                "Temporal.Duration method called on incompatible receiver".into(),
            )),
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
            Native::TemporalDuration => {
                Err(self.type_error(p, "Temporal.Duration requires new".into()))
            }
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
                    return Err(
                        self.type_error(p, "Duration.with requires a duration field".into())
                    );
                }
                self.validate_duration_fields(p, &fields)?;
                self.make_temporal_duration(p, fields)
            }
            Native::TemporalDurationAdd | Native::TemporalDurationSubtract => {
                let left = self.duration_fields(p, this)?;
                let right =
                    self.duration_record(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                self.validate_duration_fields(p, &right)?;
                if left[..3]
                    .iter()
                    .chain(right[..3].iter())
                    .any(|value| *value != 0.0)
                {
                    return Err(
                        self.range_error(p, "relativeTo required for calendar units".into())
                    );
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
                let left =
                    self.duration_record(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let right =
                    self.duration_record(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
                if !options.is_undefined() && !self.is_object_like(options) {
                    return Err(
                        self.type_error(p, "Duration.compare options must be an object".into())
                    );
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
                if left[..3]
                    .iter()
                    .chain(right[..3].iter())
                    .any(|value| *value != 0.0)
                    && relative_to.is_undefined()
                {
                    return Err(self.range_error(
                        p,
                        "relativeTo is required to compare calendar units".into(),
                    ));
                }
                let ordering = if relative_to.is_undefined() {
                    self.duration_time_nanos(&left)
                        .cmp(&self.duration_time_nanos(&right))
                } else {
                    let date = self.temporal_relative_date(p, relative_to)?;
                    let left = quench_temporal::relative_duration_nanoseconds(
                        date,
                        std::array::from_fn(|index| left[index] as i128),
                    )
                    .ok_or_else(|| self.range_error(p, "Invalid relativeTo".into()))?;
                    let right = quench_temporal::relative_duration_nanoseconds(
                        date,
                        std::array::from_fn(|index| right[index] as i128),
                    )
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
            Native::TemporalDurationValueOf => Err(self.type_error(
                p,
                "Temporal.Duration.prototype.valueOf is not allowed".into(),
            )),
            Native::TemporalDurationSignGetter
            | Native::TemporalDurationBlankGetter
            | Native::TemporalDurationYearsGetter
            | Native::TemporalDurationMonthsGetter
            | Native::TemporalDurationWeeksGetter
            | Native::TemporalDurationDaysGetter
            | Native::TemporalDurationHoursGetter
            | Native::TemporalDurationMinutesGetter
            | Native::TemporalDurationSecondsGetter
            | Native::TemporalDurationMillisecondsGetter
            | Native::TemporalDurationMicrosecondsGetter
            | Native::TemporalDurationNanosecondsGetter => {
                let fields = self.duration_fields(p, this)?;
                if native == Native::TemporalDurationSignGetter {
                    let sign = fields
                        .iter()
                        .find(|value| **value != 0.0)
                        .map_or(0.0, |value| value.signum());
                    return Ok(Value::number(sign));
                }
                if native == Native::TemporalDurationBlankGetter {
                    return Ok(if fields.iter().all(|value| *value == 0.0) {
                        Value::TRUE
                    } else {
                        Value::FALSE
                    });
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
            return quench_temporal::parse_duration(text.host_string())
                .ok_or_else(|| self.range_error(p, "Invalid duration string".into()));
        }
        if !self.is_object_like(value) {
            return Err(
                self.type_error(p, "Duration-like value must be an object or string".into())
            );
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

    pub(super) fn make_temporal_duration(
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
    ) -> Result<quench_temporal::IsoDate, JsError> {
        if let Some(Cell::String(text)) = self.heap.get(value) {
            let text = text.host_string();
            let date = if text.contains(['T', 't']) {
                super::temporal_zoned_date_time::parse_relative_date_string(&text)
            } else {
                super::temporal_date_parse::parse_plain_date_string(&text).map(|(date, _)| date)
            }
            .ok_or_else(|| self.range_error(p, "Invalid relativeTo".into()))?;
            return Ok(quench_temporal::IsoDate {
                year: date.year,
                month: date.month,
                day: date.day,
            });
        }
        if let Some(Cell::TemporalPlainDate {
            year, month, day, ..
        }) = self.heap.get(value)
        {
            return Ok(quench_temporal::IsoDate {
                year: *year,
                month: *month,
                day: *day,
            });
        }
        if !self.is_object_like(value) {
            return Err(self.type_error(p, "Invalid relativeTo".into()));
        }
        let constructor = self.temporal_plain_date_constructor(p)?;
        let date = self.temporal_plain_date_from(p, constructor, &[value])?;
        let (year, month, day, _) = self.temporal_plain_date_slots(p, date)?;
        let _ = super::temporal_plain_date_time_conversion::to_date_time(
            self,
            p,
            value,
            Value::UNDEFINED,
        )?;
        let timezone_atom = self.intern_atom("timeZone");
        let timezone = self.get_property(p, value, timezone_atom)?;
        if !timezone.is_undefined() {
            if !matches!(self.heap.get(timezone), Some(Cell::String(_))) {
                return Err(self.type_error(p, "Invalid time zone".into()));
            }
            self.temporal_timezone_id(p, timezone)?;
        }
        let offset_atom = self.intern_atom("offset");
        let offset = self.get_property(p, value, offset_atom)?;
        if !offset.is_undefined() {
            if !matches!(self.heap.get(offset), Some(Cell::String(_))) {
                return Err(self.type_error(p, "Invalid offset".into()));
            }
            let offset = self.to_string(p, offset)?;
            if !quench_temporal::valid_string_offset(&offset) {
                return Err(self.range_error(p, "Invalid offset".into()));
            }
        }
        Ok(quench_temporal::IsoDate { year, month, day })
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
            return self.temporal_duration_total_relative_date(p, &fields, index, relative_to);
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

    fn temporal_duration_total_relative_date(
        &mut self,
        p: &ResidualProgram,
        fields: &[f64; 10],
        unit: usize,
        relative_to: Value,
    ) -> Result<Value, JsError> {
        let start = self.temporal_relative_date(p, relative_to)?;
        let fields = std::array::from_fn(|index| fields[index] as i128);
        let date = quench_temporal::IsoDate {
            year: start.year,
            month: start.month,
            day: start.day,
        };
        let total = quench_temporal::total_duration(date, fields, unit)
            .ok_or_else(|| self.range_error(p, "Invalid relativeTo".into()))?;
        Ok(Value::number(total))
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
        let digits = self.duration_smallest_unit_digits(p, smallest)?.or(digits);
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
        let smallest =
            smallest.ok_or_else(|| self.range_error(p, "smallestUnit is required".into()))?;
        if !explicit_unit {
            return Err(self.range_error(p, "largestUnit or smallestUnit is required".into()));
        }
        let largest = largest.unwrap_or_else(|| {
            (0..10)
                .find(|index| fields[*index] != 0.0)
                .unwrap_or(smallest)
                .min(smallest)
        });
        let relative_date = if relative_to.is_undefined() {
            None
        } else {
            Some(self.temporal_relative_date(p, relative_to)?)
        };
        if largest > smallest {
            return Err(self.range_error(
                p,
                "largestUnit must not be smaller than smallestUnit".into(),
            ));
        }
        let needs_relative_date = smallest <= 2
            || largest <= 2
            || fields[..3].iter().any(|value| *value != 0.0);
        if needs_relative_date && relative_date.is_none() {
            return Err(self.range_error(p, "relativeTo required for calendar units".into()));
        }
        if needs_relative_date {
            return self.temporal_duration_round_relative_date(
                p,
                fields,
                largest,
                smallest,
                increment,
                &mode,
                relative_date.expect("relative date checked above"),
            );
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

    fn temporal_duration_round_relative_date(
        &mut self,
        p: &ResidualProgram,
        fields: [f64; 10],
        largest: usize,
        smallest: usize,
        increment: i128,
        mode: &str,
        relative_date: quench_temporal::IsoDate,
    ) -> Result<Value, JsError> {
        let fields_i128 = std::array::from_fn(|index| fields[index] as i128);
        let (target_date, time_remainder) = if smallest <= 1 {
            let total = quench_temporal::total_duration(relative_date, fields_i128, smallest)
                .ok_or_else(|| self.range_error(p, "Invalid relativeTo".into()))?;
            let rounded_units = round_duration_number(total / increment as f64, mode)
                * increment as f64;
            let months = if smallest == 0 {
                (rounded_units * f64::from(super::temporal_date::ISO_MONTHS_PER_YEAR)) as i128
            } else {
                rounded_units as i128
            };
            let target = super::temporal_date::shift_iso_months(relative_date.into(), months)
                .ok_or_else(|| self.range_error(p, "Invalid relativeTo".into()))?;
            (target, 0_i128)
        } else {
            let total = quench_temporal::relative_duration_nanoseconds(relative_date, fields_i128)
                .ok_or_else(|| self.range_error(p, "Invalid relativeTo".into()))?;
            let (rounded, whole_days) = if smallest
                == super::temporal_date_arithmetic::DURATION_WEEKS_FIELD
            {
                (
                    total,
                    total / super::temporal_date_arithmetic::NANOS_PER_DAY,
                )
            } else {
                let quantum = duration_round_unit_nanoseconds(smallest)
                    .checked_mul(increment)
                    .ok_or_else(|| self.range_error(p, "Invalid roundingIncrement".into()))?;
                let rounded = round_duration_integer(total, quantum, mode)
                    .checked_mul(quantum)
                    .ok_or_else(|| self.range_error(p, "Duration is out of range".into()))?;
                (
                    rounded,
                    rounded / super::temporal_date_arithmetic::NANOS_PER_DAY,
                )
            };
            let days = i64::try_from(whole_days)
                .map_err(|_| self.range_error(p, "Invalid relativeTo".into()))?;
            let target = super::temporal_date::shift_iso_days(relative_date.into(), days)
                .ok_or_else(|| self.range_error(p, "Invalid relativeTo".into()))?;
            (
                target,
                rounded % super::temporal_date_arithmetic::NANOS_PER_DAY,
            )
        };
        if largest >= 4 {
            let total = quench_temporal::relative_duration_nanoseconds(relative_date, fields_i128)
                .ok_or_else(|| self.range_error(p, "Invalid relativeTo".into()))?;
            let quantum = duration_round_unit_nanoseconds(smallest)
                .checked_mul(increment)
                .ok_or_else(|| self.range_error(p, "Invalid roundingIncrement".into()))?;
            let rounded = round_duration_integer(total, quantum, mode)
                .checked_mul(quantum)
                .ok_or_else(|| self.range_error(p, "Duration is out of range".into()))?;
            let mut result = [0.0; 10];
            balance_duration_time_units(rounded, largest, &mut result);
            self.validate_duration_fields(p, &result)?;
            return self.make_temporal_duration(p, result);
        }
        let constructor = self.temporal_plain_date_constructor(p)?;
        let start_args = [
            Value::number(f64::from(relative_date.year)),
            Value::number(f64::from(relative_date.month)),
            Value::number(f64::from(relative_date.day)),
        ];
        let start = self.temporal_plain_date_construct(p, &start_args, constructor)?;
        let end_args = [
            Value::number(f64::from(target_date.year)),
            Value::number(f64::from(target_date.month)),
            Value::number(f64::from(target_date.day)),
        ];
        let end = self.temporal_plain_date_construct(p, &end_args, constructor)?;
        let internal_options = self.object();
        for (name, value) in [
            ("largestUnit", DURATION_FIELDS[largest]),
            ("smallestUnit", "day"),
            ("roundingMode", "trunc"),
        ] {
            let atom = self.intern_atom(name);
            let value = self.heap.alloc(Cell::String(value.into()));
            self.set_property(internal_options, atom, value)?;
        }
        let date_difference = self.temporal_plain_date_difference(
            p,
            Native::TemporalPlainDateUntil,
            start,
            &[end, internal_options],
        )?;
        let mut result = self.duration_fields(p, date_difference)?;
        if smallest == super::temporal_date_arithmetic::DURATION_WEEKS_FIELD {
            let months = i128::from(result[super::temporal_date_arithmetic::DURATION_YEARS_FIELD] as i64)
                * i128::from(super::temporal_date::ISO_MONTHS_PER_YEAR)
                + i128::from(result[super::temporal_date_arithmetic::DURATION_MONTHS_FIELD] as i64);
            let cursor = super::temporal_date::shift_iso_months(relative_date.into(), months)
                .ok_or_else(|| self.range_error(p, "Invalid relativeTo".into()))?;
            let remainder_days = i128::from(super::temporal_date::days_from_iso_date(target_date))
                - i128::from(super::temporal_date::days_from_iso_date(cursor));
            let remainder = remainder_days
                .checked_mul(super::temporal_date_arithmetic::NANOS_PER_DAY)
                .and_then(|days| days.checked_add(time_remainder))
                .ok_or_else(|| self.range_error(p, "Duration is out of range".into()))?;
            let quantum = duration_round_unit_nanoseconds(smallest)
                .checked_mul(increment)
                .ok_or_else(|| self.range_error(p, "Invalid roundingIncrement".into()))?;
            let rounded_weeks = round_duration_integer(remainder, quantum, mode)
                .checked_mul(increment)
                .ok_or_else(|| self.range_error(p, "Duration is out of range".into()))?;
            result[super::temporal_date_arithmetic::DURATION_WEEKS_FIELD] =
                rounded_weeks as f64;
            result[super::temporal_date_arithmetic::DURATION_DAYS_FIELD] = 0.0;
        } else if time_remainder != 0 {
            balance_duration_time_units_into(time_remainder, &mut result);
        }
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
            Some(text) => Some(
                parse_duration_unit(text)
                    .ok_or_else(|| self.range_error(p, "Invalid largestUnit".into()))?,
            ),
        };
        let explicit_unit = smallest_text.is_some() || largest_text.is_some();
        let smallest = if let Some(text) = smallest_text.as_deref() {
            Some(
                parse_duration_unit(text)
                    .ok_or_else(|| self.range_error(p, "Invalid smallestUnit".into()))?,
            )
        } else if largest.is_some_and(|index| index <= 2) {
            largest
        } else {
            Some(9)
        };
        if !increment.is_finite()
            || increment <= 0.0
            || increment > DURATION_ROUNDING_INCREMENT_LIMIT
        {
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
        Ok((
            smallest,
            largest,
            increment as i128,
            mode,
            relative_to,
            explicit_unit,
        ))
    }
}

fn duration_total_time_out_of_range(values: &[f64]) -> bool {
    let total = values.iter().zip(DURATION_TIME_NANOSECOND_SCALES).try_fold(
        0_i128,
        |total, (value, scale)| {
            let value = *value as i128;
            total.checked_add(value.checked_mul(scale)?)
        },
    );
    total.is_none_or(|total| total.unsigned_abs() > DURATION_TOTAL_TIME_LIMIT_NANOS as u128)
}

fn balance_duration_time(left: &[f64; 10], right: &[f64; 10], subtract: bool) -> [f64; 10] {
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
        return if negative {
            -(whole as f64)
        } else {
            whole as f64
        };
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
    if negative {
        -value
    } else {
        value
    }
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

fn duration_round_unit_nanoseconds(unit: usize) -> i128 {
    match unit {
        2 => DURATION_TIME_NANOSECOND_SCALES[0] * DURATION_DAYS_PER_WEEK,
        3..=9 => DURATION_TIME_NANOSECOND_SCALES[unit - 3],
        _ => 0,
    }
}

fn balance_duration_time_units(nanoseconds: i128, largest: usize, fields: &mut [f64; 10]) {
    let sign = nanoseconds.signum();
    let mut remainder = nanoseconds.unsigned_abs();
    for unit in largest.max(4)..=9 {
        let scale = DURATION_TIME_NANOSECOND_SCALES[unit - 3] as u128;
        fields[unit] = (remainder / scale) as f64 * sign as f64;
        remainder %= scale;
    }
}

fn balance_duration_time_units_into(nanoseconds: i128, fields: &mut [f64; 10]) {
    let sign = nanoseconds.signum();
    let mut remainder = nanoseconds.unsigned_abs();
    for unit in 4..=9 {
        let scale = DURATION_TIME_NANOSECOND_SCALES[unit - 3] as u128;
        fields[unit] += (remainder / scale) as f64 * sign as f64;
        remainder %= scale;
    }
}

fn round_duration_number(value: f64, mode: &str) -> f64 {
    let truncated = value.trunc();
    let remainder = value - truncated;
    let absolute_remainder = remainder.abs();
    let increment = match mode {
        "ceil" => remainder > 0.0,
        "floor" => remainder < 0.0,
        "expand" => remainder != 0.0,
        "trunc" => false,
        "halfCeil" => absolute_remainder > 0.5 || absolute_remainder == 0.5 && value > 0.0,
        "halfFloor" => absolute_remainder > 0.5 || absolute_remainder == 0.5 && value < 0.0,
        "halfTrunc" => absolute_remainder > 0.5,
        "halfEven" => absolute_remainder > 0.5 || absolute_remainder == 0.5 && truncated % 2.0 != 0.0,
        _ => absolute_remainder >= 0.5,
    };
    if increment {
        truncated + value.signum()
    } else {
        truncated
    }
}

fn format_duration(fields: &[f64; 10]) -> String {
    format_duration_with_digits(fields, None)
}

fn format_duration_with_digits(fields: &[f64; 10], fractional_digits: Option<usize>) -> String {
    let sign = fields
        .iter()
        .find(|value| **value != 0.0)
        .map_or(1.0, |value| value.signum());
    let values = fields.map(|value| value.abs());
    let mut result = if sign < 0.0 {
        "-P".to_owned()
    } else {
        "P".to_owned()
    };
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
    if result == "P" || result == "-P" {
        format!("{result}T0S")
    } else {
        result
    }
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
    if number.fract() == 0.0 {
        format!("{number:.0}")
    } else {
        number.to_string()
    }
}
