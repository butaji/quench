use super::*;
const MAX_INSTANT_EPOCH_NANOSECONDS: i128 = 8_640_000_000_000_000_000_000;
const INSTANT_NANOSECONDS_PER_MILLISECOND: i128 = 1_000_000;
pub(super) const INSTANT_NANOSECONDS_PER_SECOND: i128 = 1_000_000_000;
const INSTANT_NANOSECONDS_PER_MICROSECOND: i128 = 1_000;
const INSTANT_SECONDS_PER_MINUTE: i128 = 60;
const INSTANT_MINUTES_PER_HOUR: i128 = 60;
const INSTANT_HOURS_PER_DAY: i128 = 24;
const INSTANT_NANOSECONDS_PER_MINUTE: i128 =
    INSTANT_SECONDS_PER_MINUTE * INSTANT_NANOSECONDS_PER_SECOND;
const INSTANT_NANOSECONDS_PER_HOUR: i128 =
    INSTANT_MINUTES_PER_HOUR * INSTANT_NANOSECONDS_PER_MINUTE;
const INSTANT_NANOSECONDS_PER_DAY: i128 = INSTANT_HOURS_PER_DAY * INSTANT_NANOSECONDS_PER_HOUR;
const INSTANT_DURATION_UNIT_SCALES: [i128; 7] = [
    INSTANT_NANOSECONDS_PER_DAY,
    INSTANT_NANOSECONDS_PER_HOUR,
    INSTANT_NANOSECONDS_PER_MINUTE,
    INSTANT_NANOSECONDS_PER_SECOND,
    INSTANT_NANOSECONDS_PER_MILLISECOND as i128,
    INSTANT_NANOSECONDS_PER_MICROSECOND,
    1,
];

impl<H: Host> Vm<H> {
    pub(super) fn install_temporal_instant(
        &mut self,
        p: &ResidualProgram,
        temporal: Value,
    ) -> Result<(), JsError> {
        let constructor =
            self.native_with_realm(Native::TemporalInstant, temporal, self.realm.globals);
        self.set_builtin_function_name(constructor, "Instant")?;
        let prototype = self.object();
        self.set_builtin_value_named(constructor, "prototype", prototype)?;
        self.lock_instant_constructor_prototype(constructor)?;
        self.set_builtin_value_named(prototype, "constructor", constructor)?;
        self.install_instant_statics(p, constructor)?;
        self.install_instant_accessors(prototype)?;
        self.install_instant_methods(p, prototype)?;
        self.install_instant_to_string_tag(prototype)?;
        self.set_builtin_value_named(temporal, "Instant", constructor)
    }

