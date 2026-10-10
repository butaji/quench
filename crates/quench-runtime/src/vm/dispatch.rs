use super::*;
use crate::heap::PrivateBrand;

// Publish the current resume PC only while a binding-site consumer runs.
macro_rules! with_binding_site_pc {
    ($vm:expr, $frame:expr, $resume_pc:expr, $action:expr) => {{
        let previous = std::mem::replace(
            &mut $vm.frames[$frame].binding_site_pc,
            Some($resume_pc as u32),
        );
        let result = $action;
        $vm.frames[$frame].binding_site_pc = previous;
        result
    }};
}

impl<H: Host> Vm<H> {
    pub(super) fn module_import_value(
        &self,
        program: ProgramId,
        function: u32,
        slot: usize,
    ) -> Option<Value> {
        if function != super::ROOT_FUNCTION_ID {
            return None;
        }
        let slot = u16::try_from(slot).ok()?;
        match self.programs.module_import(program, slot)? {
            ModuleImport::Value(value) => Some(value),
            ModuleImport::Binding(program, binding, fallback) => {
                let Some(environment) = self.programs.module_environment(program) else {
                    return Some(fallback);
                };
                let value = self
                    .heap
                    .environment_slot(environment, usize::from(binding));
                Some(match value {
                    Some(Value::DELETED) if !fallback.is_undefined() => fallback,
                    None => fallback,
                    Some(value) => value,
                })
            }
        }
    }

