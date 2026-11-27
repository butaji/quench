use std::cmp::Ordering;
use crate::heap::ProtoLink;

impl Vm {
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
                    return Ok(operator.matches(left.order(&right)));
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

    pub fn has_property(
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
    pub fn ordinary_instanceof(
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
    pub fn steady_instanceof(&self, value: Value, constructor: Value) -> Option<bool> {
        // A primitive is no instance, decided before `prototype` is read
        // (OrdinaryHasInstance step 3): a value outside the heap, or a string.
        if Self::primitive_instance(value, &self.heap) {
            self.steady_instanceof_target(constructor)?;
            return Some(false);
        }
        let target = self.steady_instanceof_target(constructor)?;
        self.instanceof_chain(value, target)
    }

    /// Whether `value` is a primitive for `instanceof`: outside the heap, or a string.
    #[inline(always)]
    fn primitive_instance(value: Value, heap: &crate::heap::Heap) -> bool {
        value.direct_object_index().is_none() && (value.heap_index().is_none() || heap.is_string(value))
    }

    /// The encoded link of `constructor`'s `prototype` object where the steady case holds:
    /// the remembered constructor layout, the native `@@hasInstance` inherited, and a
    /// `prototype` that is an object record.
    #[inline(always)]
    fn steady_instanceof_target(&self, constructor: Value) -> Option<ProtoLink> {
        let FunctionRecord { kind, object, .. } = self.heap.function(constructor)?;
        if matches!(kind, FunctionKind::Native(Native::FunctionBoundCall)) {
            return None;
        }
        let (shape, slot) = self.instanceof_constructor_layouts[instanceof_layout_index(object.shape())]?;
        if shape != object.shape()
            || object.proto_link() != self.instanceof_has_instance_link && !self.remembers_inheriting_holder(object.proto())
        {
            return None;
        }
        // SAFETY: the memo names the slot of `prototype` in the shape it holds, which is
        // never a dictionary's, so the slot exists.
        let prototype = unsafe { self.heap.property_get_shaped(object, slot) }?;
        if prototype.is_deleted() {
            return None;
        }
        ProtoLink::object_record(self.heap.record_id(prototype.direct_object_index()?))
    }

    /// `proved_instanceof` under a current proof of a primitive (`false`, as there) or a direct
    /// object whose chain is all object records: `None` for anything else (no proof, another
    /// heap value, a link outside the records), which `proved_instanceof` decides. Calls
    /// nothing, so a handler's hit path builds no frame.
    #[inline(always)]
    pub fn record_instanceof(&self, value: Value, constructor: Value) -> Option<bool> {
        let proof = &self.instanceof_proofs[instanceof_holder_index(constructor)];
        if proof.0 != constructor || !proof.2.as_ref().is_some_and(|(watch, cell)| watch.is_published() && cell.holds()) {
            return None;
        }
        let target = proof.1;
        let Some(record) = value.direct_object_index().map(|r| self.heap.record_id(r)) else {
            return (!value.is_heap() || value.direct_string_index().is_some()).then_some(false);
        };
        let mut object = self.heap.object_record_by_id(record);
        loop {
            let link = object.proto_link();
            if link == target {
                return Some(true);
            }
            if link == ProtoLink::NULL {
                return Some(false);
            }
            object = self.heap.object_record_by_id(link.object_record_id()?);
        }
    }

    /// `steady_instanceof` through the proof `constructor` holds (`Vm::instanceof_proofs`):
    /// its `prototype` link, read once and kept while no slot or layout of the constructor
    /// (marked a prototype) changes, nor the chain it inherits `@@hasInstance` through, so a
    /// use reads neither its layout nor its `prototype`.
    #[inline(always)]
    pub fn proved_instanceof(&mut self, value: Value, constructor: Value) -> Option<bool> {
        let proof = &self.instanceof_proofs[instanceof_holder_index(constructor)];
        if proof.0 != constructor || !proof.2.as_ref().is_some_and(|(watch, cell)| watch.is_published() && cell.holds()) {
            return self.prove_instanceof(value, constructor);
        }
        let target = proof.1;
        if Self::primitive_instance(value, &self.heap) {
            return Some(false);
        }
        self.instanceof_chain(value, target)
    }

    /// `proved_instanceof` without a current proof: the steady case, which then records one.
    #[inline(never)]
    fn prove_instanceof(&mut self, value: Value, constructor: Value) -> Option<bool> {
        let holds = self.steady_instanceof(value, constructor)?;
        if let Some(target) = self.steady_instanceof_target(constructor)
            && let Some((shape, link)) = self.heap.function(constructor).map(|function| (function.object.exact_shape(), function.object.proto_link()))
            && let Some(chain) = self.inheriting_cell(link)
        {
            self.heap.mark_prototype(constructor);
            let watch = crate::code_dependencies::new_dependency();
            let code: std::rc::Rc<dyn crate::code_dependencies::DependentCode> = watch.clone();
            // The mark may have changed the exact shape the subscription names.
            let shape = self.heap.function(constructor).map_or(shape, |function| function.object.exact_shape());
            // The constructor's own values and layout retire the watch; the chain it inherits
            // `@@hasInstance` through retires the cell. Another prototype's change leaves both.
            if crate::heap::hold_prototype_values(shape, &code) {
                self.instanceof_proofs[instanceof_holder_index(constructor)] = (constructor, target, Some((watch, chain)));
            }
        }
        Some(holds)
    }

    /// The cell under which a constructor whose prototype `link` names inherits the native
    /// `@@hasInstance`: of no chain where the link is the native holder's, else the remembered
    /// holder's (`remember_inheriting_holder`), where it still holds.
    fn inheriting_cell(&mut self, link: ProtoLink) -> Option<crate::heap::ChainCell> {
        if link == self.instanceof_has_instance_link {
            return self.chain_cell(Value::NULL, 0);
        }
        let holder = link.prototype();
        self.instanceof_inheriting_holders[instanceof_holder_index(holder)]
            .filter(|(held, cell)| *held == holder && cell.holds())
            .map(|(_, cell)| cell)
    }

    /// The object of `value`, a heap object other than a proxy, whose prototype link
    /// `[[GetPrototypeOf]]` reads without running guest code.
    #[inline(always)]
    pub(super) fn ordinary_link_holder(&self, value: Value) -> Option<&crate::heap::Object> {
        if matches!(self.heap.get(value), Some(Cell::Proxy { .. })) {
            return None;
        }
        self.heap.object_ref(value).map(|object| object.record())
    }

    /// `ordinary_instanceof` in full: whatever its steady case leaves.
    #[inline(never)]
    fn ordinary_instanceof_walk(&mut self, value: Value, constructor: Value) -> Option<bool> {
        let FunctionRecord { kind, object, .. } = self.heap.function(constructor)?;
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
                    _ => self.heap.object_ref(current).map_or(Value::NULL, |object| object.proto()),
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
    pub fn plain_in(&mut self, op: u32, key: Value, object: Value) -> Option<bool> {
        if op != crate::operator::BinaryOperator::In as u32 || !self.heap.is_string(key) {
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
        match self.symbol_load_proofs.answer(layout, key) {
            Some(SymbolLoadAnswer::Own(_)) => Some(true),
            Some(SymbolLoadAnswer::Absent) if !side => Some(false),
            Some(SymbolLoadAnswer::Inherited { .. } | SymbolLoadAnswer::Getter { .. }) if !side => Some(true),
            Some(_) => None,
            None => match self.prove_load(object, key) {
                super::object_get::ProvedLoad::Value(_) | super::object_get::ProvedLoad::Getter(_) => Some(true),
                super::object_get::ProvedLoad::Absent => Some(false),
                super::object_get::ProvedLoad::Unproved => None,
            },
        }
    }

    /// `left instanceof right` when it reduces to a prototype-chain walk; `None` for any
    /// other operator or a case the full algorithm must decide.
    #[inline(always)]
    pub fn instanceof_truthy(&mut self, op: u32, left: Value, right: Value) -> Option<bool> {
        if op != crate::operator::BinaryOperator::Instanceof as u32 {
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
        if self.remembers_inheriting_holder(holder) {
            return true;
        }
        let Some(symbol) = self.well_known_symbols.get(WellKnownSymbol::HasInstance) else {
            return false;
        };
        let mut current = holder;
        loop {
            if current == self.instanceof_has_instance_holder {
                self.remember_inheriting_holder(holder);
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
            let Some(object) = self.heap.object_ref(current) else {
                return false;
            };
            current = object.proto();
            if current.is_null() {
                return false;
            }
        }
    }

    /// Whether `holder` holds a current proof that it inherits the native `@@hasInstance`
    /// (`Vm::instanceof_inheriting_holders`).
    #[inline(always)]
    pub(super) fn remembers_inheriting_holder(&self, holder: Value) -> bool {
        self.instanceof_inheriting_holders[instanceof_holder_index(holder)]
            .is_some_and(|(held, cell)| held == holder && cell.holds())
    }

    /// Records that `holder` inherits the remembered native `@@hasInstance` holder, under the
    /// cell of the links up to it, each marked a prototype first, so a change to any retires
    /// the record (`Vm::chain_cell`). The native holder's own `@@hasInstance` is read-only and
    /// non-configurable, so it is no link of the cell.
    fn remember_inheriting_holder(&mut self, holder: Value) {
        let mut current = holder;
        let mut depth = 0u8;
        while current != self.instanceof_has_instance_holder {
            let Some(object) = self.heap.object_ref(current) else {
                return;
            };
            current = object.proto();
            let Some(deeper) = depth.checked_add(1) else {
                return;
            };
            depth = deeper;
        }
        if let Some(cell) = self.chain_cell(holder, depth) {
            self.instanceof_inheriting_holders[instanceof_holder_index(holder)] = Some((holder, cell));
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

    pub fn ordinary_has_instance(
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
                .map(|right| bigint.cmp(&crate::bigint::JsBigInt::from(&right))),
        ),
        (None, Some(Cell::BigInt(bigint))) if let Some(string) = heap.string(left) => Some(
            crate::bigint::parse_string(string.host_string())
                .map(|left| crate::bigint::JsBigInt::from(&left).cmp(bigint)),
        ),
        _ => None,
    }
}

/// The ordering of two BigInt values, or of a BigInt and a Number (`None` for a NaN
/// Number); `None` outside those pairs. Shared by the relational operators and by
/// `bigint_relation`, the fast path of the operators in `dispatch_fast`.
pub(super) fn compare_bigint_values(heap: &Heap, left: Value, right: Value) -> Option<Option<Ordering>> {
    match (
        heap.get(left),
        heap.get(right),
        left.as_number(),
        right.as_number(),
    ) {
        (Some(Cell::BigInt(left)), Some(Cell::BigInt(right)), _, _) => Some(Some(left.cmp(right))),
        (Some(Cell::BigInt(bigint)), _, _, Some(number)) => {
            Some(crate::staged::bigint_small::number_ordering(bigint, number))
        }
        (_, Some(Cell::BigInt(bigint)), Some(number), _) => {
            Some(crate::staged::bigint_small::number_ordering(bigint, number).map(Ordering::reverse))
        }
        _ => None,
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
