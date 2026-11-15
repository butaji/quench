//! `instanceof`: the operator's ordinary algorithm, the proofs that answer it without a walk
//! of the prototype chain, and the native `@@hasInstance` holders they rely on.

use super::*;

impl<H: Host> Vm<H> {
    pub(in crate::vm) fn instanceof(
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

    /// `ordinary_instanceof`'s steady case, which calls nothing: the remembered constructor
    /// layout and `@@hasInstance` holder, and a primitive or a value whose chain is object
    /// records up to the prototype or `null`. `None` leaves the rest to
    /// `ordinary_instanceof_walk`.
    #[inline(always)]
    pub(in crate::vm) fn steady_instanceof(&self, value: Value, constructor: Value) -> Option<bool> {
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
    pub(in crate::vm) fn primitive_instance(value: Value, heap: &crate::heap::Heap) -> bool {
        value.direct_object_index().is_none() && (value.heap_index().is_none() || heap.is_string(value))
    }

    /// The encoded link of `constructor`'s `prototype` object where the steady case holds:
    /// the remembered constructor layout, the native `@@hasInstance` inherited, and a
    /// `prototype` that is an object record.
    #[inline(always)]
    pub(in crate::vm) fn steady_instanceof_target(&self, constructor: Value) -> Option<ProtoLink> {
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
        Some(ProtoLink::object_record(prototype.direct_object_index()?))
    }

    /// `proved_instanceof` under a current proof of a primitive (`false`, as there) or a direct
    /// object whose chain is all object records: `None` for anything else (no proof, another
    /// heap value, a link outside the records), which `proved_instanceof` decides. Calls
    /// nothing, so a handler's hit path builds no frame.
    #[inline(always)]
    pub(in crate::vm) fn record_instanceof(&self, value: Value, constructor: Value) -> Option<bool> {
        let proof = &self.instanceof_proofs[instanceof_holder_index(constructor)];
        if proof.0 != constructor || !proof.2.as_ref().is_some_and(|watch| watch.is_published()) {
            return None;
        }
        let target = proof.1;
        let Some(record) = value.direct_object_index() else {
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
    /// (marked a prototype) changes, so a use reads neither its layout nor its `prototype`.
    #[inline(always)]
    pub(in crate::vm) fn proved_instanceof(&mut self, value: Value, constructor: Value) -> Option<bool> {
        let proof = &self.instanceof_proofs[instanceof_holder_index(constructor)];
        if proof.0 != constructor || !proof.2.as_ref().is_some_and(|watch| watch.is_published()) {
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
    pub(in crate::vm) fn prove_instanceof(&mut self, value: Value, constructor: Value) -> Option<bool> {
        let holds = self.steady_instanceof(value, constructor)?;
        if let Some(target) = self.steady_instanceof_target(constructor)
            && let Some(shape) = self.heap.function(constructor).map(|function| function.object.exact_shape())
        {
            self.heap.mark_prototype(constructor);
            let watch = crate::code_dependencies::new_dependency();
            let code: std::rc::Rc<dyn crate::code_dependencies::DependentCode> = watch.clone();
            // The mark may have changed the exact shape the subscription names.
            let shape = self.heap.function(constructor).map_or(shape, |function| function.object.exact_shape());
            if crate::heap::subscribe_prototype_values(shape, &code) {
                self.instanceof_proofs[instanceof_holder_index(constructor)] = (constructor, target, Some(watch));
            }
        }
        Some(holds)
    }

    /// The object of `value`, a heap object other than a proxy, whose prototype link
    /// `[[GetPrototypeOf]]` reads without running guest code.
    #[inline(always)]
    pub(in crate::vm) fn ordinary_link_holder(&self, value: Value) -> Option<&crate::heap::Object> {
        if matches!(self.heap.get(value), Some(Cell::Proxy { .. })) {
            return None;
        }
        self.heap.object(value)
    }

    /// `ordinary_instanceof` in full: whatever its steady case leaves.
    #[inline(never)]
    pub(in crate::vm) fn ordinary_instanceof_walk(&mut self, value: Value, constructor: Value) -> Option<bool> {
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

    /// `left instanceof right` when it reduces to a prototype-chain walk; `None` for any
    /// other operator or a case the full algorithm must decide.
    #[inline(always)]
    pub(in crate::vm) fn instanceof_truthy(&mut self, op: u32, left: Value, right: Value) -> Option<bool> {
        if op != oxc_ast::ast::BinaryOperator::Instanceof as u32 {
            return None;
        }
        self.ordinary_instanceof(left, right)
    }

    /// The slot of `constructor`'s own data `prototype` when it has no own `@@hasInstance`.
    pub(in crate::vm) fn instanceof_constructor_slot(&mut self, constructor: Value) -> Option<usize> {
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
    pub(in crate::vm) fn inherits_native_has_instance(&mut self, holder: Value) -> bool {
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
            let Some(object) = self.heap.object(current) else {
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
    pub(in crate::vm) fn remembers_inheriting_holder(&self, holder: Value) -> bool {
        self.instanceof_inheriting_holders[instanceof_holder_index(holder)]
            == Some((holder, crate::heap::prototype_layout_epoch()))
    }

    /// Records that `holder` inherits the remembered native `@@hasInstance` holder, after
    /// marking each link up to it a prototype so a change to any retires the record.
    pub(in crate::vm) fn remember_inheriting_holder(&mut self, holder: Value) {
        let mut current = holder;
        while current != self.instanceof_has_instance_holder {
            self.heap.mark_prototype(current);
            let Some(object) = self.heap.object(current) else {
                return;
            };
            current = object.proto();
        }
        self.instanceof_inheriting_holders[instanceof_holder_index(holder)] =
            Some((holder, crate::heap::prototype_layout_epoch()));
    }

    pub(in crate::vm) fn verify_has_instance_holder(&mut self, holder: Value) -> bool {
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

    pub(in crate::vm) fn set_instanceof_has_instance_holder(&mut self, holder: Value) {
        self.instanceof_has_instance_holder = holder;
        self.instanceof_has_instance_link = ProtoLink::of(holder);
    }

    pub(in crate::vm) fn ordinary_has_instance(
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