    #[inline(always)]
    pub(super) fn step(
        &mut self,
        p: &ResidualProgram,
        f: usize,
        i: WideInstruction,
        pc: &mut usize,
        allow_inline_calls: bool,
    ) -> Result<StepResult, JsError> {
        match i.op() {
            Op::Nop => {}
            Op::CloneEnv => {
                self.clone_frame_environment(
                    f,
                    &p.functions[self.frames[f].function as usize].environment_clones
                        [i.environment_clone_index()],
                )?;
            }
            Op::Wide => unreachable!("validated dispatch cannot contain nested wide instruction"),
            Op::LoadConst => {
                let value = self
                    .programs
                    .constant(self.frames[f].program, i.constant_index())
                    .ok_or_else(|| {
                        JsError::validation("constant index is outside program".into())
                    })?;
                self.write(f, i.result_register(), value);
            }
            Op::CreateRegExpLiteral => {
                let value = self.regexp_literal(p, f, i.regexp_literal_site_index())?;
                self.write(f, i.result_register(), value);
            }
            Op::LoadLocalPlain => {
                // SAFETY: validated bytecode bounds the slot by Function.locals,
                // and frame setup sizes locals to that count.
                let value = unsafe { self.read_validated_local(f, i.local_slot()) };
                self.write(f, i.result_register(), value);
            }
            Op::LoadLocal | Op::LoadEnvLocal => {
                let value = self.load_local_binding(p, f, i.local_slot(), None)?;
                self.write(f, i.result_register(), value);
            }
            Op::StoreVarBinding => {
                self.store_var_binding(p, f, i.local_slot(), self.read(f, i.register_a()))?;
            }
            Op::StoreLocal => {
                let value = self.read(f, i.register_a());
                let slot = i.local_slot();
                let function = &p.functions[self.frames[f].function as usize];
                if function.is_self_binding_slot(slot) {
                    if function.strict {
                        return Err(
                            self.type_error(p, "assignment to function name binding".into())
                        );
                    }
                    if let Some(register) = i.optional_register_b() {
                        self.write(f, register, value);
                    }
                    return Ok(StepResult::Continue);
                }
                self.check_local_assignment_initialized(
                    p,
                    f,
                    slot,
                    i.boolean_field(crate::bytecode::InstructionField::C)
                        .unwrap_or(false),
                )?;
                if self.frames[f].function == 0 {
                    if function
                        .local_atoms
                        .get(slot)
                        .is_some_and(|atom| function.global_immutable_atoms.contains(atom))
                        && i.boolean_field(crate::bytecode::InstructionField::C) == Some(false)
                    {
                        return Err(self.type_error(p, "assignment to constant binding".into()));
                    }
                }
                if self.local_slot_is_environment_owned(p, f, slot) {
                    *self
                        .heap
                        .environment_slot_mut(self.frames[f].env, slot)
                        .ok_or_else(|| JsError("invalid local slot".into()))? = value;
                } else {
                    self.frames[f].locals[slot] = value;
                }
                self.mirror_global_lexical_binding(p, f, slot, value);
                self.mapped_argument_store(p, f, slot, value);
                if let Some(register) = i.optional_register_b() {
                    self.write(f, register, value);
                }
            }
            Op::StoreLocalPlain => {
                let value = self.read(f, i.register_a());
                // SAFETY: validated bytecode bounds the slot by Function.locals,
                // and frame setup sizes locals to that count.
                unsafe { self.write_validated_local(f, i.local_slot(), value) };
                if let Some(register) = i.optional_register_b() {
                    self.write(f, register, value);
                }
            }
            Op::StoreEnvLocal => {
                let value = self.read(f, i.register_a());
                let slot = i.local_slot();
                let function = &p.functions[self.frames[f].function as usize];
                let atom = function.local_atoms.get(slot).copied();
                if let Some(atom) = atom
                    && self.store_with_binding(p, f, atom, value, function.strict)?
                {
                    if let Some(register) = i.optional_register_b() {
                        self.write(f, register, value);
                    }
                    return Ok(StepResult::Continue);
                }
                self.check_local_assignment_initialized(
                    p,
                    f,
                    slot,
                    i.boolean_field(crate::bytecode::InstructionField::C)
                        .unwrap_or(false),
                )?;
                if let Some(atom) = atom
                    && function.global_immutable_atoms.contains(&atom)
                    && !i
                        .boolean_field(crate::bytecode::InstructionField::C)
                        .unwrap_or(false)
                {
                    return Err(self.type_error(p, "assignment to constant binding".into()));
                }
                if function.is_self_binding_slot(slot) {
                    if function.strict {
                        return Err(
                            self.type_error(p, "assignment to function name binding".into())
                        );
                    }
                    return Ok(StepResult::Continue);
                }
                let global_var = self.root_global_var_atom(
                    p,
                    self.frames[f].program,
                    self.frames[f].function,
                    slot,
                );
                if let Some(atom) = global_var {
                    let written = self.set_property_with_receiver(
                        p,
                        self.realm.globals,
                        atom,
                        value,
                        self.realm.globals,
                    )?;
                    if !written && p.functions[self.frames[f].function as usize].strict {
                        return Err(
                            self.type_error(p, "cannot assign to read-only global binding".into())
                        );
                    }
                    if let Some(register) = i.optional_register_b() {
                        self.write(f, register, value);
                    }
                    return Ok(StepResult::Continue);
                }
                if self.eval_script_context
                    && let Some(atom) =
                        self.root_global_lexical_atom(p, self.frames[f].function, slot)
                {
                    self.realm.global_lexical_bindings.insert(atom, value);
                }
                if self.local_slot_is_environment_owned(p, f, slot) {
                    *self
                        .heap
                        .environment_slot_mut(self.frames[f].env, slot)
                        .ok_or_else(|| JsError("invalid local environment".into()))? = value;
                } else {
                    self.frames[f].locals[slot] = value;
                }
                self.mapped_argument_store(p, f, slot, value);
                if let Some(register) = i.optional_register_b() {
                    self.write(f, register, value);
                }
            }
            Op::LoadCapture => {
                let v = self.capture(p, f, i.capture_depth(), i.capture_slot())?;
                self.write(f, i.result_register(), v);
            }
            Op::StoreCapture => self.store_capture(
                p,
                f,
                i.capture_depth(),
                i.capture_slot(),
                self.read(f, i.register_a()),
            )?,
            Op::LoadName => {
                let v = with_binding_site_pc!(
                    self,
                    f,
                    *pc,
                    self.load_name(p, i.atom_index(), Some(i.cache_site_index()))
                )?;
                self.write(f, i.result_register(), v);
            }
            Op::LoadNameCall => {
                let (callee, this) = with_binding_site_pc!(
                    self,
                    f,
                    *pc,
                    self.load_name_call(p, i.atom_index(), Some(i.cache_site_index()), false)
                )?;
                self.write(f, i.result_register(), callee);
                self.write(f, i.register_b(), this);
            }
            Op::LoadNameTypeof => {
                let v = with_binding_site_pc!(
                    self,
                    f,
                    *pc,
                    self.load_name_typeof(p, i.atom_index(), i.cache_site_index())
                )?;
                self.write(f, i.result_register(), v);
            }
            Op::StoreName => {
                let value = self.read(f, i.register_a());
                with_binding_site_pc!(
                    self,
                    f,
                    *pc,
                    self.store_name(
                        p,
                        i.atom_index(),
                        value,
                        i.cache_site_index(),
                        i.boolean_field(crate::bytecode::InstructionField::B)
                            .expect("validated initialization flag"),
                    )
                )?;
            }
            Op::LoadThis => {
                let this = self.checked_this_binding(p, f)?;
                self.write(f, i.result_register(), this);
            }
            Op::LoadImportMeta => {
                let program = self.frames[f].program;
                let import_meta = if let Some(value) = self.programs.import_meta(program) {
                    value
                } else {
                    let value = self
                        .heap
                        .alloc(Cell::Object(Self::empty_object(Value::NULL)));
                    self.programs.set_import_meta(program, value);
                    value
                };
                self.write(f, i.result_register(), import_meta);
            }
            Op::InitializeThis => {
                let value = self.read(f, i.register_a());
                self.initialize_this_binding(p, f, value)?;
            }
            Op::CacheTemplateObject => {
                let key = (
                    self.frames[f].program,
                    self.frames[f].function,
                    i.template_site_index(),
                );
                let value = if let Some(value) = self.realm.template_objects.get(&key).copied() {
                    value
                } else {
                    let value = self.read(f, i.register_a());
                    self.realm.template_objects.insert(key, value);
                    value
                };
                self.write(f, i.register_a(), value);
            }
            Op::LoadCachedTemplateObject => {
                let key = (
                    self.frames[f].program,
                    self.frames[f].function,
                    i.template_site_index(),
                );
                let value = self
                    .realm
                    .template_objects
                    .get(&key)
                    .copied()
                    .unwrap_or(Value::UNDEFINED);
                self.write(f, i.result_register(), value);
            }
            Op::MakeClosure => {
                let env = with_binding_site_pc!(self, f, *pc, self.capture_binding_environment(f))?;
                let module_root = p.is_module()
                    && self.frames[f].function == super::ROOT_FUNCTION_ID
                    && self.programs.module_environment(self.frames[f].program) == Some(env);
                let v = if module_root {
                    let cached = self
                        .cached_functions_in_environment(
                            self.frames[f].program,
                            i.closure_function_index(),
                            env,
                        )
                        .next();
                    match cached {
                        Some(function) => function,
                        None => self.closure(p, i.closure_function_index(), env)?,
                    }
                } else {
                    self.closure(p, i.closure_function_index(), env)?
                };
                self.write(f, i.result_register(), v);
            }
            Op::MakeObject => {
                let v = self.object();
                self.write(f, i.result_register(), v);
            }
            Op::MakeObject2 => {
                let v = self.object_pair(
                    p,
                    i.object_site_index(),
                    self.read(f, i.register_b()),
                    self.read(f, i.register_c()),
                );
                if i.returns_from_frame() {
                    return Ok(StepResult::Return(v));
                }
                self.write(f, i.result_register(), v);
            }
            Op::SuperConstArrayObject2 => {
                if let Some(value) = self.execute_const_array_object2(p, f, i)? {
                    return Ok(StepResult::Return(value));
                }
            }
            Op::MakeArray => {
                let v = self.heap.alloc(Cell::Array {
                    object: Self::empty_object(self.array_prototype_for_realm(self.realm.globals)),
                    elements: Rc::new(vec![Value::DELETED; i.array_length()]),
                });
                self.write(f, i.result_register(), v);
            }
            Op::MakeConstArray => {
                let start = i.constant_index();
                let end = start + i.element_count() as usize;
                let elements = self
                    .programs
                    .const_array(self.frames[f].program, start, end - start)
                    .ok_or_else(|| {
                        JsError::validation("constant array is outside program".into())
                    })?;
                let v = self.heap.alloc(Cell::Array {
                    object: Self::empty_object(self.array_prototype_for_realm(self.realm.globals)),
                    elements,
                });
                self.write(f, i.result_register(), v);
            }
            Op::GetField => {
                let lookup = i.field_lookup().ok_or_else(|| {
                    JsError::validation("invalid nested field lookup encoding".into())
                })?;
                let v = match lookup {
                    crate::bytecode::FieldLookup::Site(site) => self.resolve_field(p, f, site)?,
                    crate::bytecode::FieldLookup::Atom {
                        atom,
                        base,
                        cache_site,
                    } => {
                        let base = self.resolve_field_base(p, f, base)?;
                        self.get_field_cached(p, base, atom, cache_site)?
                    }
                };
                if i.writes_current_this() {
                    let crate::bytecode::FieldLookup::Site(site) = lookup else {
                        unreachable!("field sink is only attached to nested field lookup")
                    };
                    let sink = p.field_sites[site].sink.expect("fused field sink");
                    self.set_field_cached(
                        p,
                        self.frames[f].this,
                        sink.0,
                        v,
                        sink.1,
                        p.functions[self.frames[f].function as usize].strict,
                    )?;
                }
                if i.returns_from_frame() {
                    return Ok(StepResult::Return(v));
                }
                self.write(f, i.result_register(), v);
            }
            Op::GetIndex => {
                #[cfg(feature = "profile-aggregate")]
                self.profile.index_dispatch(false, false);
                let base = self.resolve_operand(p, f, i.operand_b())?;
                let key = self.resolve_operand(p, f, i.operand_c())?;
                let v = self.get_index(p, base, key)?;
                self.write(f, i.result_register(), v);
            }
            Op::CheckPrivate => {
                let object = self.read(f, i.register_a());
                let atom = i.atom_index();
                self.check_private_brand(p, object, atom)?;
                if self.own_property(object, atom).is_none()
                    && self.property_accessor(object, atom).is_none()
                {
                    return Err(
                        self.type_error(p, "private member is not present on this object".into())
                    );
                }
            }
            Op::PrivateIn => {
                let object = self.read(f, i.register_b());
                if !self.is_object_like(object) {
                    return Err(
                        self.type_error(p, "right-hand side of 'in' is not an object".into())
                    );
                }
                let result = self.has_private_brand(p, object, i.atom_index());
                self.write(
                    f,
                    i.result_register(),
                    if result { Value::TRUE } else { Value::FALSE },
                );
            }
            Op::MarkPrivateName => {
                let object = self.read(f, i.register_b());
                let home = self.read(f, i.register_c());
                let atom = i.atom_index();
                let brand = PrivateBrand { home, name: atom };
                if object != home {
                    let extensible = self.object_data(object).is_some_and(Object::is_extensible);
                    let already_branded = self
                        .object_data(object)
                        .is_some_and(|object| object.has_private_name(brand));
                    if !extensible {
                        return Err(self.type_error(
                            p,
                            "Cannot add private field to a non-extensible object".into(),
                        ));
                    }
                    if already_branded {
                        return Err(self.type_error(
                            p,
                            "Cannot add private method to an object that already has it".into(),
                        ));
                    }
                }
                if let Some(object) = self.object_data_mut(object) {
                    object.add_private_name(brand);
                }
            }
            Op::ResolveName => {
                let strict = i
                    .boolean_field(crate::bytecode::InstructionField::B)
                    .ok_or_else(|| JsError::validation("invalid resolve-name flag".into()))?;
                let value = with_binding_site_pc!(
                    self,
                    f,
                    *pc,
                    self.resolve_name(p, i.atom_index(), strict)
                )?;
                self.write(f, i.result_register(), value);
            }
            Op::LoadResolvedName => {
                let strict = i
                    .boolean_field(crate::bytecode::InstructionField::C)
                    .ok_or_else(|| JsError::validation("invalid resolved-name flag".into()))?;
                let object = self.read(f, i.register_b());
                let value = self.load_resolved_name(p, object, i.atom_index(), strict)?;
                self.write(f, i.result_register(), value);
            }
            Op::ValidateClassHeritage => {
                let heritage = self.read(f, i.register_a());
                if !heritage.is_null() && !self.is_constructable(p, heritage) {
                    return Err(self
                        .type_error(p, "Class extends value is not a constructor or null".into()));
                }
            }
            Op::DeleteName => {
                let value =
                    with_binding_site_pc!(self, f, *pc, self.delete_name(p, i.atom_index()))?;
                self.write(f, i.result_register(), value);
            }
            Op::StoreResolvedName => {
                let strict = i
                    .boolean_field(crate::bytecode::InstructionField::C)
                    .ok_or_else(|| JsError::validation("invalid resolved-name flag".into()))?;
                let object = self.read(f, i.register_b());
                let atom = i.atom_index();
                self.store_resolved_name(p, object, atom, self.read(f, i.register_a()), strict)?;
            }
            Op::ToPropertyKey => {
                let value = self.to_property_key(p, self.read(f, i.register_b()))?;
                self.write(f, i.result_register(), value);
            }
            Op::ToNumeric => {
                let value = self.to_numeric(p, self.read(f, i.register_b()))?;
                self.write(f, i.result_register(), value);
            }
            Op::CopyDataProperties => self.copy_data_properties(
                p,
                self.read(f, i.register_a()),
                self.read(f, i.register_b()),
                self.read(f, i.register_c()),
            )?,
            Op::GetIterator => {
                let value = self.get_iterator(p, self.read(f, i.register_b()))?;
                self.write(f, i.result_register(), value);
            }
            Op::SpreadToArray => {
                let array = self.spread_to_array(p, self.read(f, i.register_b()))?;
                self.write(f, i.result_register(), array);
            }
            Op::RequireObjectCoercible => {
                self.require_object_coercible(p, self.read(f, i.register_b()))?;
            }
            Op::RequireIteratorResult => {
                if !self.is_object_like(self.read(f, i.register_b())) {
                    return Err(self.type_error(p, "iterator next result is not an object".into()));
                }
            }
            Op::SuperCallCheck => self.check_super_call(p)?,
            Op::IteratorClose => {
                self.iterator_close(p, self.read(f, i.register_b()))?;
            }
            Op::IteratorCleanupPush => self.frames[f].active_iterators.push(ActiveIterator {
                iterator: i.register_a(),
                done: i.register_b(),
            }),
            Op::IteratorCleanupPop => {
                self.frames[f]
                    .active_iterators
                    .pop()
                    .ok_or_else(|| JsError("iterator cleanup stack underflow".into()))?;
            }
            Op::SetFunctionName => {
                self.set_function_name(p, self.read(f, i.register_a()), i.atom_index())?;
            }
            Op::SetFunctionNameKey => {
                self.set_function_name_key(
                    self.read(f, i.register_a()),
                    self.read(f, i.register_b()),
                    i.function_name_prefix(),
                );
            }
            Op::InitializeTdz => {
                let slot = i.local_slot();
                if self.local_slot_is_environment_owned(p, f, slot) {
                    *self
                        .heap
                        .environment_slot_mut(self.frames[f].env, slot)
                        .ok_or_else(|| JsError("invalid local slot".into()))? = Value::DELETED;
                } else {
                    self.frames[f].locals[slot] = Value::DELETED;
                }
            }
            Op::GetAsyncIterator => {
                let value = self.get_async_iterator(p, self.read(f, i.register_b()))?;
                self.write(f, i.result_register(), value);
            }
            Op::Await => {
                // Promise conversion is part of this instruction, so abrupt
                // completion reaches the active frame's catch/finally handlers.
                let value = self.promise_for_value(p, self.read(f, i.register_b()))?;
                return Ok(StepResult::Await {
                    value,
                    destination: i.result_register(),
                });
            }
            Op::Yield => {
                let value = self.read(f, i.register_b());
                let value = if p.functions[self.frames[f].function as usize].is_async {
                    self.promise_for_value(p, value)?
                } else {
                    value
                };
                return Ok(StepResult::Yield {
                    value,
                    destination: i.result_register(),
                    delegated_result: None,
                });
            }
            Op::YieldStar => {
                let iterator = self.read(f, i.register_c());
                let iterator = if iterator.is_undefined() {
                    let source = self.read(f, i.register_b());
                    let asynchronous = p.functions[self.frames[f].function as usize].is_async;
                    let iterator = if asynchronous {
                        self.get_async_iterator(p, source)?
                    } else {
                        self.get_iterator(p, source)?
                    };
                    self.write(f, i.register_c(), iterator);
                    iterator
                } else {
                    iterator
                };
                let (state_register, next_method_register) = i.register_pair();
                let next_method = self.read(f, next_method_register);
                let next_method = if next_method.is_undefined() {
                    let receiver = match self.heap.get(iterator) {
                        Some(Cell::Iterator {
                            source,
                            kind: IteratorKind::AsyncFromSync,
                            ..
                        }) => *source,
                        _ => iterator,
                    };
                    let atom = self.intern_atom("next");
                    let method = self.get_property(p, receiver, atom)?;
                    if !self.is_function(method) {
                        return Err(
                            self.type_error(p, "iterator next method is not callable".into())
                        );
                    }
                    self.write(f, next_method_register, method);
                    method
                } else {
                    next_method
                };
                let awaited_result = self.read(f, state_register);
                let asynchronous = p.functions[self.frames[f].function as usize].is_async;
                let result = if asynchronous && !awaited_result.is_undefined() {
                    self.write(f, state_register, Value::UNDEFINED);
                    awaited_result
                } else {
                    let input = self.read(f, i.result_register());
                    let result =
                        self.iterator_next_with_cached_method(p, iterator, next_method, &[input])?;
                    if asynchronous {
                        *pc -= 1;
                        let value = self.promise_for_value(p, result)?;
                        return Ok(StepResult::Await {
                            value,
                            destination: state_register,
                        });
                    }
                    result
                };
                let done_atom = self.intern_atom("done");
                let done = self.get_property(p, result, done_atom)?;
                if self.truthy(done) {
                    let value_atom = self.intern_atom("value");
                    let value = self.get_property(p, result, value_atom)?;
                    self.write(f, i.result_register(), value);
                } else {
                    *pc -= 1;
                    if asynchronous {
                        self.write(f, state_register, Value::UNDEFINED);
                    }
                    let value = if asynchronous {
                        let value_atom = self.intern_atom("value");
                        self.get_property(p, result, value_atom)?
                    } else {
                        Value::UNDEFINED
                    };
                    return Ok(StepResult::Yield {
                        value,
                        destination: i.result_register(),
                        delegated_result: Some(result),
                    });
                }
            }
            Op::SetField | Op::SetFieldStrict => {
                let object = self.read(f, i.register_b());
                let value = self.read(f, i.register_a());
                self.set_field_cached(
                    p,
                    object,
                    i.atom_index(),
                    value,
                    i.cache_site_index(),
                    p.functions[self.frames[f].function as usize].strict
                        || i.op() == Op::SetFieldStrict,
                )?;
            }
            Op::DefinePropertyRecord => {
                let mode = i.property_definition_mode().ok_or_else(|| {
                    JsError::validation("invalid property definition mode".into())
                })?;
                let target = self.read(f, i.register_b());
                let key = self.read(f, i.register_c());
                let descriptor =
                    super::object_descriptors::PropertyDescriptorRecord::for_definition(
                        mode,
                        self.read(f, i.register_a()),
                    );
                self.define_property_or_throw(p, target, key, descriptor)?;
            }
            Op::DefineField => {
                let object = self.read(f, i.register_b());
                let value = self.read(f, i.register_a());
                self.define_class_field(p, object, PropertyKey::string(i.atom_index()), value)?;
            }
            Op::DefineComputedField => {
                let object = self.read(f, i.register_b());
                let key = self.read(f, i.register_c());
                let key = self.to_property_key(p, key)?;
                let key = match self.heap.get(key).cloned() {
                    Some(Cell::String(text)) => PropertyKey::string(self.intern_js_atom(&text)),
                    Some(Cell::Symbol(_)) => PropertyKey::symbol(key),
                    _ => return Err(JsError::validation("invalid class field key".into())),
                };
                self.define_class_field(p, object, key, self.read(f, i.register_a()))?;
            }
            Op::SetThisField | Op::SetThisFieldStrict => {
                let this = self.checked_this_binding(p, f)?;
                self.set_field_cached(
                    p,
                    this,
                    i.atom_index(),
                    self.read(f, i.register_a()),
                    i.cache_site_index(),
                    p.functions[self.frames[f].function as usize].strict
                        || i.op() == Op::SetThisFieldStrict,
                )?;
            }
            Op::SetIndex => {
                #[cfg(feature = "profile-aggregate")]
                self.profile.index_dispatch(true, false);
                self.set_index_mode(
                    p,
                    self.read(f, i.register_b()),
                    self.read(f, i.register_c()),
                    self.read(f, i.register_a()),
                    p.functions[self.frames[f].function as usize].strict
                        || i.boolean_flag().expect("validated boolean immediate"),
                )?
            }
            Op::DefineArrayElement => self.define_array_literal_element(
                p,
                self.read(f, i.register_b()),
                i.array_index() as usize,
                self.read(f, i.register_a()),
            )?,
            Op::Move => self.write(f, i.result_register(), self.read(f, i.register_b())),
            Op::Binary | Op::NumericAdd | Op::NumericMultiply => {
                let operator = i.binary_operator();
                let left_operand = i.operand_b();
                let right_operand = i.operand_c();
                self.profile
                    .binary(operator as usize, left_operand.0, right_operand.0);
                let left = self.resolve_operand(p, f, left_operand)?;
                let right = self.resolve_operand(p, f, right_operand)?;
                #[cfg(feature = "profile-aggregate")]
                self.profile.regional_binary(
                    self.frames[f].function,
                    (*pc - 1) as u32,
                    self.numeric_binary(operator, left, right).is_some(),
                );
                let v = self.binary(p, operator, left, right)?;
                if i.returns_from_frame() {
                    return Ok(StepResult::Return(v));
                }
                if let Some(local) = i.numeric_local_target() {
                    self.frames[f].locals[local as usize] = v;
                    self.profile.virtual_opcode(Op::StoreLocalPlain as usize);
                } else {
                    self.write(f, i.result_register(), v);
                }
            }
            Op::WasmIndirectTarget => {
                let table = self.read(f, i.register_b());
                let index = self.wasm_table_index(table, self.read(f, i.register_c()))?;
                let value =
                    self.wasm_indirect_target(self.frames[f].program, table, index, i.imm())?;
                self.write(f, i.result_register(), value);
            }
            Op::WasmGlobalGet => {
                let value = self.wasm_global_load(self.read(f, i.register_b()))?;
                self.write(f, i.result_register(), value);
            }
            Op::WasmGlobalSet => {
                self.wasm_global_store(self.read(f, i.register_b()), self.read(f, i.register_a()))?;
            }
            Op::WasmRefFunc => {
                let value = self.wasm_function_reference(p, i.imm(), self.frames[f].env)?;
                self.write(f, i.result_register(), value);
            }
            Op::WasmRefIsNull => {
                let value = self.read(f, i.register_b()).is_null();
                self.write(f, i.result_register(), Value::integer(i32::from(value)));
            }
            Op::WasmRefAsNonNull => {
                let value = self.read(f, i.register_b());
                if value.is_null() {
                    return Err(JsError::wasm_trap_error(
                        crate::wasm::reference::NonNullCheck::from_tag(i.imm())
                            .expect("validated non-null check")
                            .trap(),
                    ));
                }
                self.write(f, i.result_register(), value);
            }
            Op::WasmTableSize => {
                let table = self.read(f, i.register_b());
                let size = self.wasm_table_elements(table)?.len() as u64;
                let value = self.encode_wasm_table_size(table, Some(size))?;
                self.write(f, i.result_register(), value);
            }
            Op::WasmTableGet | Op::WasmTableSet => {
                let table = self.read(f, i.register_b());
                let index = self.wasm_table_index(table, self.read(f, i.register_c()))?;
                if i.op() == Op::WasmTableGet {
                    let value = self.wasm_table_get(table, index)?;
                    self.write(f, i.result_register(), value);
                } else {
                    self.wasm_table_fill(table, index, self.read(f, i.register_a()), 1)?;
                }
            }
            Op::WasmTableGrow => {
                let table = self.read(f, i.register_b());
                let (delta, _) = i.register_pair();
                let delta = self.wasm_table_index(table, self.read(f, delta))?;
                let size = self.wasm_table_grow(table, self.read(f, i.register_c()), delta)?;
                let value = self.encode_wasm_table_size(table, size)?;
                self.write(f, i.result_register(), value);
            }
            Op::WasmTableFill | Op::WasmTableCopy | Op::WasmTableInit => {
                let table = self.read(f, i.register_a());
                let source = self.read(f, i.register_b());
                let (output, input) = i.register_pair();
                let output = self.wasm_table_index(table, self.read(f, output))?;
                let table_type = self.wasm_table_index_type(table)?;
                let length_type = match i.op() {
                    Op::WasmTableInit => crate::WasmType::I32,
                    Op::WasmTableCopy => match (table_type, self.wasm_table_index_type(source)?) {
                        (crate::WasmType::I64, crate::WasmType::I64) => crate::WasmType::I64,
                        _ => crate::WasmType::I32,
                    },
                    _ => table_type,
                };
                let length = self.wasm_index_operand(self.read(f, i.register_c()), length_type)?;
                match i.op() {
                    Op::WasmTableFill => self.wasm_table_fill(table, output, source, length)?,
                    Op::WasmTableInit => {
                        let input =
                            self.wasm_index_operand(self.read(f, input), crate::WasmType::I32)?;
                        self.wasm_table_init(table, source, output, input, length)?;
                    }
                    _ => {
                        let input = self.wasm_table_index(source, self.read(f, input))?;
                        self.wasm_table_copy(table, source, output, input, length)?;
                    }
                }
            }
            Op::WasmMemoryInit | Op::WasmMemoryCopy | Op::WasmMemoryFill => {
                let destination = self.read(f, i.register_a());
                let source = self.read(f, i.register_b());
                let (output, input) = i.register_pair();
                let output = self.wasm_memory_index(destination, self.read(f, output))?;
                let index_type = self.wasm_memory_index_type(destination)?;
                let length_type = match i.op() {
                    Op::WasmMemoryInit => crate::WasmType::I32,
                    Op::WasmMemoryCopy => {
                        match (index_type, self.wasm_memory_index_type(source)?) {
                            (crate::WasmType::I64, crate::WasmType::I64) => crate::WasmType::I64,
                            _ => crate::WasmType::I32,
                        }
                    }
                    _ => index_type,
                };
                let length = self.wasm_index_operand(self.read(f, i.register_c()), length_type)?;
                if i.op() == Op::WasmMemoryFill {
                    self.wasm_memory_fill(
                        destination,
                        output,
                        Self::wasm_u32_operand(source)?,
                        length,
                    )?;
                } else {
                    let input = if i.op() == Op::WasmMemoryInit {
                        self.wasm_index_operand(self.read(f, input), crate::WasmType::I32)?
                    } else {
                        self.wasm_memory_index(source, self.read(f, input))?
                    };
                    let source = if i.op() == Op::WasmMemoryInit && source == Value::UNDEFINED {
                        None
                    } else {
                        Some(source)
                    };
                    self.wasm_copy_bytes(destination, source, output, input, length)?;
                }
            }
            Op::WasmMemoryAddress => {
                let memory = self.read(f, i.register_c());
                let address = self.wasm_memory_index(memory, self.read(f, i.register_b()))?;
                let Constant::WasmBits64(offset) = p.constants[i.constant_index()] else {
                    return Err(JsError::validation(
                        "invalid Wasm memory offset constant".into(),
                    ));
                };
                let effective = address
                    .checked_add(offset)
                    .ok_or_else(|| JsError::wasm_trap_error(crate::WasmTrap::OutOfBoundsMemory))?;
                let value = self.encode_wasm_value(crate::WasmValue::I64(effective as i64));
                self.write(f, i.result_register(), value);
            }
            Op::WasmAtomicFence => crate::wasm::atomic::fence(),
            Op::WasmAtomicAccess => {
                use crate::wasm::atomic::{AtomicInput, AtomicOperator};
                let operator =
                    AtomicOperator::from_tag(i.imm()).expect("validated atomic operator");
                let window = i.register_window();
                let input = |role: AtomicInput| self.read(f, window.base + role as u16);
                let memory = input(AtomicInput::Memory);
                let address = self.wasm_i64_operand(input(AtomicInput::Address))? as u64;
                let value = if window.count > AtomicInput::Value as u16 {
                    Some(self.decode_wasm_value(input(AtomicInput::Value), operator.value_type())?)
                } else {
                    None
                };
                let replacement = if window.count > AtomicInput::Auxiliary as u16 {
                    Some(self.decode_wasm_value(
                        input(AtomicInput::Auxiliary),
                        operator.auxiliary_type(),
                    )?)
                } else {
                    None
                };
                let Some(Cell::WasmMemory { bytes, ty }) = self.heap.get(memory) else {
                    return Err(JsError::validation("invalid atomic memory binding".into()));
                };
                let result = operator
                    .apply(bytes, address, value, replacement, ty.shared)
                    .map_err(JsError::wasm_trap_error)?;
                let result = result
                    .map(|value| self.encode_wasm_value(value))
                    .unwrap_or(Value::UNDEFINED);
                self.write(f, i.result_register(), result);
            }
            Op::WasmMemoryLoad | Op::WasmMemoryStore => {
                let memory = self.read(f, i.register_b());
                let address = self.wasm_i64_operand(self.read(f, i.register_c()))? as u64;
                if i.op() == Op::WasmMemoryLoad {
                    let operator = crate::wasm::memory::MemoryLoad::from_tag(i.imm())
                        .expect("validated memory load");
                    let value = operator
                        .read(&self.wasm_memory_bytes(memory)?, address)
                        .map_err(JsError::wasm_trap_error)?;
                    let value = self.encode_wasm_value(value);
                    self.write(f, i.result_register(), value);
                } else {
                    let operator = crate::wasm::memory::MemoryStore::from_tag(i.imm())
                        .expect("validated memory store");
                    self.wasm_memory_store(
                        memory,
                        address,
                        operator,
                        self.read(f, i.register_a()),
                    )?;
                }
            }
            Op::WasmMemorySize | Op::WasmMemoryGrow => {
                let memory = self.read(f, i.register_b());
                let pages = if i.op() == Op::WasmMemoryGrow {
                    let delta = self.wasm_memory_index(memory, self.read(f, i.register_c()))?;
                    self.wasm_memory_grow(memory, delta)?
                } else {
                    Some(self.wasm_memory_pages(memory)?)
                };
                let index_type = self.wasm_memory_index_type(memory)?;
                let value = self.encode_wasm_index_size(index_type, pages);
                self.write(f, i.result_register(), value);
            }
            Op::WasmSimd => {
                let (operator, lane) = crate::wasm::simd::SimdOperator::from_selector(i.imm())
                    .expect("validated SIMD operator");
                let left =
                    self.decode_wasm_value(self.read(f, i.register_b()), operator.left_type())?;
                let right = operator
                    .right_type()
                    .map(|ty| self.decode_wasm_value(self.read(f, i.register_c()), ty))
                    .transpose()?;
                let value = self.encode_wasm_value(operator.apply(left, right, lane));
                self.write(f, i.result_register(), value);
            }
            Op::WasmSimdShuffle => {
                let crate::WasmValue::V128(left) =
                    self.decode_wasm_value(self.read(f, i.register_b()), crate::WasmType::V128)?
                else {
                    unreachable!()
                };
                let crate::WasmValue::V128(right) =
                    self.decode_wasm_value(self.read(f, i.register_c()), crate::WasmType::V128)?
                else {
                    unreachable!()
                };
                let Constant::WasmV128(indices) = p.constants[i.constant_index()] else {
                    unreachable!("validated shuffle indices")
                };
                let bits = crate::wasm::simd::shuffle(left, right, indices);
                let value = self.encode_wasm_value(crate::WasmValue::V128(bits));
                self.write(f, i.result_register(), value);
            }
            Op::WasmUnreachable => {
                return Err(JsError::wasm_trap_error(crate::WasmTrap::Unreachable));
            }
            Op::WasmI32Binary => {
                let (left, right) =
                    Value::int_pair(self.read(f, i.register_b()), self.read(f, i.register_c()))
                        .ok_or_else(|| JsError::validation("invalid Wasm i32 operands".into()))?;
                let operator = crate::wasm::integer::I32BinaryOperator::from_tag(i.imm())
                    .expect("validated Wasm binary operator");
                let value = operator
                    .apply(left, right)
                    .map_err(JsError::wasm_trap_error)?;
                let crate::WasmValue::I32(value) = value else {
                    unreachable!("i32 operator result")
                };
                self.write(f, i.result_register(), Value::integer(value));
            }
            Op::WasmArrayGet | Op::WasmArrayGetS | Op::WasmArrayGetU | Op::WasmArraySet => {
                let access =
                    crate::wasm::gc::GcFieldAccess::from_op(i.op()).expect("array field opcode");
                let write = i.op() == Op::WasmArraySet;
                let reference = self.read(
                    f,
                    if write {
                        i.register_a()
                    } else {
                        i.register_b()
                    },
                );
                let index = self
                    .read(
                        f,
                        if write {
                            i.register_b()
                        } else {
                            i.register_c()
                        },
                    )
                    .as_int()
                    .ok_or_else(|| JsError::validation("invalid Wasm array index".into()))?
                    as u32;
                let value = if write {
                    Some(self.read(f, i.register_c()))
                } else {
                    None
                };
                let result = self.wasm_gc_field(
                    reference,
                    super::wasm_gc::GcFieldIndex::Array(index),
                    access,
                    value,
                )?;
                if !write {
                    self.write(f, i.result_register(), result);
                }
            }
            Op::WasmArrayNewData | Op::WasmArrayNewElem => {
                use crate::wasm::gc::{ArraySegmentInput, ArraySegmentKind};
                let base = i.register_b();
                let input =
                    Self::wasm_u32_operand(self.read(f, base + ArraySegmentInput::Offset as u16))?;
                let count =
                    Self::wasm_u32_operand(self.read(f, base + ArraySegmentInput::Count as u16))?;
                let source = self.read(f, base + ArraySegmentInput::Segment as u16);
                let declarations = self
                    .programs
                    .wasm_signatures(self.frames[f].program)
                    .ok_or_else(|| {
                        JsError::validation("array constructor requires module declarations".into())
                    })?
                    .declarations
                    .clone();
                let value = self.wasm_array_segment_new(
                    &declarations,
                    i.imm(),
                    ArraySegmentKind::from_op(i.op()).unwrap(),
                    source,
                    input,
                    count,
                )?;
                self.write(f, i.result_register(), value);
            }
            Op::WasmArrayInitData | Op::WasmArrayInitElem => {
                let reference = self.read(f, i.register_a());
                let source = self.read(f, i.register_b());
                let count = Self::wasm_u32_operand(self.read(f, i.register_c()))?;
                let (output, input) = i.register_pair();
                let output = Self::wasm_u32_operand(self.read(f, output))?;
                let input = Self::wasm_u32_operand(self.read(f, input))?;
                self.wasm_array_segment_init(
                    reference,
                    crate::wasm::gc::ArraySegmentKind::from_op(i.op()).unwrap(),
                    source,
                    output,
                    input,
                    count,
                )?;
            }
            Op::WasmArrayFill | Op::WasmArrayCopy => {
                let destination = self.read(f, i.register_a());
                let source = self.read(f, i.register_b());
                let count = Self::wasm_u32_operand(self.read(f, i.register_c()))?;
                let (output, input) = i.register_pair();
                let output = Self::wasm_u32_operand(self.read(f, output))?;
                if i.op() == Op::WasmArrayFill {
                    self.wasm_array_fill(destination, output, source, count)?;
                } else {
                    let input = Self::wasm_u32_operand(self.read(f, input))?;
                    self.wasm_array_copy(destination, source, output, input, count)?;
                }
            }
            Op::WasmArrayLen => {
                let length = self.wasm_array_len(self.read(f, i.register_b()))?;
                self.write(f, i.result_register(), Value::integer(length as i32));
            }
            Op::WasmArrayNew | Op::WasmArrayNewDefault | Op::WasmArrayNewFixed => {
                let declarations = self
                    .programs
                    .wasm_signatures(self.frames[f].program)
                    .ok_or_else(|| {
                        JsError::validation("Wasm array requires registered declarations".into())
                    })?
                    .declarations
                    .clone();
                let initialization = if i.op() == Op::WasmArrayNewFixed {
                    let mut values = Vec::new();
                    values
                        .try_reserve_exact(usize::from(i.register_window().count))
                        .map_err(|_| JsError::wasm_trap_error(crate::WasmTrap::ArrayTooLarge))?;
                    for offset in 0..i.register_window().count {
                        values.push(self.read(f, i.register_b() + offset));
                    }
                    super::wasm_gc::ArrayInitialization::Fixed(values)
                } else {
                    let count_register = if i.op() == Op::WasmArrayNew {
                        i.register_c()
                    } else {
                        i.register_b()
                    };
                    let count =
                        self.read(f, count_register).as_int().ok_or_else(|| {
                            JsError::validation("invalid Wasm array length".into())
                        })? as u32;
                    if i.op() == Op::WasmArrayNew {
                        super::wasm_gc::ArrayInitialization::Repeated {
                            count,
                            value: self.read(f, i.register_b()),
                        }
                    } else {
                        super::wasm_gc::ArrayInitialization::Default(count)
                    }
                };
                let value = self.wasm_array_new(&declarations, i.imm(), initialization)?;
                self.write(f, i.result_register(), value);
            }
            Op::WasmStructGet | Op::WasmStructGetS | Op::WasmStructGetU | Op::WasmStructSet => {
                let access =
                    crate::wasm::gc::GcFieldAccess::from_op(i.op()).expect("struct field opcode");
                let write = i.op() == Op::WasmStructSet;
                let reference = self.read(
                    f,
                    if write {
                        i.register_a()
                    } else {
                        i.register_b()
                    },
                );
                let value = if write {
                    Some(self.read(f, i.register_b()))
                } else {
                    None
                };
                let result = self.wasm_gc_field(
                    reference,
                    super::wasm_gc::GcFieldIndex::Struct(i.imm()),
                    access,
                    value,
                )?;
                if !write {
                    self.write(f, i.result_register(), result);
                }
            }
            Op::WasmStructNew
            | Op::WasmStructNewDefault
            | Op::WasmStructNewDesc
            | Op::WasmStructNewDefaultDesc
            | Op::WasmRefGetDesc => {
                let declarations = self
                    .programs
                    .wasm_signatures(self.frames[f].program)
                    .ok_or_else(|| {
                        JsError::validation("Wasm struct requires registered declarations".into())
                    })?
                    .declarations
                    .clone();
                if i.op() == Op::WasmRefGetDesc {
                    let value = self.wasm_ref_get_desc(
                        &declarations,
                        i.imm(),
                        self.read(f, i.register_b()),
                    )?;
                    self.write(f, i.result_register(), value);
                } else {
                    let mode = crate::wasm::gc::StructConstruction::from_op(i.op()).unwrap();
                    let window =
                        (!mode.defaulted() || mode.described()).then(|| i.register_window());
                    let descriptor = if mode.described() {
                        let window = window.unwrap();
                        Some(self.read(f, window.base + window.count - 1))
                    } else {
                        None
                    };
                    let values = if mode.defaulted() {
                        None
                    } else {
                        let window = window.unwrap();
                        let count = window.count - u16::from(mode.described());
                        let mut values = Vec::new();
                        values.try_reserve_exact(usize::from(count)).map_err(|_| {
                            JsError::validation("Wasm struct allocation failed".into())
                        })?;
                        values.extend((0..count).map(|offset| self.read(f, window.base + offset)));
                        Some(values)
                    };
                    let value = self.wasm_struct_new(&declarations, i.imm(), values, descriptor)?;
                    self.write(f, i.result_register(), value);
                }
            }
            Op::WasmExternalConversion => {
                let conversion = crate::wasm::reference::ExternalConversion::from_tag(i.imm())
                    .expect("validated external conversion");
                let input = self.read(f, i.register_b());
                self.decode_wasm_value(input, conversion.input_type())?;
                let value = self.wasm_external_conversion(conversion, input);
                self.write(f, i.result_register(), value);
            }
            Op::WasmRefEq => {
                let equal = self.read(f, i.register_b()) == self.read(f, i.register_c());
                self.write(f, i.result_register(), Value::integer(i32::from(equal)));
            }
            Op::WasmDescriptorTest | Op::WasmDescriptorCast => {
                let target = crate::wasm::reference::ReferenceTarget::from_tag(i.imm())
                    .expect("validated reference target");
                let owner = self
                    .programs
                    .wasm_signatures(self.frames[f].program)
                    .ok_or_else(|| {
                        JsError::validation(
                            "descriptor cast requires registered declarations".into(),
                        )
                    })?
                    .declarations
                    .clone();
                let reference = self.read(f, i.register_b());
                let valid = self.wasm_descriptor_matches(
                    &owner,
                    target,
                    reference,
                    self.read(f, i.register_c()),
                )?;
                let result = if i.op() == Op::WasmDescriptorTest {
                    Value::integer(i32::from(valid))
                } else if valid {
                    reference
                } else {
                    return Err(JsError::wasm_trap_error(
                        crate::WasmTrap::DescriptorCastFailure,
                    ));
                };
                self.write(f, i.result_register(), result);
            }
            Op::WasmRefTest | Op::WasmRefCast => {
                let reference = self.read(f, i.register_b());
                let target = crate::wasm::reference::ReferenceTarget::from_tag(i.imm())
                    .and_then(|target| target.reference_type())
                    .expect("validated reference target");
                let pool = self
                    .programs
                    .wasm_signatures(self.frames[f].program)
                    .ok_or_else(|| {
                        JsError::validation(
                            "reference cast requires registered module declarations".into(),
                        )
                    })?;
                let ty = pool
                    .declarations
                    .callable_value_type(wasmparser::ValType::Ref(target))
                    .ok_or_else(|| {
                        JsError::validation("invalid reference cast declaration".into())
                    })?;
                let valid = self.wasm_reference_valid_in(reference, ty, Some(&pool.declarations));
                let result = if i.op() == Op::WasmRefTest {
                    Value::integer(i32::from(valid))
                } else if valid {
                    reference
                } else {
                    return Err(JsError::wasm_trap_error(crate::WasmTrap::CastFailure));
                };
                self.write(f, i.result_register(), result);
            }
            Op::WasmI31 => {
                let operator = crate::wasm::i31::I31Operator::from_tag(i.imm())
                    .expect("validated i31 operator");
                let value =
                    self.decode_wasm_value(self.read(f, i.register_b()), operator.input_type())?;
                let value = operator.apply(value).map_err(JsError::wasm_trap_error)?;
                let value = self.encode_wasm_value(value);
                self.write(f, i.result_register(), value);
            }
            Op::WasmI32Unary => {
                let value = self
                    .read(f, i.register_b())
                    .as_int()
                    .ok_or_else(|| JsError::validation("invalid Wasm i32 operand".into()))?;
                let operator = crate::wasm::integer::I32UnaryOperator::from_tag(i.imm())
                    .expect("validated Wasm unary operator");
                let crate::WasmValue::I32(value) = operator.apply(value) else {
                    unreachable!("i32 operator result")
                };
                self.write(f, i.result_register(), Value::integer(value));
            }
            Op::WasmI64Binary => {
                let left = self.wasm_i64_operand(self.read(f, i.register_b()))?;
                let right = self.wasm_i64_operand(self.read(f, i.register_c()))?;
                let operator = crate::wasm::integer::I64BinaryOperator::from_tag(i.imm())
                    .expect("validated Wasm binary operator");
                let value = operator
                    .apply(left, right)
                    .map_err(JsError::wasm_trap_error)?;
                let value = self.encode_wasm_value(value);
                self.write(f, i.result_register(), value);
            }
            Op::WasmI64Unary => {
                let value = self.wasm_i64_operand(self.read(f, i.register_b()))?;
                let operator = crate::wasm::integer::I64UnaryOperator::from_tag(i.imm())
                    .expect("validated Wasm unary operator");
                let value = self.encode_wasm_value(operator.apply(value));
                self.write(f, i.result_register(), value);
            }
            Op::WasmScalarConvert => {
                let operator = crate::wasm::conversion::ScalarConversionOperator::from_tag(i.imm())
                    .expect("validated Wasm scalar conversion");
                let value =
                    self.decode_wasm_value(self.read(f, i.register_b()), operator.source_type())?;
                let value = operator.apply(value).map_err(JsError::wasm_trap_error)?;
                debug_assert_eq!(value.ty(), operator.result_type());
                let value = self.encode_wasm_value(value);
                self.write(f, i.result_register(), value);
            }
            Op::WasmF32Binary => {
                let left = self.wasm_f32_operand(self.read(f, i.register_b()))?;
                let right = self.wasm_f32_operand(self.read(f, i.register_c()))?;
                let operator = crate::wasm::float::F32BinaryOperator::from_tag(i.imm())
                    .expect("validated Wasm float operator");
                let value = self.encode_wasm_value(operator.apply(left, right));
                self.write(f, i.result_register(), value);
            }
            Op::WasmF32Unary => {
                let left = self.wasm_f32_operand(self.read(f, i.register_b()))?;
                let operator = crate::wasm::float::F32UnaryOperator::from_tag(i.imm())
                    .expect("validated Wasm float operator");
                let value = self.encode_wasm_value(operator.apply(left));
                self.write(f, i.result_register(), value);
            }
            Op::WasmF64Binary => {
                let left = self.wasm_f64_operand(self.read(f, i.register_b()))?;
                let right = self.wasm_f64_operand(self.read(f, i.register_c()))?;
                let operator = crate::wasm::float::F64BinaryOperator::from_tag(i.imm())
                    .expect("validated Wasm float operator");
                let value = self.encode_wasm_value(operator.apply(left, right));
                self.write(f, i.result_register(), value);
            }
            Op::WasmF64Unary => {
                let left = self.wasm_f64_operand(self.read(f, i.register_b()))?;
                let operator = crate::wasm::float::F64UnaryOperator::from_tag(i.imm())
                    .expect("validated Wasm float operator");
                let value = self.encode_wasm_value(operator.apply(left));
                self.write(f, i.result_register(), value);
            }
            Op::IncDec => {
                let input = self.read(f, i.register_b());
                let is_decrement = i.boolean_flag().expect("validated boolean immediate");
                let delta = if is_decrement { -1.0 } else { 1.0 };
                let value = if matches!(self.heap.get(input), Some(Cell::BigInt(_))) {
                    let one = self.heap.alloc(Cell::BigInt("1".into()));
                    self.binary(p, if is_decrement { 9 } else { 8 }, input, one)?
                } else if let Some(integer) = input.as_int() {
                    let next = if is_decrement {
                        integer.checked_sub(1)
                    } else {
                        integer.checked_add(1)
                    };
                    next.map(Value::integer)
                        .unwrap_or_else(|| Value::number(f64::from(integer) + delta))
                } else {
                    Value::number(self.to_number(p, input)? + delta)
                };
                self.write(f, i.result_register(), value);
            }
            Op::Unary => {
                let v = self.unary(p, i.unary_operator(), self.read(f, i.register_b()))?;
                self.write(f, i.result_register(), v);
            }
            Op::Delete => {
                let result = self.delete_reference_property(
                    p,
                    self.read(f, i.register_b()),
                    self.read(f, i.register_c()),
                )?;
                if !self.truthy(result)
                    && (p.functions[self.frames[f].function as usize].strict
                        || i.boolean_flag().expect("validated boolean immediate"))
                {
                    return Err(self.type_error(p, "Cannot delete property in strict mode".into()));
                }
                self.write(f, i.result_register(), result);
            }
            Op::Jump => {
                *pc = i.jump_target() as usize;
                self.frames[f].pc = *pc;
                self.maybe_collect(p);
            }
            Op::JumpFalse => {
                let value = self.read(f, i.register_a());
                let truthy = self.truthy(value);
                #[cfg(feature = "profile-aggregate")]
                self.profile
                    .branch_value(value.profile_kind() as usize, truthy);
                if !truthy {
                    *pc = i.jump_target() as usize;
                }
            }
            Op::JumpBinaryFalse => {
                let operator = i.binary_operator_field();
                let left_operand = i.operand_b();
                let right_operand = i.operand_c();
                self.profile
                    .binary(operator as usize, left_operand.0, right_operand.0);
                let left = self.resolve_operand(p, f, left_operand)?;
                let right = self.resolve_operand(p, f, right_operand)?;
                if !self.binary_truthy(p, operator, left, right)? {
                    *pc = i.jump_target() as usize;
                }
            }
            Op::Call | Op::CallDirectEvalArray => {
                self.profile.call_source(0);
                let window = i.call_window();
                let arguments = if i.op() == Op::CallDirectEvalArray {
                    let array = self.read(f, window.base);
                    CallArguments::from_values(self.array_values(array)?)
                } else {
                    CallArguments::from_slice(self.register_window(f, window))
                };
                let this = self.read(f, i.register_c());
                let callee = self.read(f, i.register_b());
                let args = arguments.as_slice();
                self.frames[f].pc = *pc;
                let direct_eval = i.direct_eval()
                    && matches!(
                        self.heap.get(callee),
                        Some(Cell::Function {
                            kind: FunctionKind::Native(Native::Eval),
                            realm,
                            ..
                        }) if *realm == self.realm.globals
                    );
                let previous_binding_site_pc = if direct_eval {
                    Some(std::mem::replace(
                        &mut self.frames[f].binding_site_pc,
                        Some(*pc as u32),
                    ))
                } else {
                    None
                };
                let parameter_eval = direct_eval && i.parameter_eval();
                let previous_direct_eval = self.direct_eval;
                let previous_parameter_eval = self.parameter_eval;
                self.direct_eval = direct_eval;
                self.parameter_eval = parameter_eval;
                let previous_this = (direct_eval && !this.is_undefined())
                    .then(|| std::mem::replace(&mut self.frames[f].this, this));
                let terminal = i.returns_from_frame()
                    || p.functions[self.frames[f].function as usize]
                        .code
                        .get(*pc)
                        .is_some_and(|packed| {
                            if packed.is_wide() {
                                p.functions[self.frames[f].function as usize].wide
                                    [packed.wide_index()]
                                .op()
                                    == Op::Return
                            } else {
                                packed.op() == Op::Return
                            }
                        });
                if allow_inline_calls
                    && i.op() == Op::Call
                    && !direct_eval
                    && !previous_direct_eval
                    && !previous_parameter_eval
                    && !terminal
                    && p.kind != crate::bytecode::ProgramKind::Wasm
                    && f + 1 == self.frames.len()
                    && self.frames[f].function != super::ROOT_FUNCTION_ID
                    && p.functions
                        .get(self.frames[f].function as usize)
                        .is_some_and(|function| {
                            function.dispatch == DispatchClass::General
                        })
                    && let Some(CallTarget::User(program_id, id, env)) =
                        self.call_target(callee).ok()
                    && id != super::ROOT_FUNCTION_ID
                    && program_id == self.frames[f].program
                    && program_id == self.active_program
                    && self.heap.get(callee).is_some_and(|cell| {
                        matches!(cell, Cell::Function { realm, .. } if *realm == self.realm.globals)
                    })
                    && p.functions.get(id as usize).is_some_and(|function| {
                        function.dispatch == DispatchClass::General
                            && !function.is_async
                            && !function.is_generator
                            && !function.is_class_constructor
                            && !function.derived_constructor
                            && !function.class_field_initializer
                            && !function.parameter_eval_arguments_error
                    })
                {
                    self.profile.call_target(1, args.len());
                    // Collection happens only at back edges, tail calls and explicit host
                    // safepoints, none of which can run while a frame is pushed; afterwards the
                    // new frame itself roots the callee, receiver and arguments.
                    let result = self.push_general_user_frame(
                        p,
                        id,
                        env,
                        this,
                        args,
                        CallContext::user_function(id, callee),
                    );
                    self.direct_eval = previous_direct_eval;
                    self.parameter_eval = previous_parameter_eval;
                    let stack_guard = result?;
                    return Ok(StepResult::PushFrame {
                        destination: i.result_register(),
                        stack_guard,
                        construct_this: None,
                    });
                }
                if p.kind == crate::bytecode::ProgramKind::Wasm
                    && i.returns_from_frame()
                    && let Some(CallTarget::User(program_id, id, env)) =
                        self.call_target(callee).ok()
                {
                    let program = self.programs.get(program_id).ok_or_else(|| {
                        JsError::validation("missing Wasm tail-call program".into())
                    })?;
                    let realm = match self.heap.get(callee) {
                        Some(Cell::Function { realm, .. }) => *realm,
                        _ => self.realm.globals,
                    };
                    let result = self.with_call_roots(
                        [callee, this].into_iter().chain(args.iter().copied()),
                        |vm| {
                            vm.active_program = program_id;
                            vm.realm.globals = realm;
                            vm.prepare_user_tail(
                                &program,
                                f,
                                id,
                                env,
                                this,
                                args,
                                CallContext::Function(callee),
                            )
                        },
                    );
                    self.direct_eval = previous_direct_eval;
                    self.parameter_eval = previous_parameter_eval;
                    result?;
                    self.profile.terminal_call(0);
                    return Ok(StepResult::TailCall);
                }
                if terminal
                    && p.kind != crate::bytecode::ProgramKind::Wasm
                    && p.functions[self.frames[f].function as usize].strict
                    && !p.functions[self.frames[f].function as usize].class_field_initializer
                    && !matches!(self.heap.get(callee), Some(Cell::Proxy { .. }))
                    && let Some(CallTarget::User(program_id, id, env)) =
                        self.call_target(callee).ok()
                    && program_id == self.frames[f].program
                    && !p.functions[id as usize].is_async
                    && !p.functions[id as usize].is_generator
                {
                    self.with_call_roots(
                        [callee, this].into_iter().chain(args.iter().copied()),
                        |vm| {
                            vm.prepare_user_tail(
                                p,
                                f,
                                id,
                                env,
                                this,
                                args,
                                CallContext::Function(callee),
                            )
                        },
                    )?;
                    self.direct_eval = previous_direct_eval;
                    self.parameter_eval = previous_parameter_eval;
                    self.profile.terminal_call(0);
                    return Ok(StepResult::TailCall);
                }
                let discarded_wasm_frame =
                    p.kind == crate::bytecode::ProgramKind::Wasm && i.returns_from_frame();
                if discarded_wasm_frame {
                    // Native/host calls need argument roots, not the discarded guest activation.
                    self.with_stack.truncate(self.frames[f].with_base);
                    let frame = &mut self.frames[f];
                    frame.context = CallContext::Internal;
                    frame.original_arguments.clear();
                    frame.locals.fill(Value::UNDEFINED);
                    frame.registers.fill(Value::UNDEFINED);
                    frame.dynamic_bindings.clear();
                    frame.active_iterators.clear();
                    frame.env = Value::NULL;
                    frame.this = Value::UNDEFINED;
                    frame.fixed_this = false;
                    frame.captured = false;
                }
                let called = if discarded_wasm_frame {
                    self.call_value(p, callee, this, args)
                } else {
                    self.call_value_from_frame(p, callee, this, args)
                };
                if let Some(binding_site_pc) = previous_binding_site_pc {
                    self.frames[f].binding_site_pc = binding_site_pc;
                }
                let value = match called {
                    Ok(value) => value,
                    Err(error) => {
                        if let Some(previous_this) = previous_this {
                            self.frames[f].this = previous_this;
                        }
                        self.direct_eval = previous_direct_eval;
                        self.parameter_eval = previous_parameter_eval;
                        return Err(error);
                    }
                };
                if let Some(previous_this) = previous_this {
                    self.frames[f].this = previous_this;
                }
                self.direct_eval = previous_direct_eval;
                self.parameter_eval = previous_parameter_eval;
                if i.returns_from_frame() {
                    self.profile.terminal_call(0);
                    return Ok(StepResult::Return(value));
                }
                self.write(f, i.result_register(), value);
            }
            Op::CallKnown => {
                self.profile.call_source(1);
                let window = i.call_window();
                let function_index = i.known_function_index();
                let arguments = CallArguments::from_slice(self.register_window(f, window));
                let args = arguments.as_slice();
                let parent = self.capture_env(f, 0).unwrap_or(self.frames[f].env);
                self.profile.call_target(1, usize::from(window.count));
                self.frames[f].pc = *pc;
                let terminal = i.returns_from_frame()
                    || p.functions[self.frames[f].function as usize]
                        .code
                        .get(*pc)
                        .is_some_and(|packed| {
                            if packed.is_wide() {
                                p.functions[self.frames[f].function as usize].wide
                                    [packed.wide_index()]
                                .op()
                                    == Op::Return
                            } else {
                                packed.op() == Op::Return
                            }
                        });
                if ((p.kind == crate::bytecode::ProgramKind::Wasm && i.returns_from_frame())
                    || (p.kind != crate::bytecode::ProgramKind::Wasm
                        && terminal
                        && p.functions[self.frames[f].function as usize].strict))
                    && !p.functions[function_index as usize].is_async
                    && !p.functions[function_index as usize].is_generator
                {
                    self.prepare_user_tail(
                        p,
                        f,
                        u32::from(function_index),
                        parent,
                        Value::UNDEFINED,
                        args,
                        CallContext::Internal,
                    )?;
                    self.profile.terminal_call(0);
                    return Ok(StepResult::TailCall);
                }
                let value = self.call_user_maybe_async(
                    p,
                    u32::from(function_index),
                    parent,
                    Value::UNDEFINED,
                    args,
                    CallContext::Internal,
                )?;
                if i.returns_from_frame() {
                    self.profile.terminal_call(0);
                    return Ok(StepResult::Return(value));
                }
                self.write(f, i.result_register(), value);
            }
            Op::CallMethod => {
                self.profile.call_source(2);
                let this = self.read(f, i.register_b());
                self.frames[f].pc = *pc;
                let value = self.call_method_site_safe(p, f, i.method_site_index(), this)?;
                if i.returns_from_frame() {
                    self.profile.terminal_call(1);
                    return Ok(StepResult::Return(value));
                }
                self.write(f, i.result_register(), value);
            }
            Op::CallThisMethod => {
                self.profile.call_source(3);
                let method_site = i.method_site_index();
                let path = p.method_sites[method_site].receiver_path;
                let this = if let Some((atom, cache)) = path {
                    self.get_field_cached(p, self.frames[f].this, atom, cache)?
                } else {
                    self.frames[f].this
                };
                self.frames[f].pc = *pc;
                let value = self.call_method_site_safe(p, f, method_site, this)?;
                if i.returns_from_frame() {
                    self.profile.terminal_call(2);
                    return Ok(StepResult::Return(value));
                }
                self.write(f, i.result_register(), value);
            }
            Op::Construct => {
                self.profile.call_source(4);
                if allow_inline_calls
                    && !i.is_super_construct()
                    && !i.returns_from_frame()
                    && let crate::bytecode::ConstructArguments::Registers(window) =
                        i.construct_arguments()
                {
                    let callee = self.read(f, i.register_b());
                    if let Some((id, env, this)) = self.inline_construct_target(p, f, callee) {
                        let arguments = CallArguments::from_slice(self.register_window(f, window));
                        self.frames[f].pc = *pc;
                        self.construct_target = Some(callee);
                        let pushed = self.push_general_user_frame(
                            p,
                            id,
                            env,
                            this,
                            arguments.as_slice(),
                            CallContext::user_function(id, callee),
                        );
                        self.construct_target = None;
                        return Ok(StepResult::PushFrame {
                            destination: i.result_register(),
                            stack_guard: pushed?,
                            construct_this: Some(this),
                        });
                    }
                }
                let args = match i.construct_arguments() {
                    crate::bytecode::ConstructArguments::Array(register) => {
                        let array = self.read(f, register);
                        if !matches!(self.heap.get(array), Some(Cell::Array { .. })) {
                            return Err(JsError(
                                "super constructor arguments are not an array".into(),
                            ));
                        }
                        self.call_argument_list(p, array, false)?
                    }
                    crate::bytecode::ConstructArguments::Registers(window) => {
                        self.register_window(f, window).to_vec()
                    }
                };
                self.frames[f].pc = *pc;
                let callee = self.read(f, i.register_b());
                let v = if i.is_super_construct() {
                    self.construct_super_value(p, callee, &args)?
                } else {
                    self.construct_value(p, callee, &args)?
                };
                if i.returns_from_frame() {
                    return Ok(StepResult::Return(v));
                }
                self.write(f, i.result_register(), v);
            }
            Op::Return => return Ok(StepResult::Return(self.read(f, i.register_a()))),
            Op::WasmExceptionNew => {
                use crate::wasm::tag::ExceptionInput;
                let window = i.register_window();
                if window.count < ExceptionInput::MIN_COUNT {
                    return Err(JsError::validation("missing Wasm exception tag".into()));
                }
                let tag = self.read(f, window.base + ExceptionInput::Tag as u16);
                let payload = (ExceptionInput::Payload as u16..window.count)
                    .map(|offset| self.read(f, window.base + offset))
                    .collect();
                let value = self.wasm_exception_new(tag, payload)?;
                self.write(f, i.result_register(), value);
            }
            Op::WasmExceptionMatch => {
                let matches = self.wasm_exception_matches(
                    self.read(f, i.register_b()),
                    self.read(f, i.register_c()),
                )?;
                self.write(f, i.result_register(), Value::integer(i32::from(matches)));
            }
            Op::WasmExceptionPayload => {
                let value = self.wasm_exception_payload(self.read(f, i.register_b()), i.imm())?;
                self.write(f, i.result_register(), value);
            }
            Op::WasmThrowRef => return Err(self.wasm_throw_ref(self.read(f, i.register_a()))),
            Op::Throw => {
                let value = self.read(f, i.register_a());
                let message = if self.object_data(value).is_some() {
                    let atom = self.intern_atom("message");
                    match self.get_property(p, value, atom)? {
                        value if matches!(self.heap.get(value), Some(Cell::String(_))) => {
                            self.to_string(p, value)?
                        }
                        _ => self.to_string(p, value)?,
                    }
                } else {
                    self.to_string(p, value)?
                };
                return Err(JsError::thrown(value, message));
            }
        }
        Ok(StepResult::Continue)
    }
    #[inline(always)]
    pub(super) fn resolve_operand(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        operand: Operand,
    ) -> Result<Value, JsError> {
        let tag = operand.tag();
        self.profile.operand(tag as usize);
        match operand.kind() {
            Some(crate::bytecode::OperandKind::Register) => {
                Ok(self.read(frame, operand.payload() as Register))
            }
            Some(crate::bytecode::OperandKind::Constant) => self
                .programs
                .constant(self.frames[frame].program, operand.payload() as usize)
                .ok_or_else(|| JsError::validation("constant operand is outside program".into())),
            Some(crate::bytecode::OperandKind::Field) => {
                self.resolve_field(p, frame, usize::from(operand.payload()))
            }
            Some(crate::bytecode::OperandKind::Local) => {
                let slot = operand.payload() as usize;
                // A plain slot is exactly what `LoadLocalPlain` reads.
                if p.functions[self.frames[frame].function as usize].plain_local(&p.atoms, slot) {
                    return Ok(self.frames[frame].locals[slot]);
                }
                self.load_local_binding(p, frame, slot, None)
            }
            None => unreachable!("two-bit operand tag"),
        }
    }
    #[inline(always)]
    pub(super) fn resolve_field(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        site: usize,
    ) -> Result<Value, JsError> {
        let site = p.field_sites[site];
        let base = self.resolve_field_base(p, frame, site.base)?;
        let value = self.get_field_cached(p, base, site.first.0, site.first.1)?;
        match site.second {
            Some((atom, cache)) => self.get_field_cached(p, value, atom, cache),
            None => Ok(value),
        }
    }
}
