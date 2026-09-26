use std::cmp::Ordering;

use crate::bigint::{
    IEEE754_EXPONENT_BIAS, IEEE754_FRACTION_BITS, IEEE754_MAX_EXPONENT_BITS,
    IEEE754_SUBNORMAL_EXPONENT,
};
use num_bigint::{BigInt, Sign};

impl<H: Host> Vm<H> {
    pub(super) fn compare_relational(
        &mut self,
        p: &ResidualProgram,
        operator: super::operations::RelationalOperator,
        left: Value,
        right: Value,
    ) -> Result<bool, JsError> {
        if let (Some(left), Some(right)) = (left.as_number(), right.as_number()) {
            return Ok(!left.is_nan() && !right.is_nan() && compare_numbers(left, right, operator));
        }
        let left = self.to_primitive(p, left, "number")?;
        let right = self.to_primitive(p, right, "number")?;
        if let (Some(Cell::String(left)), Some(Cell::String(right))) =
            (self.heap.get(left), self.heap.get(right))
        {
            return Ok(operator.matches(left.units().cmp(right.units())));
        }
        if let Some(ordering) = compare_bigint_string(&self.heap, left, right) {
            return Ok(ordering.is_some_and(|ordering| operator.matches(ordering)));
        }
        let left = self.to_numeric_value(p, left)?;
        let right = self.to_numeric_value(p, right)?;
        if let Some(ordering) = compare_bigint_values(&self.heap, left, right) {
            return Ok(ordering.is_some_and(|ordering| operator.matches(ordering)));
        }
        let (Some(left), Some(right)) = (left.as_number(), right.as_number()) else {
            return Ok(false);
        };
        Ok(compare_numbers(left, right, operator))
    }

    pub(super) fn has_property(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        key: Value,
    ) -> Result<bool, JsError> {
        if let Some(Cell::Proxy {
            target, handler, ..
        }) = self.heap.get(object).cloned()
        {
            if handler.is_null() {
                return Err(JsError("cannot access a revoked proxy".into()));
            }
            let trap_atom = self.intern_atom("has");
            let trap = self.get_property(p, handler, trap_atom)?;
            if self.is_function(trap) {
                let key = self.to_property_key(p, key)?;
                let result = self.call_value(p, trap, handler, &[target, key])?;
                if self.truthy(result) {
                    return Ok(true);
                }
                let descriptor = self.object_get_own_property_descriptor(p, &[target, key])?;
                if !descriptor.is_undefined() {
                    let extensible = self.object_is_extensible(p, &[target])?;
                    if !self.descriptor_flag(descriptor, "configurable") || !self.truthy(extensible)
                    {
                        return Err(self.type_error(
                            p,
                            "proxy has trap hid a property from a non-extensible target".into(),
                        ));
                    }
                }
                return Ok(false);
            }
            if trap.is_null() || trap.is_undefined() {
                return self.has_property(p, target, key);
            }
            return Err(self.type_error(p, "proxy has trap is not callable".into()));
        }
        if self.object_data(object).is_none() {
            return Err(JsError("right-hand side of 'in' is not an object".into()));
        }
        let symbol_key = matches!(self.heap.get(key), Some(Cell::Symbol(_))).then_some(key);
        let atom = if symbol_key.is_none() {
            let key = self.to_string(p, key)?;
            Some(self.intern_atom(&key))
        } else {
            None
        };
        let property_key = atom.map_or_else(
            || super::property_key::PropertyKey::symbol(symbol_key.expect("symbol key")),
            super::property_key::PropertyKey::string,
        );
        let mut current = object;
        loop {
            if matches!(self.heap.get(current), Some(Cell::Proxy { .. })) {
                return self.has_property(p, current, key);
            }
            self.evaluate_deferred_namespace_for_key(p, current, Some(property_key))?;
            if atom == Some(self.length_atom)
                && matches!(
                    self.heap.get(current),
                    Some(Cell::Array { .. } | Cell::TypedArray { .. })
                )
            {
                return Ok(true);
            }
            if self.property_attributes(current, property_key).is_some()
                || symbol_key.is_some_and(|key| self.symbol_property(current, key).is_some())
            {
                return Ok(true);
            }
            if atom.is_some_and(|atom| {
                super::object_static::array_index(self.atom_name(atom)).is_some_and(|index| {
                    match self.heap.get(current) {
                        Some(Cell::Array { .. }) => {
                            let index = index as usize;
                            self.has_own_array_index(current, index)
                        }
                        Some(Cell::TypedArray { .. }) => self
                            .typed_array_length(current)
                            .is_some_and(|length| (index as usize) < length),
                        _ => self.indexed_view_property(current, atom).is_some(),
                    }
                })
            }) {
                return Ok(true);
            }
            if atom.is_some_and(|atom| self.own_property(current, atom).is_some()) {
                return Ok(true);
            }
            let Some(data) = self.object_data(current) else {
                return Ok(false);
            };
            if data.proto.is_null() {
                return Ok(false);
            }
            current = data.proto;
        }
    }

