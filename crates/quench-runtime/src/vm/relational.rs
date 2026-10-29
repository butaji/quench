use std::cmp::Ordering;
use crate::heap::ProtoLink;

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
                if let (Some(left), Some(right)) = (vm.heap.string(left), vm.heap.string(right))
                {
                    return Ok(operator.matches(left.units().cmp(&right.units())));
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
        // Array elements live outside the ordinary property table. Resolve both dense and
        // sparse own elements directly, and avoid creating an atom for every absent index
        // when the array and its prototype chain provably have no property there. This is
        // especially important for methods such as `every` that perform HasProperty across
        // long sparse ranges. Keep descriptors and exotic prototype chains on the general
        // path so accessors, proxies and inherited indexed properties retain their behavior.
        if let Some(index) = self.array_index_key(key)
            && self.heap.array(object).is_some()
        {
            let index = index as usize;
            if self.own_element_present(object, index) {
                return Ok(true);
            }
            if self.array_descriptor_entry(object, index).is_none()
                && self.inherited_index_absent(object, index)
            {
                return Ok(false);
            }
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
            if vm.is_host_private_symbol(key) {
                return Ok(!matches!(vm.heap.get(object), Some(Cell::Proxy { .. }))
                    && vm.symbol_property(object, key).is_some());
            }
            if let Some((target, handler)) = vm.proxy_parts(object)
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
            let key_atom = vm.string_key_atom(key);
            if matches!(vm.heap.get(object), Some(Cell::TypedArray { .. }))
                && let Some(name) = vm.heap.string(key).cloned()
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
                    // [[GetOwnProperty]] itself: presence needs no descriptor object, and an
                    // authoritative shape needs no property lookup either.
                    let own = match key_atom.and_then(|atom| vm.shape_own_presence(current, atom)) {
                        Some(present) => present,
                        None => vm.ordinary_get_own_property(p, current, key)?.is_some(),
                    };
                    if own {
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

    /// Whether an ordinary object has an own property `atom`, read from its shape where the
    /// shape answers it (`shape_property_lookup`): a lazy data property holds its slot before
    /// it materializes, so presence needs no materialization. An index key, which elements
    /// may hold outside the shape, and an arguments object, which maps its indices, give
    /// `None`, as does any other object: [[GetOwnProperty]] answers them.
    pub(super) fn shape_own_presence(&self, object: Value, atom: crate::bytecode::Atom) -> Option<bool> {
        if self.atom_class(atom).contains(super::object::AtomClass::ARRAY_INDEX)
            || self.heap.ordinary_object(object)?.is_arguments_object()
        {
            return None;
        }
        let data = self.shape_property_lookup(object, atom)?;
        Some(self.shape_slot(data.shape(), atom).is_some())
    }

    pub(super) fn instanceof(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        constructor: Value,
    ) -> Result<bool, JsError> {
        if let Some(result) = self.ordinary_instanceof(value, constructor) {
            return Ok(result);
        }
        let _stack = self.enter_stack()?;
        self.with_call_roots([value, constructor], |vm| {
            if !vm.is_object_like(constructor) {
                return Err(vm.type_error(p, "right-hand side of 'instanceof' is not an object".into()));
            }
            if let Some(has_instance) = vm.well_known_symbols.get(WellKnownSymbol::HasInstance) {
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

    /// `InstanceofOperator(value, constructor)` when it reduces to a prototype-chain walk:
    /// `constructor` is an unbound function without an own `@@hasInstance`, whose prototype
    /// holds the native `Function.prototype[@@hasInstance]` as data, whose own `prototype` is
    /// a data property holding an object, and `value`'s chain has no proxy. Every step is then
    /// an ordinary data read that runs no guest code and cannot throw; `None` leaves any other
    /// case, unobserved, to the general path.
    #[inline(always)]
    pub(super) fn ordinary_instanceof(
        &mut self,
        value: Value,
        constructor: Value,
    ) -> Option<bool> {
        match self.steady_instanceof(value, constructor) {
            Some(holds) => Some(holds),
            None => self.ordinary_instanceof_walk(value, constructor),
        }
    }

    /// `ordinary_instanceof`'s steady case, which calls nothing: the remembered constructor
    /// layout and `@@hasInstance` holder, and a primitive or a value whose chain is object
    /// records up to the prototype or `null`. `None` leaves the rest to
    /// `ordinary_instanceof_walk`.
    #[inline(always)]
    pub(super) fn steady_instanceof(&self, value: Value, constructor: Value) -> Option<bool> {
        let Some(FunctionRecord { kind, object, .. }) = self.heap.function(constructor) else {
            return None;
        };
        if matches!(kind, FunctionKind::Native(Native::FunctionBoundCall)) {
            return None;
        }
        let (shape, slot) = self.instanceof_constructor_layouts[instanceof_layout_index(object.shape())]?;
        if shape != object.shape() || object.proto_link() != self.instanceof_has_instance_link {
            return None;
        }
        // SAFETY: the memo names the slot of `prototype` in the shape it holds, which is
        // never a dictionary's, so the slot exists.
        let prototype = unsafe { self.heap.property_get_shaped(object, slot) }?;
        if prototype.is_deleted() {
            return None;
        }
        let target = ProtoLink::object_record(prototype.direct_object_index()?);
        let Some(record) = value.direct_object_index() else {
            // A primitive has no prototype chain to hold the prototype: a value outside the
            // heap, or a string. Any other value takes the walk, whose object test makes a
            // call this steady case leaves out.
            return (value.heap_index().is_none() || self.heap.is_string(value))
                .then_some(false);
        };
        // The walk compares encoded links; only a link outside the object records decodes.
        let mut object = self.heap.object_record_by_id(record);
        loop {
            let link = object.proto_link();
            if link == target {
                return Some(true);
            }
            if link == ProtoLink::NULL {
                return Some(false);
            }
            object = match link.object_record_id() {
                Some(id) => self.heap.object_record_by_id(id),
                None => self.heap.object_record(object.proto())?,
            };
        }
    }

    /// `ordinary_instanceof` in full: whatever its steady case leaves.
    #[inline(never)]
    fn ordinary_instanceof_walk(&mut self, value: Value, constructor: Value) -> Option<bool> {
        let Some(FunctionRecord { kind, object, .. }) = self.heap.function(constructor) else {
            return None;
        };
        if matches!(kind, FunctionKind::Native(Native::FunctionBoundCall)) {
            return None;
        }
        let (shape, holder) = (object.shape(), object.proto());
        // The layout memo answers the `prototype` read from the cell already in hand.
        let remembered = match self.instanceof_constructor_layouts[instanceof_layout_index(shape)] {
            Some((cached, slot)) if cached == shape => Some(self.heap.property_get(object, slot)),
            _ => None,
        };
        let prototype = match remembered {
            Some(prototype) => prototype?,
            None => {
                let slot = self.instanceof_constructor_slot(constructor)?;
                if !self.shape_is_dictionary(shape) {
                    self.instanceof_constructor_layouts[instanceof_layout_index(shape)] = Some((shape, slot));
                }
                self.heap.property_get(self.object_data(constructor)?, slot)?
            }
        };
        if !self.inherits_native_has_instance(holder) {
            return None;
        }
        if !self.is_object_like(value) {
            return Some(false);
        }
        if !self.is_object_like(prototype) {
            return None;
        }
        // One cell lookup per link: a proxy anywhere on the chain needs the full algorithm.
        let mut current = value;
        loop {
            let next = match self.heap.object_record(current) {
                Some(object) => object.proto(),
                None => match self.heap.get(current) {
                    Some(Cell::Proxy { .. }) => return None,
                    _ => self.heap.object(current).map_or(Value::NULL, |object| object.proto()),
                },
            };
            if next.is_null() {
                return Some(false);
            }
            if next == prototype {
                return Some(true);
            }
            current = next;
        }
    }

    /// `key in object` for a string key `plain_named_key` admits, along a chain of ordinary
    /// objects (`Heap::ordinary_object`) up to `null`: HasProperty, whether a shape along it
    /// names the key, data property or accessor alike, which runs no guest code. `None`
    /// leaves any other case (a key a side-table descriptor holds, a namespace, a proxy, an
    /// exotic object, a primitive operand) to the general path.
    pub(super) fn plain_in(&mut self, op: u32, key: Value, object: Value) -> Option<bool> {
        if op != oxc_ast::ast::BinaryOperator::In as u32 || !self.heap.is_string(key) {
            return None;
        }
        self.heap.ordinary_object(object)?;
        let atom = self.string_key_atom(key)?;
        if !self.plain_named_key(atom) {
            return None;
        }
        let property = PropertyKey::string(atom);
        let mut current = object;
        loop {
            let data = self.heap.ordinary_object(current)?;
            if data.is_module_namespace() || data.deferred_module().is_some() {
                return None;
            }
            if self.property_shape_slot(data.shape(), property).is_some() {
                return Some(true);
            }
            if data.has_side_descriptors() && self.descriptors.contains_key(&(current, property)) {
                return None;
            }
            current = data.proto();
            if current.is_null() {
                return Some(false);
            }
        }
    }

    /// `left instanceof right` when it reduces to a prototype-chain walk; `None` for any
    /// other operator or a case the full algorithm must decide.
    #[inline(always)]
    pub(super) fn instanceof_truthy(&mut self, op: u32, left: Value, right: Value) -> Option<bool> {
        if op != oxc_ast::ast::BinaryOperator::Instanceof as u32 {
            return None;
        }
        self.ordinary_instanceof(left, right)
    }

    /// The slot of `constructor`'s own data `prototype` when it has no own `@@hasInstance`.
    fn instanceof_constructor_slot(&mut self, constructor: Value) -> Option<usize> {
        let has_instance = PropertyKey::symbol(self.well_known_symbols.get(WellKnownSymbol::HasInstance)?);
        if self.property_attributes(constructor, has_instance).is_some() {
            return None;
        }
        let prototype = PropertyKey::string(self.prototype_atom());
        if self.property_attributes(constructor, prototype)?.accessor {
            return None;
        }
        Some(self.object_property_slot(constructor, prototype)?.1)
    }

    /// Whether `holder`'s `@@hasInstance` is the native `Function.prototype[@@hasInstance]`
    /// as a data property; remembers a holder whose property can never change.
    /// Whether the `@@hasInstance` a constructor with prototype `holder` inherits is the
    /// native one held as data (`verify_has_instance_holder`): the first object on the chain
    /// with an own `@@hasInstance` holds it, and no proxy precedes it. A subclass constructor
    /// inherits it through its parent constructors.
    fn inherits_native_has_instance(&mut self, holder: Value) -> bool {
        let Some(symbol) = self.well_known_symbols.get(WellKnownSymbol::HasInstance) else {
            return false;
        };
        let mut current = holder;
        loop {
            if current == self.instanceof_has_instance_holder {
                return true;
            }
            if matches!(self.heap.get(current), Some(Cell::Proxy { .. })) {
                return false;
            }
            if self
                .property_attributes(current, PropertyKey::symbol(symbol))
                .is_some()
            {
                return self.verify_has_instance_holder(current);
            }
            let Some(object) = self.heap.object(current) else {
                return false;
            };
            current = object.proto();
            if current.is_null() {
                return false;
            }
        }
    }

    fn verify_has_instance_holder(&mut self, holder: Value) -> bool {
        let Some(symbol) = self.well_known_symbols.get(WellKnownSymbol::HasInstance) else {
            return false;
        };
        let Some(attributes) = self.property_attributes(holder, PropertyKey::symbol(symbol))
        else {
            return false;
        };
        let native = !attributes.accessor
            && self.symbol_property(holder, symbol).is_some_and(|method| {
                matches!(
                    self.heap.function(method),
                    Some(FunctionRecord {
                        kind: FunctionKind::Native(Native::FunctionPrototypeHasInstance),
                        ..
                    })
                )
            });
        if native && !attributes.writable && !attributes.configurable {
            self.set_instanceof_has_instance_holder(holder);
        }
        native
    }

    pub(super) fn set_instanceof_has_instance_holder(&mut self, holder: Value) {
        self.instanceof_has_instance_holder = holder;
        self.instanceof_has_instance_link = ProtoLink::of(holder);
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
            let bound_env = match vm.heap.function(constructor) {
                Some(FunctionRecord {
                    kind: FunctionKind::Native(Native::FunctionBoundCall),
                    env,
                    ..
                }) => Some(*env),
                _ => None,
            };
            if let Some(env) = bound_env {
                let target_atom = vm.runtime_atoms.bound_target;
                let target = vm
                    .own_property(env, target_atom)
                    .unwrap_or(Value::UNDEFINED);
                return vm.instanceof(p, value, target);
            }
            if !vm.is_object_like(value) {
                return Ok(false);
            }
            let prototype_atom = vm.prototype_atom();
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
        (Some(Cell::BigInt(bigint)), None) if let Some(string) = heap.string(right) => Some(
            crate::bigint::parse_string(string.host_string())
                .map(|right| bigint.as_ref().cmp(&right)),
        ),
        (None, Some(Cell::BigInt(bigint))) if let Some(string) = heap.string(left) => Some(
            crate::bigint::parse_string(string.host_string())
                .map(|left| left.cmp(bigint)),
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
        (Some(Cell::BigInt(left)), Some(Cell::BigInt(right)), _, _) => Some(Some(left.cmp(right))),
        (Some(Cell::BigInt(bigint)), _, _, Some(number)) => {
            Some(bigint_number_ordering(bigint, number))
        }
        (_, Some(Cell::BigInt(bigint)), Some(number), _) => {
            Some(bigint_number_ordering(bigint, number).map(Ordering::reverse))
        }
        _ => None,
    }
}

fn bigint_number_ordering(integer: &BigInt, number: f64) -> Option<Ordering> {
    if number.is_nan() {
        return None;
    }
    if number == f64::INFINITY {
        return Some(Ordering::Less);
    }
    if number == f64::NEG_INFINITY {
        return Some(Ordering::Greater);
    }
    let sign = integer.sign();
    if number == 0.0 {
        return Some(integer.cmp(&BigInt::ZERO));
    }
    if sign == Sign::Minus && number.is_sign_positive() {
        return Some(Ordering::Less);
    }
    if sign != Sign::Minus && number.is_sign_negative() {
        return Some(Ordering::Greater);
    }
    let ordering = compare_positive_bigint_number(BigInt::from(integer.magnitude().clone()), number.abs());
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

/// The `Vm::instanceof_constructor_layouts` entry a constructor of `shape` takes.
#[inline(always)]
fn instanceof_layout_index(shape: u32) -> usize {
    shape as usize % INSTANCEOF_CONSTRUCTOR_LAYOUTS
}
