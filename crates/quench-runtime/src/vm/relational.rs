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
        self.with_coerced_operands(
            p,
            super::operations::OperandCoercion::PrimitiveNumber,
            left,
            right,
            |vm, left, right| {
                if let (Some(Cell::String(left)), Some(Cell::String(right))) =
                    (vm.heap.get(left), vm.heap.get(right))
                {
                    return Ok(operator.matches(left.units().cmp(right.units())));
                }
                if let Some(ordering) = compare_bigint_string(&vm.heap, left, right) {
                    return Ok(ordering.is_some_and(|ordering| operator.matches(ordering)));
                }
                let left = vm.to_numeric(p, left)?;
                let right = vm.to_numeric(p, right)?;
                if let Some(ordering) = compare_bigint_values(&vm.heap, left, right) {
                    return Ok(ordering.is_some_and(|ordering| operator.matches(ordering)));
                }
                let (Some(left), Some(right)) = (left.as_number(), right.as_number()) else {
                    return Ok(false);
                };
                Ok(compare_numbers(left, right, operator))
            },
        )
    }

    pub(super) fn has_property(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        key: Value,
    ) -> Result<bool, JsError> {
        if !self.is_object_like(object) {
            return Err(self.type_error(p, "right-hand side of 'in' is not an object".into()));
        }
        self.with_call_roots([object, key], |vm| {
            let key = vm.to_property_key(p, key)?;
            vm.has_property_key(p, object, key)
        })
    }

    fn has_property_key(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        key: Value,
    ) -> Result<bool, JsError> {
        let _stack = self.enter_stack()?;
        self.with_call_roots([object, key], |vm| {
            if let Some(Cell::Proxy {
                target, handler, ..
            }) = vm.heap.get(object).cloned()
            {
                if handler.is_null() {
                    return Err(vm.type_error(p, "cannot access a revoked proxy".into()));
                }
                return vm.with_call_roots([target, handler], |vm| {
                    let trap_name = "has";
                    let trap_atom = vm.intern_atom(trap_name);
                    let trap = vm.get_property(p, handler, trap_atom)?;
                    if trap.is_null() || trap.is_undefined() {
                        return vm.has_property_key(p, target, key);
                    }
                    if !vm.is_function(trap) {
                        return Err(vm.type_error(p, "proxy has trap is not callable".into()));
                    }
                    let result = vm.call_value(p, trap, handler, &[target, key])?;
                    if vm.truthy(result) {
                        return Ok(true);
                    }
                    vm.validate_proxy_property_absence(p, target, key, trap_name)?;
                    Ok(false)
                });
            }
            let key_atom = match vm.heap.get(key).cloned() {
                Some(Cell::String(name)) => Some(vm.intern_js_atom(&name)),
                _ => None,
            };
            if matches!(vm.heap.get(object), Some(Cell::TypedArray { .. }))
                && let Some(Cell::String(name)) = vm.heap.get(key).cloned()
            {
                match Self::typed_array_index_key(name.host_string()) {
                    super::object_descriptors::TypedArrayIndexKey::Index(index) => {
                        return Ok(vm
                            .typed_array_length(object)
                            .is_some_and(|length| index < length));
                    }
                    super::object_descriptors::TypedArrayIndexKey::Invalid => return Ok(false),
                    super::object_descriptors::TypedArrayIndexKey::NotCanonical => {}
                }
            }
            let current_root = vm.heap.root(object);
            let result = (|| {
                loop {
                    let current = vm
                        .root_value(current_root)
                        .expect("rooted property lookup object");
                    if matches!(vm.heap.get(current), Some(Cell::Proxy { .. })) {
                        return vm.has_property_key(p, current, key);
                    }
                    if let Some(atom) = key_atom
                        && vm
                            .object_data(current)
                            .is_some_and(Object::is_module_namespace)
                    {
                        vm.evaluate_deferred_namespace_for_key(
                            p,
                            current,
                            Some(crate::vm::property_key::PropertyKey::string(atom)),
                        )?;
                        if vm.module_binding_value(current, atom).is_some()
                            || vm.own_property(current, atom).is_some()
                        {
                            return Ok(true);
                        }
                    }
                    let descriptor = vm.object_get_own_property_descriptor(p, &[current, key])?;
                    if !descriptor.is_undefined() {
                        return Ok(true);
                    }
                    let next = vm.object_get_prototype_of(p, current)?;
                    vm.heap.update_root(current_root, next);
                    if next.is_null() {
                        return Ok(false);
                    }
                }
            })();
            vm.heap.release_root(current_root);
            result
        })
    }

    pub(super) fn instanceof(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        constructor: Value,
    ) -> Result<bool, JsError> {
        let _stack = self.enter_stack()?;
        self.with_call_roots([value, constructor], |vm| {
            if !vm.is_object_like(constructor) {
                return Err(vm.type_error(p, "right-hand side of 'instanceof' is not an object".into()));
            }
            if let Some(has_instance) = vm.well_known_symbols.get("hasInstance").copied() {
                let method = vm.get_index(p, constructor, has_instance)?;
                if !method.is_undefined() && !method.is_null() {
                    if !vm.is_function(method) {
                        return Err(vm.type_error(p, "@@hasInstance is not callable".into()));
                    }
                    let result = vm.call_value(p, method, constructor, &[value])?;
                    return Ok(vm.truthy(result));
                }
            }
            if !vm.is_function(constructor) {
                return Err(vm.type_error(p, "right-hand side of 'instanceof' is not callable".into()));
            }
            vm.ordinary_has_instance(p, constructor, value)
        })
    }

    pub(super) fn ordinary_has_instance(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
        value: Value,
    ) -> Result<bool, JsError> {
        let _stack = self.enter_stack()?;
        self.with_call_roots([constructor, value], |vm| {
            if !vm.is_function(constructor) {
                return Ok(false);
            }
            let bound_env = match vm.heap.get(constructor) {
                Some(Cell::Function {
                    kind: FunctionKind::Native(Native::FunctionBoundCall),
                    env,
                    ..
                }) => Some(*env),
                _ => None,
            };
            if let Some(env) = bound_env {
                let target_atom = vm.intern_atom("\0rqj:bound-target");
                let target = vm
                    .own_property(env, target_atom)
                    .unwrap_or(Value::UNDEFINED);
                return vm.instanceof(p, value, target);
            }
            if !vm.is_object_like(value) {
                return Ok(false);
            }
            let prototype_atom = vm.intern_atom("prototype");
            let prototype = vm.get_property(p, constructor, prototype_atom)?;
            if !vm.is_object_like(prototype) {
                return Err(vm.type_error(p, "instanceof prototype is not an object".into()));
            }
            vm.with_call_roots([prototype], |vm| {
                let current_root = vm.heap.root(value);
                let result = (|| {
                    loop {
                        let current = vm
                            .root_value(current_root)
                            .expect("rooted instanceof traversal object");
                        let next = vm.object_get_prototype_of(p, current)?;
                        vm.heap.update_root(current_root, next);
                        if next.is_null() {
                            return Ok(false);
                        }
                        if next == prototype {
                            return Ok(true);
                        }
                    }
                })();
                vm.heap.release_root(current_root);
                result
            })
        })
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
