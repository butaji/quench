use super::property_key::PropertyKey;
use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn install_bigint(&mut self, p: &ResidualProgram) -> Result<(), JsError> {
        let constructor = self.native_value(Native::BigInt);
        self.install_bigint_for_realm(p, self.realm.globals, self.object_proto, constructor)
    }

    pub(super) fn install_bigint_for_realm(
        &mut self,
        p: &ResidualProgram,
        global: Value,
        object_prototype: Value,
        constructor: Value,
    ) -> Result<(), JsError> {
        self.set_builtin_function_name(constructor, "BigInt")?;
        let prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
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
        self.set_realm_bigint_method(global, constructor, "asIntN", Native::BigIntAsIntN)?;
        self.set_realm_bigint_method(global, constructor, "asUintN", Native::BigIntAsUintN)?;
        self.set_realm_bigint_method(global, prototype, "toString", Native::BigIntToString)?;
        self.set_realm_bigint_method(global, prototype, "valueOf", Native::BigIntValueOf)?;
        if self.well_known_symbols.contains_key("toStringTag") {
            self.install_builtin_to_string_tag(prototype, "BigInt")?;
        }
        if global == self.realm.globals {
            self.global(p, "BigInt", constructor)
        } else {
            let atom = self.intern_atom("BigInt");
            self.set_property(global, atom, constructor)?;
            self.set_property_attributes(
                global,
                PropertyKey::string(atom),
                PropertyAttributes {
                    writable: true,
                    enumerable: false,
                    configurable: true,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
            Ok(())
        }
    }

    fn set_realm_bigint_method(
        &mut self,
        global: Value,
        object: Value,
        name: &str,
        native: Native,
    ) -> Result<(), JsError> {
        let function = self.native_with_realm(native, global, global);
        self.set_builtin_function_name(function, name)?;
        self.set_builtin_value_named(object, name, function)
    }

    pub(super) fn bigint_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::BigIntValueOf => self.bigint_value_of(p, this),
            Native::BigIntToString => self.bigint_to_string(p, this, args),
            Native::BigIntAsIntN => self.bigint_as_n(p, args, true),
            Native::BigIntAsUintN => self.bigint_as_n(p, args, false),
            _ => Err(JsError("invalid BigInt method".into())),
        }
    }

    fn bigint_value_of(&mut self, p: &ResidualProgram, this: Value) -> Result<Value, JsError> {
        if matches!(self.heap.get(this), Some(Cell::BigInt(_))) {
            return Ok(this);
        }
        let value_atom = self.intern_atom("\0rqj:bigint-value");
        let value = self
            .own_property(this, value_atom)
            .unwrap_or(Value::UNDEFINED);
        if matches!(self.heap.get(value), Some(Cell::BigInt(_))) {
            Ok(value)
        } else {
            Err(self.type_error(
                p,
                "BigInt.prototype.valueOf called on incompatible receiver".into(),
            ))
        }
    }

    fn bigint_to_string(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let bigint = self.bigint_value_of(p, this)?;
        let Some(Cell::BigInt(value)) = self.heap.get(bigint).cloned() else {
            return Err(self.type_error(p, "invalid BigInt receiver".into()));
        };
        let radix = args
            .first()
            .copied()
            .map(|value| self.to_number(p, value))
            .transpose()?
            .unwrap_or(10.0);
        let radix = if radix.is_nan() { 10.0 } else { radix.trunc() };
        if !(2.0..=36.0).contains(&radix) {
            return Err(self.range_error(p, "invalid BigInt radix".into()));
        }
        let value = value
            .parse::<num_bigint::BigInt>()
            .map_err(|_| self.type_error(p, "invalid BigInt value".into()))?;
        Ok(self
            .heap
            .alloc(Cell::String(value.to_str_radix(radix as u32).into())))
    }

    fn bigint_as_n(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        signed: bool,
    ) -> Result<Value, JsError> {
        let width = self.bigint_width(p, args.first().copied())?;
        let value = self.to_bigint(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
        let magnitude_bits = usize::try_from(value.magnitude().bits()).unwrap_or(usize::MAX);
        let unchanged = if signed {
            width > magnitude_bits.saturating_add(1)
        } else {
            value.sign() != num_bigint::Sign::Minus && width >= magnitude_bits
        };
        if unchanged {
            return Ok(self.heap.alloc(Cell::BigInt(value.to_string())));
        }
        let modulus = num_bigint::BigInt::from(1_u8) << width;
        let mut reduced = ((value % &modulus) + &modulus) % &modulus;
        if signed && width > 0 && reduced >= (num_bigint::BigInt::from(1_u8) << (width - 1)) {
            reduced -= &modulus;
        }
        Ok(self.heap.alloc(Cell::BigInt(reduced.to_string())))
    }

    fn bigint_width(
        &mut self,
        p: &ResidualProgram,
        value: Option<Value>,
    ) -> Result<usize, JsError> {
        let value = value.ok_or_else(|| self.type_error(p, "BigInt.asN requires bits".into()))?;
        let number = self.to_number(p, value)?;
        if number.is_infinite() {
            return Err(self.range_error(p, "invalid BigInt width".into()));
        }
        let integer = if number.is_nan() { 0.0 } else { number.trunc() };
        if !(0.0..=MAX_SAFE_INTEGER).contains(&integer) {
            return Err(self.range_error(p, "invalid BigInt width".into()));
        }
        Ok(integer as usize)
    }
}
