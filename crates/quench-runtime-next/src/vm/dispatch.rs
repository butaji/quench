use super::*;
use crate::heap::PrivateBrand;
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
                let value = match self.heap.get(environment) {
                    Some(Cell::Environment { slots, .. }) => {
                        slots.get(usize::from(binding)).copied()
                    }
                    _ => None,
                };
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
    ) -> Result<StepResult, JsError> {
        match i.op() {
            Op::Nop => {}
            Op::CloneEnv => {
                self.clone_frame_environment(f);
            }
            Op::Wide => unreachable!("validated dispatch cannot contain nested wide instruction"),
            Op::LoadConst => {
                let value = self
                    .programs
                    .constant(self.frames[f].program, i.constant_index())
                    .ok_or_else(|| {
                        JsError::validation("constant index is outside program".into())
                    })?;
                self.write(f, i.a(), value);
            }
            Op::LoadLocal => {
                let slot = i.local_slot();
                let v = if let Some(value) =
                    self.module_import_value(self.frames[f].program, self.frames[f].function, slot)
                {
                    value
                } else if self.frames[f].captured {
                    let Some(Cell::Environment { slots, .. }) = self.heap.get(self.frames[f].env)
                    else {
                        return Err(JsError("invalid local environment".into()));
                    };
                    *slots
                        .get(slot)
                        .ok_or_else(|| JsError("invalid local slot".into()))?
                } else {
                    // SAFETY: compiler construction and residual decoding establish
                    // the frame-local bound before interpretation.
                    unsafe { *self.frames.get_unchecked(f).locals.get_unchecked(slot) }
                };
                if v.is_deleted() {
                    let atom = p.functions[self.frames[f].function as usize]
                        .local_atoms
                        .get(slot)
                        .copied()
                        .unwrap_or_default();
                    return Err(self.reference_error(
                        p,
                        format!(
                            "Cannot access '{}' before initialization",
                            self.atom_name(atom)
                        ),
                    ));
                }
                let v = self.mapped_argument_load(p, f, slot, v);
                self.write(f, i.a(), v);
            }
            Op::StoreLocal => {
                let value = self.read(f, i.a());
                let slot = i.local_slot();
                let function = &p.functions[self.frames[f].function as usize];
                if function
                    .local_atoms
                    .get(slot)
                    .is_some_and(|atom| self.atom_name(*atom).contains("\0rqj:self-binding:"))
                {
                    if function.strict {
                        return Err(
                            self.type_error(p, "assignment to function name binding".into())
                        );
                    }
                    if i.b() != 0 {
                        self.write(f, i.b() - 1, value);
                    }
                    return Ok(StepResult::Continue);
                }
                if self.frames[f].function == 0 {
                    let current = if self.frames[f].captured {
                        match self.heap.get(self.frames[f].env) {
                            Some(Cell::Environment { slots, .. }) => slots.get(slot).copied(),
                            _ => None,
                        }
                    } else {
                        self.frames[f].locals.get(slot).copied()
                    };
                    if current.is_some_and(Value::is_deleted) && i.c() == 0 {
                        let atom = function.local_atoms.get(slot).copied().unwrap_or_default();
                        return Err(self.reference_error(
                            p,
                            format!(
                                "Cannot access '{}' before initialization",
                                self.atom_name(atom)
                            ),
                        ));
                    }
                    if function
                        .local_atoms
                        .get(slot)
                        .is_some_and(|atom| function.global_immutable_atoms.contains(atom))
                        && i.c() == 0
                    {
                        return Err(self.type_error(p, "assignment to constant binding".into()));
                    }
                }
                if self.frames[f].captured {
                    let Some(Cell::Environment { slots, .. }) =
                        self.heap.get_mut(self.frames[f].env)
                    else {
                        return Err(JsError("invalid local environment".into()));
                    };
                    *slots
                        .get_mut(slot)
                        .ok_or_else(|| JsError("invalid local slot".into()))? = value;
                } else {
                    self.frames[f].locals[slot] = value;
                }
                self.mirror_global_lexical_binding(p, f, slot, value);
                self.mapped_argument_store(p, f, slot, value);
                if i.b() != 0 {
                    self.write(f, i.b() - 1, value);
                }
            }
            Op::LoadEnvLocal => {
                let slot = i.local_slot();
                let value = if let Some(value) =
                    self.module_import_value(self.frames[f].program, self.frames[f].function, slot)
                {
                    value
                } else if self.frames[f].captured {
                    let Some(Cell::Environment { slots, .. }) = self.heap.get(self.frames[f].env)
                    else {
                        return Err(JsError("invalid local environment".into()));
                    };
                    slots[slot]
                } else {
                    self.frames[f].locals[slot]
                };
                if value.is_deleted() {
                    let atom = p.functions[self.frames[f].function as usize]
                        .local_atoms
                        .get(slot)
                        .copied()
                        .unwrap_or_default();
                    return Err(self.reference_error(
                        p,
                        format!(
                            "Cannot access '{}' before initialization",
                            self.atom_name(atom)
                        ),
                    ));
                }
                let value = self.mapped_argument_load(p, f, slot, value);
                self.write(f, i.a(), value);
            }
            Op::StoreEnvLocal => {
                let value = self.read(f, i.a());
                let slot = i.local_slot();
                let function = &p.functions[self.frames[f].function as usize];
                if function
                    .local_atoms
                    .get(slot)
                    .is_some_and(|atom| self.atom_name(*atom).contains("\0rqj:self-binding:"))
                {
                    if function.strict {
                        return Err(
                            self.type_error(p, "assignment to function name binding".into())
                        );
                    }
                    return Ok(StepResult::Continue);
                }
                let global_var = self.root_global_var_atom(p, self.frames[f].function, slot);
                if let Some(atom) = global_var {
                    if !self.set_property_with_receiver(
                        p,
                        self.realm.globals,
                        atom,
                        value,
                        self.realm.globals,
                    )? && p.functions[self.frames[f].function as usize].strict
                    {
                        return Err(
                            self.type_error(p, "cannot assign to read-only global binding".into())
                        );
                    }
                }
                if self.frames[f].captured {
                    let Some(Cell::Environment { slots, .. }) =
                        self.heap.get_mut(self.frames[f].env)
                    else {
                        return Err(JsError("invalid local environment".into()));
                    };
                    slots[slot] = value;
                } else {
                    self.frames[f].locals[slot] = value;
                }
                self.mapped_argument_store(p, f, slot, value);
            }
            Op::LoadCapture => {
                let v = self.capture(p, f, i.capture_depth(), i.capture_slot())?;
                self.write(f, i.a(), v);
            }
            Op::StoreCapture => self.store_capture(
                p,
                f,
                i.capture_depth(),
                i.capture_slot(),
                self.read(f, i.a()),
            )?,
            Op::LoadName => {
                let v = self.load_name(p, i.atom_index(), i.c())?;
                self.write(f, i.a(), v);
            }
            Op::LoadNameCall => {
                let (callee, this) = self.load_name_call(p, i.atom_index(), i.c())?;
                self.write(f, i.a(), callee);
                self.write(f, i.b(), this);
            }
            Op::LoadNameTypeof => {
                let v = self.load_name_typeof(p, i.atom_index(), i.c())?;
                self.write(f, i.a(), v);
            }
            Op::StoreName => self.store_name(p, i.atom_index(), self.read(f, i.a()), i.c())?,
            Op::LoadThis => {
                let this = self.checked_this_binding(p, f)?;
                self.write(f, i.a(), this);
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
                self.write(f, i.a(), import_meta);
            }
            Op::InitializeThis => {
                let value = self.read(f, i.a());
                self.initialize_this_binding(f, value);
            }
            Op::CacheTemplateObject => {
                let key = (self.frames[f].program, self.frames[f].function, i.imm());
                let value = if let Some(value) = self.realm.template_objects.get(&key).copied() {
                    value
                } else {
                    let value = self.read(f, i.a());
                    self.realm.template_objects.insert(key, value);
                    value
                };
                self.write(f, i.a(), value);
            }
            Op::LoadCachedTemplateObject => {
                let key = (self.frames[f].program, self.frames[f].function, i.imm());
                let value = self
                    .realm
                    .template_objects
                    .get(&key)
                    .copied()
                    .unwrap_or(Value::UNDEFINED);
                self.write(f, i.a(), value);
            }
            Op::MakeClosure => {
                let env = self.promote_frame_environment(f);
                let module_root = p.module
                    && self.frames[f].function == super::ROOT_FUNCTION_ID
                    && self.programs.module_environment(self.frames[f].program) == Some(env);
                let v = if module_root {
                    self.function_values
                        .get(&(self.frames[f].program, i.closure_function_index()))
                        .and_then(|closures| {
                            closures
                                .iter()
                                .find(|(closure_env, _)| *closure_env == env)
                                .map(|(_, closure)| *closure)
                        })
                        .map(Ok)
                        .unwrap_or_else(|| self.closure(p, i.closure_function_index(), env))?
                } else {
                    self.closure(p, i.closure_function_index(), env)?
                };
                self.write(f, i.a(), v);
            }
            Op::MakeObject => {
                let v = self.object();
                self.write(f, i.a(), v);
            }
            Op::MakeObject2 => {
                let v = self.object_pair(
                    p,
                    i.object_site_index(),
                    self.read(f, i.b()),
                    self.read(f, i.c()),
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
                    object: Self::empty_object(self.array_proto),
                    elements: Rc::new(vec![Value::DELETED; i.array_length()]),
                });
                self.write(f, i.a(), v);
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
                    object: Self::empty_object(self.array_proto),
                    elements,
                });
                self.write(f, i.a(), v);
            }
            Op::GetField => {
                let lookup = i.field_lookup();
                let v = match lookup {
                    crate::bytecode::FieldLookup::Site(site) => self.resolve_field(p, f, site)?,
                    crate::bytecode::FieldLookup::Atom(atom) => {
                        let base = self.resolve_field_base(p, f, FieldBase(i.b()))?;
                        self.get_field_cached(p, base, atom, i.c())?
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
                let base = self.read(f, i.b());
                let key = self.read(f, i.c());
                let v = self.get_index(p, base, key)?;
                self.write(f, i.a(), v);
            }
            Op::CheckPrivate => {
                let object = self.read(f, i.a());
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
                let object = self.read(f, i.b());
                if !self.is_object_like(object) {
                    return Err(
                        self.type_error(p, "right-hand side of 'in' is not an object".into())
                    );
                }
                let result = self.has_private_brand(p, object, i.atom_index());
                self.write(f, i.a(), if result { Value::TRUE } else { Value::FALSE });
            }
            Op::MarkPrivateName => {
                let object = self.read(f, i.b());
                let home = self.read(f, i.c());
                let atom = i.atom_index();
                let brand = PrivateBrand { home, name: atom };
                if object != home {
                    let extensible = self.object_data(object).is_some_and(Object::is_extensible);
                    let already_branded = self
                        .object_data(object)
                        .is_some_and(|object| object.private_names.contains(&brand));
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
                if let Some(object) = self.object_data_mut(object)
                    && !object.private_names.contains(&brand)
                {
                    object.private_names.push(brand);
                }
            }
            Op::ResolveName => {
                let value = self.resolve_name(p, i.atom_index(), i.b() != 0)?;
                self.write(f, i.a(), value);
            }
            Op::LoadResolvedName => {
                let object = self.read(f, i.b());
                let value = self.load_resolved_name(p, object, i.atom_index(), i.c() != 0)?;
                self.write(f, i.a(), value);
            }
            Op::ValidateClassHeritage => {
                let heritage = self.read(f, i.a());
                if !heritage.is_null() && !self.is_constructable(p, heritage) {
                    return Err(self
                        .type_error(p, "Class extends value is not a constructor or null".into()));
                }
            }
            Op::DeleteName => {
                let value = self.delete_name(p, i.atom_index())?;
                self.write(f, i.a(), value);
            }
            Op::StoreResolvedName => {
                let object = self.read(f, i.b());
                let atom = i.atom_index();
                self.store_resolved_name(p, object, atom, self.read(f, i.a()), i.c() != 0)?;
            }
            Op::ToPropertyKey => {
                let value = self.to_property_key(p, self.read(f, i.b()))?;
                self.write(f, i.a(), value);
            }
            Op::ToNumeric => {
                let value = self.to_numeric(p, self.read(f, i.b()))?;
                self.write(f, i.a(), value);
            }
            Op::CopyDataProperties => self.copy_data_properties(
                p,
                self.read(f, i.a()),
                self.read(f, i.b()),
                self.read(f, i.c()),
            )?,
            Op::GetIterator => {
                let value = self.get_iterator(p, self.read(f, i.b()))?;
                self.write(f, i.a(), value);
            }
            Op::SpreadToArray => {
                let array = self.spread_to_array(p, self.read(f, i.b()))?;
                self.write(f, i.a(), array);
            }
            Op::RequireObjectCoercible => {
                self.require_object_coercible(p, self.read(f, i.b()))?;
            }
            Op::RequireIteratorResult => {
                if !self.is_object_like(self.read(f, i.b())) {
                    return Err(self.type_error(p, "iterator next result is not an object".into()));
                }
            }
            Op::SuperCallCheck => self.check_super_call(p)?,
            Op::IteratorClose => {
                self.iterator_close(p, self.read(f, i.b()))?;
            }
            Op::IteratorCleanupPush => self.frames[f].active_iterators.push(ActiveIterator {
                iterator: i.a(),
                done: i.b(),
            }),
            Op::IteratorCleanupPop => {
                self.frames[f]
                    .active_iterators
                    .pop()
                    .ok_or_else(|| JsError("iterator cleanup stack underflow".into()))?;
            }
            Op::SetFunctionName => {
                self.set_function_name(p, self.read(f, i.a()), i.atom_index())?;
            }
            Op::SetFunctionNameKey => {
                self.set_function_name_key(
                    self.read(f, i.a()),
                    self.read(f, i.b()),
                    i.function_name_prefix(),
                );
            }
            Op::InitializeTdz => {
                let slot = i.local_slot();
                if self.frames[f].captured {
                    let Some(Cell::Environment { slots, .. }) =
                        self.heap.get_mut(self.frames[f].env)
                    else {
                        return Err(JsError("invalid local environment".into()));
                    };
                    *slots
                        .get_mut(slot)
                        .ok_or_else(|| JsError("invalid local slot".into()))? = Value::DELETED;
                } else {
                    self.frames[f].locals[slot] = Value::DELETED;
                }
            }
            Op::GetAsyncIterator => {
                let value = self.get_async_iterator(p, self.read(f, i.b()))?;
                self.write(f, i.a(), value);
            }
            Op::Await => {
                return Ok(StepResult::Await {
                    value: self.read(f, i.b()),
                    destination: i.a(),
                });
            }
            Op::Yield => {
                return Ok(StepResult::Yield {
                    value: self.read(f, i.b()),
                    destination: i.a(),
                    delegated_result: None,
                });
            }
            Op::YieldStar => {
                let iterator = self.read(f, i.c());
                let iterator = if iterator.is_undefined() {
                    let source = self.read(f, i.b());
                    let asynchronous = p.functions[self.frames[f].function as usize].is_async;
                    let iterator = if asynchronous {
                        self.get_async_iterator(p, source)?
                    } else {
                        self.get_iterator(p, source)?
                    };
                    self.write(f, i.c(), iterator);
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
                    let input = self.read(f, i.a());
                    let result =
                        self.iterator_next_with_cached_method(p, iterator, next_method, &[input])?;
                    if asynchronous {
                        *pc -= 1;
                        return Ok(StepResult::Await {
                            value: result,
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
                    self.write(f, i.a(), value);
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
                        destination: i.a(),
                        delegated_result: Some(result),
                    });
                }
            }
            Op::SetField => {
                let object = self.read(f, i.b());
                let value = self.read(f, i.a());
                self.set_field_cached(
                    p,
                    object,
                    i.atom_index(),
                    value,
                    i.c(),
                    p.functions[self.frames[f].function as usize].strict,
                )?;
            }
            Op::DefineField => {
                let object = self.read(f, i.b());
                let value = self.read(f, i.a());
                self.define_class_field(p, object, PropertyKey::string(i.atom_index()), value)?;
            }
            Op::DefineComputedField => {
                let object = self.read(f, i.b());
                let key = self.read(f, i.c());
                let key = self.to_property_key(p, key)?;
                let key = match self.heap.get(key).cloned() {
                    Some(Cell::String(text)) => PropertyKey::string(self.intern_js_atom(&text)),
                    Some(Cell::Symbol(_)) => PropertyKey::symbol(key),
                    _ => return Err(JsError::validation("invalid class field key".into())),
                };
                self.define_class_field(p, object, key, self.read(f, i.a()))?;
            }
            Op::SetThisField => {
                let this = self.checked_this_binding(p, f)?;
                self.set_field_cached(
                    p,
                    this,
                    i.atom_index(),
                    self.read(f, i.a()),
                    i.c(),
                    p.functions[self.frames[f].function as usize].strict,
                )?;
            }
            Op::SetIndex => {
                #[cfg(feature = "profile-aggregate")]
                self.profile.index_dispatch(true, false);
                self.set_index_mode(
                    p,
                    self.read(f, i.b()),
                    self.read(f, i.c()),
                    self.read(f, i.a()),
                    p.functions[self.frames[f].function as usize].strict
                        || i.boolean_flag().expect("validated boolean immediate"),
                )?
            }
            Op::DefineArrayElement => self.define_array_literal_element(
                p,
                self.read(f, i.b()),
                i.array_index() as usize,
                self.read(f, i.a()),
            )?,
            Op::Move => self.write(f, i.a(), self.read(f, i.b())),
            Op::Binary | Op::NumericAdd | Op::NumericMultiply => {
                let operator = i.binary_operator();
                self.profile.binary(operator as usize, i.b(), i.c());
                let left = self.resolve_operand(p, f, Operand(i.b()))?;
                let right = self.resolve_operand(p, f, Operand(i.c()))?;
                let site_pc = *pc - 1;
                let armed = self.profile_regional_binary(f, site_pc, operator, left, right);
                let v = if armed {
                    match self.numeric_binary(operator, left, right) {
                        Some(value) => value,
                        None => {
                            self.deopt_numeric_site(f, site_pc);
                            self.binary(p, operator, left, right)?
                        }
                    }
                } else {
                    self.binary(p, operator, left, right)?
                };
                if i.returns_from_frame() {
                    return Ok(StepResult::Return(v));
                }
                self.write(f, i.result_register(), v);
            }
            Op::IncDec => {
                let input = self.read(f, i.b());
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
                self.write(f, i.a(), value);
            }
            Op::Unary => {
                let v = self.unary(p, i.unary_operator(), self.read(f, i.b()))?;
                self.write(f, i.a(), v);
            }
            Op::Delete => {
                let result =
                    self.delete_reference_property(p, self.read(f, i.b()), self.read(f, i.c()))?;
                if !self.truthy(result)
                    && (p.functions[self.frames[f].function as usize].strict
                        || i.boolean_flag().expect("validated boolean immediate"))
                {
                    return Err(self.type_error(p, "Cannot delete property in strict mode".into()));
                }
                self.write(f, i.a(), result);
            }
            Op::Jump => {
                *pc = i.jump_target() as usize;
                self.frames[f].pc = *pc;
                self.maybe_collect(p);
            }
            Op::JumpFalse => {
                let value = self.read(f, i.a());
                let truthy = self.truthy(value);
                #[cfg(feature = "profile-aggregate")]
                self.profile.branch_value(value.profile_kind(), truthy);
                if !truthy {
                    *pc = i.jump_target() as usize;
                }
            }
            Op::JumpBinaryFalse => {
                self.profile.binary(i.a() as usize, i.b(), i.c());
                let left = self.resolve_operand(p, f, Operand(i.b()))?;
                let right = self.resolve_operand(p, f, Operand(i.c()))?;
                if !self.binary_truthy(p, u32::from(i.a()), left, right)? {
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
                    CallArguments::from_values(
                        (0..window.count).map(|x| self.read(f, window.base + x)),
                    )
                };
                let this = self.read(f, i.c());
                let callee = self.read(f, i.b());
                let args = arguments.as_slice();
                self.frames[f].pc = *pc;
                let direct_eval = crate::bytecode::ImmediateLayout::direct_eval(i.imm())
                    && callee == self.native_value(Native::Eval);
                let parameter_eval =
                    direct_eval && crate::bytecode::ImmediateLayout::parameter_eval(i.imm());
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
                if terminal
                    && let Some(CallTarget::User(program_id, id, env)) =
                        self.call_target(callee).ok()
                    && program_id == self.frames[f].program
                    && !p.functions[id as usize].is_async
                    && !p.functions[id as usize].is_generator
                {
                    self.prepare_user_tail(p, f, id, env, this, args)?;
                    self.direct_eval = previous_direct_eval;
                    self.parameter_eval = previous_parameter_eval;
                    self.profile.terminal_call(0);
                    return Ok(StepResult::TailCall);
                }
                let value = match self.call_value(p, callee, this, args) {
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
                let n = window.count;
                let arguments =
                    CallArguments::from_values((0..n).map(|x| self.read(f, window.base + x)));
                let args = arguments.as_slice();
                let parent = self.capture_env(f, 0).unwrap_or(self.frames[f].env);
                self.profile.call_target(1, n as usize);
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
                if terminal
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
                )?;
                if i.returns_from_frame() {
                    self.profile.terminal_call(0);
                    return Ok(StepResult::Return(value));
                }
                self.write(f, i.result_register(), value);
            }
            Op::CallMethod => {
                self.profile.call_source(2);
                let this = self.read(f, i.b());
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
                        let arguments = CallArguments::from_values(
                            (0..window.count).map(|x| self.read(f, window.base + x)),
                        );
                        arguments.as_slice().to_vec()
                    }
                };
                self.frames[f].pc = *pc;
                let callee = self.read(f, i.b());
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
            Op::Return => return Ok(StepResult::Return(self.read(f, i.a()))),
            Op::Throw => {
                let value = self.read(f, i.a());
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
                if self.frames[frame].captured {
                    let Some(Cell::Environment { slots, .. }) =
                        self.heap.get(self.frames[frame].env)
                    else {
                        return Err(JsError("invalid local environment".into()));
                    };
                    Ok(slots[slot])
                } else {
                    Ok(self.frames[frame].locals[slot])
                }
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