    pub(super) fn instanceof(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        constructor: Value,
    ) -> Result<bool, JsError> {
        if !self.is_object_like(constructor) {
            return Err(
                self.type_error(p, "right-hand side of 'instanceof' is not an object".into())
            );
        }
        if let Some(has_instance) = self.well_known_symbols.get("hasInstance").copied() {
            let method = self.get_index(p, constructor, has_instance)?;
            if !method.is_undefined() && !method.is_null() {
                if !self.is_function(method) {
                    return Err(self.type_error(p, "@@hasInstance is not callable".into()));
                }
                let result = self.call_value(p, method, constructor, &[value])?;
                return Ok(self.truthy(result));
            }
        }
        if !self.is_function(constructor) {
            return Err(
                self.type_error(p, "right-hand side of 'instanceof' is not callable".into())
            );
        }
        self.ordinary_has_instance(p, constructor, value)
    }

    pub(super) fn ordinary_has_instance(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
        value: Value,
    ) -> Result<bool, JsError> {
        if !self.is_function(constructor) {
            return Ok(false);
        }
        let bound_env = match self.heap.get(constructor) {
            Some(Cell::Function {
                kind: FunctionKind::Native(Native::FunctionBoundCall),
                env,
                ..
            }) => Some(*env),
            _ => None,
        };
        if let Some(env) = bound_env {
            let target_atom = self.intern_atom("\0rqj:bound-target");
            let target = self
                .own_property(env, target_atom)
                .unwrap_or(Value::UNDEFINED);
            return self.ordinary_has_instance(p, target, value);
        }
        if self.object_data(value).is_none() {
            return Ok(false);
        }
        let prototype_atom = self.intern_atom("prototype");
        let prototype = self.get_property(p, constructor, prototype_atom)?;
        if self.object_data(prototype).is_none() {
            return Err(self.type_error(p, "instanceof prototype is not an object".into()));
        }
        let mut current = self.object_get_prototype_of(p, value)?;
        loop {
            if current == prototype {
                return Ok(true);
            }
            if current.is_null() {
                return Ok(false);
            }
            current = self.object_get_prototype_of(p, current)?;
        }
    }
}

fn compare_numbers(left: f64, right: f64, operator: super::operations::RelationalOperator) -> bool {
    left.partial_cmp(&right)
        .is_some_and(|ordering| operator.matches(ordering))
}

fn compare_bigint_string(heap: &Heap, left: Value, right: Value) -> Option<Option<Ordering>> {
    match (heap.get(left), heap.get(right)) {
        (Some(Cell::BigInt(bigint)), Some(Cell::String(string))) => Some(
            crate::bigint::parse_string(string.host_string())
                .and_then(|right| Some(bigint.parse::<BigInt>().ok()?.cmp(&right))),
        ),
        (Some(Cell::String(string)), Some(Cell::BigInt(bigint))) => Some(
            crate::bigint::parse_string(string.host_string())
                .and_then(|left| Some(left.cmp(&bigint.parse::<BigInt>().ok()?))),
        ),
        _ => None,
    }
}

fn compare_bigint_values(heap: &Heap, left: Value, right: Value) -> Option<Option<Ordering>> {
    match (
        heap.get(left),
        heap.get(right),
        left.as_number(),
        right.as_number(),
    ) {
        (Some(Cell::BigInt(left)), Some(Cell::BigInt(right)), _, _) => Some(Some(
            left.parse::<BigInt>()
                .ok()?
                .cmp(&right.parse::<BigInt>().ok()?),
        )),
        (Some(Cell::BigInt(bigint)), _, _, Some(number)) => {
            Some(bigint_number_ordering(bigint, number))
        }
        (_, Some(Cell::BigInt(bigint)), Some(number), _) => {
            Some(bigint_number_ordering(bigint, number).map(Ordering::reverse))
        }
        _ => None,
    }
}

fn bigint_number_ordering(bigint: &str, number: f64) -> Option<Ordering> {
    if number.is_nan() {
        return None;
    }
    let integer = bigint.parse::<BigInt>().ok()?;
    if number == f64::INFINITY {
        return Some(Ordering::Less);
    }
    if number == f64::NEG_INFINITY {
        return Some(Ordering::Greater);
    }
    let sign = integer.sign();
    if number == 0.0 {
        return Some(integer.cmp(&BigInt::from(0)));
    }
    if sign == Sign::Minus && number.is_sign_positive() {
        return Some(Ordering::Less);
    }
    if sign != Sign::Minus && number.is_sign_negative() {
        return Some(Ordering::Greater);
    }
    let magnitude = if sign == Sign::Minus {
        -integer
    } else {
        integer
    };
    let ordering = compare_positive_bigint_number(magnitude, number.abs());
    Some(if number.is_sign_negative() {
        ordering.reverse()
    } else {
        ordering
    })
}

fn compare_positive_bigint_number(integer: BigInt, number: f64) -> Ordering {
    let bits = number.to_bits();
    let exponent_bits = ((bits >> IEEE754_FRACTION_BITS) & IEEE754_MAX_EXPONENT_BITS) as i32;
    let fraction_mask = (1_u64 << IEEE754_FRACTION_BITS) - 1;
    let significand = bits & fraction_mask;
    let significand = if exponent_bits == 0 {
        significand
    } else {
        significand | (1_u64 << IEEE754_FRACTION_BITS)
    };
    let exponent = if exponent_bits == 0 {
        IEEE754_SUBNORMAL_EXPONENT
    } else {
        exponent_bits - (IEEE754_EXPONENT_BIAS + IEEE754_FRACTION_BITS as i32)
    };
    let significand = BigInt::from(significand);
    if exponent >= 0 {
        integer.cmp(&(significand << exponent as usize))
    } else {
        (integer << (-exponent) as usize).cmp(&significand)
    }
}
