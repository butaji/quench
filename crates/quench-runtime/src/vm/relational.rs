use std::cmp::Ordering;
use crate::heap::ProtoLink;

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
                    let trap_atom = vm.trap_atom(ProxyTrap::Has);
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
                    vm.validate_proxy_property_absence(p, target, key, ProxyTrap::Has.name())?;
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
        let found = match self.proved_has(object, property) {
            Some(found) => found,
            None => self.plain_in_chain(object, property)?,
        };
        // The answer is a fact the native tiers take of the site (`in_presence`).
        self.remember_presence(object, atom, found);
        Some(found)
    }

    /// Whether a shape along the chain of `object`, ordinary objects up to `null`, names `property`
    /// (`plain_in`'s walk): `None` where an object on it is not ordinary or holds the key in a
    /// side table.
    fn plain_in_chain(&self, object: Value, property: PropertyKey) -> Option<bool> {
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

    /// `HasProperty` of `key` on the ordinary `object` from the fact table (K-ENTRY): any
    /// answer but an absent key is a present one, and a table miss proves the chain once
    /// (`Vm::prove_load`). `None` where neither answers.
    #[inline]
    pub(super) fn proved_has(&mut self, object: Value, key: PropertyKey) -> Option<bool> {
        use super::symbol_load_proof::SymbolLoadAnswer;
        let data = self.heap.ordinary_object(object)?;
        let (layout, side) = (data.layout_id(), data.has_side_descriptors());
        let epoch = crate::heap::prototype_layout_epoch();
        match self.symbol_load_proofs.answer(layout, key, epoch) {
            Some(SymbolLoadAnswer::Own(_)) => Some(true),
            Some(SymbolLoadAnswer::Absent) if !side => Some(false),
            Some(SymbolLoadAnswer::Inherited { .. } | SymbolLoadAnswer::Getter { .. }) if !side => Some(true),
            Some(_) => None,
            None => match self.prove_load(object, key, epoch) {
                super::object_get::ProvedLoad::Value(_) | super::object_get::ProvedLoad::Getter => Some(true),
                super::object_get::ProvedLoad::Absent => Some(false),
                super::object_get::ProvedLoad::Unproved => None,
            },
        }
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
                .map(|right| bigint.cmp(&crate::bigint::JsBigInt::from(&right))),
        ),
        (None, Some(Cell::BigInt(bigint))) if let Some(string) = heap.string(left) => Some(
            crate::bigint::parse_string(string.host_string())
                .map(|left| crate::bigint::JsBigInt::from(&left).cmp(bigint)),
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
            Some(bigint_number_ordering(&bigint.to_num(), number))
        }
        (_, Some(Cell::BigInt(bigint)), Some(number), _) => {
            Some(bigint_number_ordering(&bigint.to_num(), number).map(Ordering::reverse))
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
    let (significand, exponent) = crate::float_layout::binary64_parts(number);
    let significand = BigInt::from(significand);
    if exponent >= 0 {
        integer.cmp(&(significand << exponent as usize))
    } else {
        (integer << (-exponent) as usize).cmp(&significand)
    }
}

/// The `Vm::instanceof_constructor_layouts` entry a constructor of `shape` takes.
#[inline(always)]
pub(super) fn instanceof_layout_index(shape: u32) -> usize {
    shape as usize % INSTANCEOF_CONSTRUCTOR_LAYOUTS
}

/// The `Vm::instanceof_inheriting_holders` entry of `holder`, by its cell.
#[inline(always)]
fn instanceof_holder_index(holder: Value) -> usize {
    holder.heap_index().map_or(0, |index| index as usize % INSTANCEOF_CONSTRUCTOR_LAYOUTS)
}