    fn lock_instant_constructor_prototype(&mut self, constructor: Value) -> Result<(), JsError> {
        let key = self.intern_atom("prototype");
        self.set_property_attributes(
            constructor,
            PropertyKey::string(key),
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

    fn install_instant_statics(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
    ) -> Result<(), JsError> {
        for (name, native) in [
            ("from", Native::TemporalInstantFrom),
            ("compare", Native::TemporalInstantCompare),
            (
                "fromEpochMilliseconds",
                Native::TemporalInstantFromEpochMilliseconds,
            ),
            (
                "fromEpochNanoseconds",
                Native::TemporalInstantFromEpochNanoseconds,
            ),
        ] {
            self.set_builtin_named(p, constructor, name, native)?;
        }
        Ok(())
    }

    fn install_instant_accessors(&mut self, prototype: Value) -> Result<(), JsError> {
        for (name, native) in [
            (
                "epochNanoseconds",
                Native::TemporalInstantEpochNanosecondsGetter,
            ),
            (
                "epochMilliseconds",
                Native::TemporalInstantEpochMillisecondsGetter,
            ),
        ] {
            let getter = self.native_value(native);
            self.set_builtin_function_name(getter, &format!("get {name}"))?;
            let key = self.intern_atom(name);
            self.set_property(prototype, key, Value::UNDEFINED)?;
            self.set_property_attributes(
                prototype,
                PropertyKey::string(key),
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
        Ok(())
    }

    fn install_instant_methods(
        &mut self,
        p: &ResidualProgram,
        prototype: Value,
    ) -> Result<(), JsError> {
        for (name, native) in [
            ("toString", Native::TemporalInstantToString),
            ("toJSON", Native::TemporalInstantToJSON),
            ("valueOf", Native::TemporalInstantValueOf),
            ("equals", Native::TemporalInstantEquals),
            ("add", Native::TemporalInstantAdd),
            ("subtract", Native::TemporalInstantSubtract),
            ("round", Native::TemporalInstantRound),
        ] {
            self.set_builtin_named(p, prototype, name, native)?;
        }
        Ok(())
    }

    fn install_instant_to_string_tag(&mut self, prototype: Value) -> Result<(), JsError> {
        let Some(symbol) = self.well_known_symbols.get("toStringTag").copied() else {
            return Ok(());
        };
        let tag = self.heap.alloc(Cell::String("Temporal.Instant".into()));
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
        Ok(())
    }

    pub(super) fn temporal_instant_construct(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let bigint = self.to_bigint(p, value)?;
        let epoch = bigint
            .to_string()
            .parse::<i128>()
            .map_err(|_| self.range_error(p, "epochNanoseconds outside supported range".into()))?;
        if epoch.unsigned_abs() > MAX_INSTANT_EPOCH_NANOSECONDS as u128 {
            return Err(self.range_error(p, "epochNanoseconds outside supported range".into()));
        }
        let prototype_atom = self.intern_atom("prototype");
        let prototype = self.get_property(p, new_target, prototype_atom)?;
        let prototype = if self.is_object_like(prototype) {
            prototype
        } else {
            self.object_proto
        };
        Ok(self.heap.alloc(Cell::TemporalInstant {
            object: Box::new(Self::empty_object(prototype)),
            epoch_nanoseconds: epoch,
        }))
    }

    pub(super) fn temporal_instant_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::TemporalInstant => {
                Err(self.type_error(p, "Temporal.Instant requires new".into()))
            }
            Native::TemporalInstantFrom => {
                self.temporal_instant_from(p, args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::TemporalInstantFromEpochNanoseconds
            | Native::TemporalInstantFromEpochMilliseconds => {
                self.temporal_instant_epoch_factory(p, native, args)
            }
            Native::TemporalInstantCompare => self.temporal_instant_compare(p, args),
            Native::TemporalInstantEpochNanosecondsGetter
            | Native::TemporalInstantEpochMillisecondsGetter => {
                self.temporal_instant_getter(p, native, this)
            }
            Native::TemporalInstantToString | Native::TemporalInstantToJSON => {
                self.temporal_instant_to_string(p, this)
            }
            Native::TemporalInstantValueOf => self.temporal_instant_value_of(p),
            Native::TemporalInstantEquals => self.temporal_instant_equals(p, this, args),
            Native::TemporalInstantAdd | Native::TemporalInstantSubtract => {
                self.temporal_instant_arithmetic(p, native, this, args)
            }
            Native::TemporalInstantRound => {
                super::temporal_instant_round::round(self, p, this, args)
            }
            _ => unreachable!("not a Temporal.Instant native"),
        }
    }

    fn temporal_instant_from(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<Value, JsError> {
        let epoch = self.temporal_instant_input(p, value)?;
        let constructor = self.temporal_instant_constructor(p)?;
        self.make_temporal_instant(p, epoch, constructor)
    }

    fn temporal_instant_epoch_factory(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let epoch = match native {
            Native::TemporalInstantFromEpochNanoseconds => self
                .to_bigint(p, value)?
                .to_string()
                .parse::<i128>()
                .map_err(|_| {
                    self.range_error(p, "epochNanoseconds outside supported range".into())
                })?,
            Native::TemporalInstantFromEpochMilliseconds => {
                let milliseconds = self.to_number(p, value)?;
                let limit =
                    (MAX_INSTANT_EPOCH_NANOSECONDS / INSTANT_NANOSECONDS_PER_MILLISECOND) as f64;
                if !milliseconds.is_finite()
                    || milliseconds.fract() != 0.0
                    || milliseconds.abs() > limit
                {
                    return Err(self.range_error(p, "Invalid epochMilliseconds".into()));
                }
                (milliseconds as i128) * INSTANT_NANOSECONDS_PER_MILLISECOND
            }
            _ => unreachable!("not an epoch factory"),
        };
        let constructor = self.temporal_instant_constructor(p)?;
        self.make_temporal_instant(p, epoch, constructor)
    }

    fn temporal_instant_compare(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let left =
            self.temporal_instant_input(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let right =
            self.temporal_instant_input(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
        let order = match left.cmp(&right) {
            std::cmp::Ordering::Less => -1.0,
            std::cmp::Ordering::Equal => 0.0,
            std::cmp::Ordering::Greater => 1.0,
        };
        Ok(Value::number(order))
    }

    fn temporal_instant_getter(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
    ) -> Result<Value, JsError> {
        let epoch = self.temporal_instant_epoch(p, this)?;
        match native {
            Native::TemporalInstantEpochNanosecondsGetter => {
                Ok(self.heap.alloc(Cell::BigInt(epoch.to_string())))
            }
            Native::TemporalInstantEpochMillisecondsGetter => Ok(Value::number(
                epoch.div_euclid(INSTANT_NANOSECONDS_PER_MILLISECOND) as f64,
            )),
            _ => unreachable!("not an Instant getter"),
        }
    }

    fn temporal_instant_to_string(
        &mut self,
        p: &ResidualProgram,
        this: Value,
    ) -> Result<Value, JsError> {
        let epoch = self.temporal_instant_epoch(p, this)?;
        let text = super::temporal_instant_format::format_instant(epoch)
            .ok_or_else(|| self.range_error(p, "Invalid epochNanoseconds".into()))?;
        Ok(self.heap.alloc(Cell::String(text.into())))
    }

    fn temporal_instant_value_of(&mut self, p: &ResidualProgram) -> Result<Value, JsError> {
        Err(self.type_error(
            p,
            "Temporal.Instant.prototype.valueOf is not allowed".into(),
        ))
    }

    fn temporal_instant_equals(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let epoch = self.temporal_instant_epoch(p, this)?;
        let other =
            self.temporal_instant_input(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        Ok(if epoch == other {
            Value::TRUE
        } else {
            Value::FALSE
        })
    }

    fn temporal_instant_arithmetic(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let epoch = self.temporal_instant_epoch(p, this)?;
        let input = args.first().copied().unwrap_or(Value::UNDEFINED);
        let duration = self.temporal_instant_duration_nanoseconds(p, input)?;
        let delta = if native == Native::TemporalInstantSubtract {
            duration.checked_neg()
        } else {
            Some(duration)
        }
        .ok_or_else(|| self.range_error(p, "Instant duration is outside supported range".into()))?;
        let epoch = epoch
            .checked_add(delta)
            .ok_or_else(|| self.range_error(p, "Instant is outside supported range".into()))?;
        let constructor = self.temporal_instant_constructor(p)?;
        self.make_temporal_instant(p, epoch, constructor)
    }

    pub(super) fn make_temporal_instant(
        &mut self,
        p: &ResidualProgram,
        epoch: i128,
        new_target: Value,
    ) -> Result<Value, JsError> {
        if epoch.unsigned_abs() > MAX_INSTANT_EPOCH_NANOSECONDS as u128 {
            return Err(self.range_error(p, "epochNanoseconds outside supported range".into()));
        }
        let prototype_atom = self.intern_atom("prototype");
        let prototype = self.get_property(p, new_target, prototype_atom)?;
        let prototype = if self.is_object_like(prototype) {
            prototype
        } else {
            self.object_proto
        };
        Ok(self.heap.alloc(Cell::TemporalInstant {
            object: Box::new(Self::empty_object(prototype)),
            epoch_nanoseconds: epoch,
        }))
    }

    pub(super) fn temporal_instant_epoch(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<i128, JsError> {
        match self.heap.get(value) {
            Some(Cell::TemporalInstant {
                epoch_nanoseconds, ..
            }) => Ok(*epoch_nanoseconds),
            _ => Err(self.type_error(
                p,
                "Temporal.Instant method called on incompatible receiver".into(),
            )),
        }
    }

    pub(super) fn temporal_instant_constructor(
        &mut self,
        p: &ResidualProgram,
    ) -> Result<Value, JsError> {
        let temporal_atom = self.intern_atom("Temporal");
        let temporal = self.get_property(p, self.realm.globals, temporal_atom)?;
        let instant_atom = self.intern_atom("Instant");
        self.get_property(p, temporal, instant_atom)
    }

    fn temporal_instant_input(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<i128, JsError> {
        if let Some(Cell::TemporalInstant {
            epoch_nanoseconds, ..
        }) = self.heap.get(value)
        {
            return Ok(*epoch_nanoseconds);
        }
        if let Some(Cell::TemporalZonedDateTime {
            epoch_nanoseconds, ..
        }) = self.heap.get(value)
        {
            return Ok(*epoch_nanoseconds);
        }
        if value.is_null() || value.is_undefined() {
            return Err(self.type_error(p, "Invalid Instant input".into()));
        }
        let text = self.to_string(p, value)?.to_string();
        let base = text.split_once('[').map_or(text.as_str(), |(base, _)| base);
        let Some((local, Some(offset), leap_second)) =
            super::temporal_zoned_date_time::parse_iso_zoned_base(base)
        else {
            return Err(self.range_error(p, "Invalid Instant string".into()));
        };
        let utc = local.and_utc();
        let epoch = i128::from(utc.timestamp()) * INSTANT_NANOSECONDS_PER_SECOND
            + i128::from(utc.timestamp_subsec_nanos())
            - offset
            + if leap_second {
                INSTANT_NANOSECONDS_PER_SECOND
            } else {
                0
            };
        if epoch.unsigned_abs() > MAX_INSTANT_EPOCH_NANOSECONDS as u128 {
            return Err(self.range_error(p, "Instant is outside supported range".into()));
        }
        Ok(epoch)
    }

    fn temporal_instant_duration_nanoseconds(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<i128, JsError> {
        let fields = self.duration_record(p, value)?;
        if fields[..4].iter().any(|field| *field != 0.0) {
            return Err(self.range_error(p, "Instant arithmetic does not accept date units".into()));
        }
        if fields[4..]
            .iter()
            .any(|field| !field.is_finite() || field.fract() != 0.0)
        {
            return Err(self.range_error(p, "Instant duration fields must be integral".into()));
        }
        let sign = fields
            .iter()
            .find(|field| **field != 0.0)
            .map_or(0.0, |field| field.signum());
        if fields
            .iter()
            .any(|field| *field != 0.0 && field.signum() != sign)
        {
            return Err(self.range_error(p, "Instant duration fields must share a sign".into()));
        }
        fields[3..]
            .iter()
            .zip(INSTANT_DURATION_UNIT_SCALES)
            .try_fold(0_i128, |total, (field, scale)| {
                let field = *field as i128;
                total.checked_add(field.checked_mul(scale)?)
            })
            .ok_or_else(|| {
                self.range_error(p, "Instant duration is outside supported range".into())
            })
    }
}
