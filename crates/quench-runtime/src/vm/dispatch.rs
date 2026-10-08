use super::*;
use crate::heap::PrivateBrand;

fn wasm_gc_store_value(value: Value, field: crate::WasmGcField) -> Result<Value, JsError> {
    let Some(bits) = field.packed_bits else {
        return Ok(value);
    };
    let raw = value
        .as_int()
        .ok_or_else(|| JsError::validation("invalid packed Wasm GC field value".into()))?
        as u32;
    let mask = (1u32 << bits) - 1;
    Ok(Value::integer((raw & mask) as i32))
}

fn wasm_gc_load_value(value: Value, field: crate::WasmGcField, mode: u32) -> Value {
    let Some(bits) = field.packed_bits else {
        return value;
    };
    let raw = value.as_int().unwrap_or_default() as u32;
    let mask = (1u32 << bits) - 1;
    let raw = raw & mask;
    let result = if mode == 1 && raw & (1u32 << (bits - 1)) != 0 {
        (raw | !mask) as i32
    } else {
        raw as i32
    };
    Value::integer(result)
}

fn wasm_gc_array_length(value: Value) -> Result<usize, JsError> {
    let length = value
        .as_int()
        .ok_or_else(|| JsError::validation("invalid Wasm array length".into()))?;
    usize::try_from(length as u32)
        .map_err(|_| JsError::wasm_trap_error(crate::WasmTrap::ArrayOutOfBounds))
}

fn wasm_gc_data_width(field: crate::WasmGcField) -> Result<usize, JsError> {
    if let Some(bits) = field.packed_bits {
        return Ok(usize::from(bits / u8::BITS as u8));
    }
    match field.ty {
        crate::WasmType::I32 | crate::WasmType::F32 => Ok(std::mem::size_of::<u32>()),
        crate::WasmType::I64 | crate::WasmType::F64 => Ok(std::mem::size_of::<u64>()),
        crate::WasmType::V128 => Ok(std::mem::size_of::<u128>()),
        crate::WasmType::FuncRef
        | crate::WasmType::ExternRef
        | crate::WasmType::I31Ref
        | crate::WasmType::ExnRef => Err(JsError::validation(
            "array.new_data requires numeric elements".into(),
        )),
    }
}

fn wasm_gc_data_value(
    field: crate::WasmGcField,
    bytes: &[u8],
) -> Result<crate::WasmValue, JsError> {
    let width = wasm_gc_data_width(field)?;
    if bytes.len() != width {
        return Err(JsError::validation(
            "invalid Wasm GC data element width".into(),
        ));
    }
    let raw = bytes
        .iter()
        .enumerate()
        .fold(0u128, |value, (shift, byte)| {
            value | (u128::from(*byte) << (shift * u8::BITS as usize))
        });
    Ok(match field.ty {
        crate::WasmType::I32 => crate::WasmValue::I32(raw as u32 as i32),
        crate::WasmType::I64 => crate::WasmValue::I64(raw as u64 as i64),
        crate::WasmType::F32 => crate::WasmValue::F32(raw as u32),
        crate::WasmType::F64 => crate::WasmValue::F64(raw as u64),
        crate::WasmType::V128 => crate::WasmValue::V128(raw),
        crate::WasmType::FuncRef
        | crate::WasmType::ExternRef
        | crate::WasmType::I31Ref
        | crate::WasmType::ExnRef => {
            return Err(JsError::validation(
                "array.new_data requires numeric elements".into(),
            ));
        }
    })
}

fn wasm_gc_range(
    start: usize,
    length: usize,
    bound: usize,
    trap: crate::WasmTrap,
) -> Result<std::ops::Range<usize>, JsError> {
    let end = start
        .checked_add(length)
        .filter(|end| *end <= bound)
        .ok_or_else(|| JsError::wasm_trap_error(trap))?;
    Ok(start..end)
}

fn wasm_gc_data_values(
    field: crate::WasmGcField,
    data: &[u8],
    source: usize,
    length: usize,
) -> Result<Vec<crate::WasmValue>, JsError> {
    let width = wasm_gc_data_width(field)?;
    let byte_length = length
        .checked_mul(width)
        .ok_or_else(|| JsError::wasm_trap_error(crate::WasmTrap::MemoryOutOfBounds))?;
    let range = wasm_gc_range(
        source,
        byte_length,
        data.len(),
        crate::WasmTrap::MemoryOutOfBounds,
    )?;
    data[range]
        .chunks_exact(width)
        .map(|bytes| wasm_gc_data_value(field, bytes))
        .collect()
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
    ) -> Result<StepResult, JsError> {
        self.frames[f].binding_site_pc = Some(*pc as u32);
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
                if self.frames[f].captured {
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
                if self.frames[f].captured {
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
                let v = self.load_name(p, i.atom_index(), Some(i.cache_site_index()))?;
                self.write(f, i.result_register(), v);
            }
            Op::LoadNameCall => {
                let (callee, this) =
                    self.load_name_call(p, i.atom_index(), Some(i.cache_site_index()), false)?;
                self.write(f, i.result_register(), callee);
                self.write(f, i.register_b(), this);
            }
            Op::LoadNameTypeof => {
                let v = self.load_name_typeof(p, i.atom_index(), i.cache_site_index())?;
                self.write(f, i.result_register(), v);
            }
            Op::StoreName => self.store_name(
                p,
                i.atom_index(),
                self.read(f, i.register_a()),
                i.cache_site_index(),
                i.boolean_field(crate::bytecode::InstructionField::B)
                    .expect("validated initialization flag"),
            )?,
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
                let env = self.capture_binding_environment(f)?;
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
                let value = self.resolve_name(p, i.atom_index(), strict)?;
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
                let value = self.delete_name(p, i.atom_index())?;
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
                if self.frames[f].captured {
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
            Op::WasmUnreachable => {
                return Err(JsError::wasm_trap_error(crate::WasmTrap::Unreachable));
            }
            Op::WasmThrow => {
                let module = self.active_wasm_module.ok_or_else(|| {
                    JsError::validation("Wasm exception has no active module".into())
                })?;
                let (tag, signature) = self
                    .wasm_modules
                    .get(module.raw() as usize)
                    .and_then(|stored| {
                        Some((
                            *stored.tags.get(i.imm() as usize)?,
                            stored.tag_signatures.get(i.imm() as usize)?.clone(),
                        ))
                    })
                    .ok_or_else(|| {
                        JsError::validation("Wasm exception tag out of bounds".into())
                    })?;
                let base = i.register_b();
                let values = signature
                    .params
                    .iter()
                    .enumerate()
                    .map(|(offset, ty)| {
                        self.decode_wasm_scalar(self.read(f, base + offset as u16), *ty)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                return Err(JsError::wasm_exception_error(tag, values));
            }
            Op::WasmThrowRef => {
                let exception_reference = self.read(f, i.register_b());
                if exception_reference == Value::NULL {
                    return Err(JsError::wasm_trap_error(crate::WasmTrap::NullReference));
                }
                let Some(Cell::WasmExceptionRef { tag, payload }) =
                    self.heap.get(exception_reference).cloned()
                else {
                    return Err(JsError::validation(
                        "invalid Wasm exception reference".into(),
                    ));
                };
                let signature = self
                    .wasm_modules
                    .get(tag.module.raw() as usize)
                    .and_then(|stored| stored.tag_signatures.get(tag.index as usize))
                    .cloned()
                    .ok_or_else(|| {
                        JsError::validation("Wasm exception tag out of bounds".into())
                    })?;
                if payload.len() != signature.params.len() {
                    return Err(JsError::validation(
                        "Wasm exception reference payload count mismatch".into(),
                    ));
                }
                let values = payload
                    .into_iter()
                    .zip(signature.params)
                    .map(|(value, ty)| self.decode_wasm_scalar(value, ty))
                    .collect::<Result<Vec<_>, _>>()?;
                return Err(JsError::wasm_exception_ref_error(
                    tag,
                    values,
                    exception_reference,
                ));
            }
            Op::WasmGlobalGet => {
                let module = self.active_wasm_module.ok_or_else(|| {
                    JsError::validation("Wasm global access has no active module".into())
                })?;
                let value = self
                    .wasm_global(module, i.imm())
                    .ok_or_else(|| JsError::validation("Wasm global index out of bounds".into()))?;
                let value = self.encode_wasm_scalar(value);
                self.write(f, i.result_register(), value);
            }
            Op::WasmGlobalSet => {
                let module = self.active_wasm_module.ok_or_else(|| {
                    JsError::validation("Wasm global access has no active module".into())
                })?;
                let (initial_value, mutable) = self
                    .wasm_global_id(module, i.imm())
                    .and_then(|global| self.wasm_globals.get(global.raw() as usize).copied())
                    .ok_or_else(|| JsError::validation("Wasm global index out of bounds".into()))?;
                if !mutable {
                    return Err(JsError::validation("write to immutable Wasm global".into()));
                }
                let value =
                    self.decode_wasm_scalar(self.read(f, i.register_a()), initial_value.ty())?;
                let global_id = self
                    .wasm_global_id(module, i.imm())
                    .ok_or_else(|| JsError::validation("Wasm global index out of bounds".into()))?;
                let (global, _) = self
                    .wasm_globals
                    .get_mut(global_id.raw() as usize)
                    .ok_or_else(|| JsError::validation("Wasm global index out of bounds".into()))?;
                *global = value;
            }
            Op::WasmMemoryLoad => {
                let module = self.active_wasm_module.ok_or_else(|| {
                    JsError::validation("Wasm memory access has no active module".into())
                })?;
                let selector = i.field_value(crate::bytecode::InstructionField::C);
                let kind = crate::WasmMemoryAccessKind::from_tag(selector & 31)
                    .filter(|kind| !kind.is_store())
                    .ok_or_else(|| JsError::validation("invalid Wasm memory load kind".into()))?;
                let memory_index = u32::from(selector >> 5);
                let address =
                    self.wasm_memory_operand(module, memory_index, self.read(f, i.register_b()))?;
                let value = self.wasm_memory_load(module, memory_index, address, i.imm(), kind)?;
                let value = self.encode_wasm_scalar(value);
                self.write(f, i.result_register(), value);
            }
            Op::WasmMemoryStore => {
                let module = self.active_wasm_module.ok_or_else(|| {
                    JsError::validation("Wasm memory access has no active module".into())
                })?;
                let selector = i.field_value(crate::bytecode::InstructionField::C);
                let kind = crate::WasmMemoryAccessKind::from_tag(selector & 31)
                    .filter(|kind| kind.is_store())
                    .ok_or_else(|| JsError::validation("invalid Wasm memory store kind".into()))?;
                let memory_index = u32::from(selector >> 5);
                let address =
                    self.wasm_memory_operand(module, memory_index, self.read(f, i.register_b()))?;
                self.wasm_memory_store(
                    module,
                    memory_index,
                    address,
                    i.imm(),
                    kind,
                    self.read(f, i.register_a()),
                )?;
            }
            Op::WasmMemorySize => {
                let module = self.active_wasm_module.ok_or_else(|| {
                    JsError::validation("Wasm memory access has no active module".into())
                })?;
                let (pages, memory64) = self
                    .wasm_memory_size(module, i.imm())
                    .ok_or_else(|| JsError::validation("Wasm memory index out of bounds".into()))?;
                let value = if memory64 {
                    self.encode_wasm_scalar(crate::WasmValue::I64(pages as i64))
                } else {
                    Value::integer(pages as i32)
                };
                self.write(f, i.result_register(), value);
            }
            Op::WasmMemoryGrow => {
                let module = self.active_wasm_module.ok_or_else(|| {
                    JsError::validation("Wasm memory access has no active module".into())
                })?;
                let delta =
                    self.wasm_memory_operand(module, i.imm(), self.read(f, i.register_b()))?;
                let (previous, memory64) = self
                    .wasm_memory_grow(module, i.imm(), delta)
                    .ok_or_else(|| JsError::validation("Wasm memory index out of bounds".into()))?;
                let value = if memory64 {
                    self.encode_wasm_scalar(crate::WasmValue::I64(previous as i64))
                } else {
                    Value::integer(previous as u32 as i32)
                };
                self.write(f, i.result_register(), value);
            }
            Op::WasmTableGet => {
                let module = self.active_wasm_module.ok_or_else(|| {
                    JsError::validation("Wasm table access has no active module".into())
                })?;
                let index =
                    self.wasm_table_operand(module, i.imm(), self.read(f, i.register_b()))?;
                let table = *self
                    .wasm_modules
                    .get(module.raw() as usize)
                    .and_then(|module| module.tables.get(i.imm() as usize))
                    .ok_or_else(|| JsError::validation("Wasm table index out of bounds".into()))?;
                let value = {
                    let table = self
                        .wasm_tables
                        .get(table.raw() as usize)
                        .ok_or_else(|| {
                            JsError::validation("Wasm table index out of bounds".into())
                        })?
                        .borrow();
                    table.get(index).ok_or_else(|| {
                        JsError::wasm_trap_error(crate::WasmTrap::TableOutOfBounds)
                    })?
                };
                let value = self.encode_wasm_scalar(value);
                self.write(f, i.result_register(), value);
            }
            Op::WasmTableSet => {
                let module = self.active_wasm_module.ok_or_else(|| {
                    JsError::validation("Wasm table access has no active module".into())
                })?;
                let index =
                    self.wasm_table_operand(module, i.imm(), self.read(f, i.register_a()))?;
                let table = *self
                    .wasm_modules
                    .get(module.raw() as usize)
                    .and_then(|module| module.tables.get(i.imm() as usize))
                    .ok_or_else(|| JsError::validation("Wasm table index out of bounds".into()))?;
                let element_type = self
                    .wasm_tables
                    .get(table.raw() as usize)
                    .ok_or_else(|| JsError::validation("Wasm table index out of bounds".into()))?
                    .borrow()
                    .element_type;
                let reference =
                    self.decode_wasm_scalar(self.read(f, i.register_b()), element_type)?;
                let table = self
                    .wasm_tables
                    .get(table.raw() as usize)
                    .cloned()
                    .ok_or_else(|| JsError::validation("Wasm table index out of bounds".into()))?;
                let mut table = table.borrow_mut();
                table.set(index, reference)?;
            }
            Op::WasmTableSize => {
                let module = self.active_wasm_module.ok_or_else(|| {
                    JsError::validation("Wasm table access has no active module".into())
                })?;
                let (size, table64) = self
                    .wasm_table_size(module, i.imm())
                    .ok_or_else(|| JsError::validation("Wasm table index out of bounds".into()))?;
                let value = if table64 {
                    self.encode_wasm_scalar(crate::WasmValue::I64(size as i64))
                } else {
                    Value::integer(size as u32 as i32)
                };
                self.write(f, i.result_register(), value);
            }
            Op::WasmTableGrow => {
                let module = self.active_wasm_module.ok_or_else(|| {
                    JsError::validation("Wasm table access has no active module".into())
                })?;
                let table_index = self
                    .wasm_modules
                    .get(module.raw() as usize)
                    .and_then(|module| module.tables.get(i.imm() as usize))
                    .copied()
                    .ok_or_else(|| JsError::validation("Wasm table index out of bounds".into()))?;
                let element_type = self
                    .wasm_tables
                    .get(table_index.raw() as usize)
                    .ok_or_else(|| JsError::validation("Wasm table index out of bounds".into()))?
                    .borrow()
                    .element_type;
                let delta =
                    self.wasm_table_operand(module, i.imm(), self.read(f, i.register_b()))?;
                let initial =
                    self.decode_wasm_scalar(self.read(f, i.register_c()), element_type)?;
                let (previous, table64) = self
                    .wasm_table_grow(module, i.imm(), delta, initial)
                    .ok_or_else(|| JsError::validation("Wasm table index out of bounds".into()))?;
                let value = if table64 {
                    self.encode_wasm_scalar(crate::WasmValue::I64(previous as i64))
                } else {
                    Value::integer(previous as u32 as i32)
                };
                self.write(f, i.result_register(), value);
            }
            Op::WasmTableFill => {
                let module = self.active_wasm_module.ok_or_else(|| {
                    JsError::validation("Wasm table fill has no active module".into())
                })?;
                let table = *self
                    .wasm_modules
                    .get(module.raw() as usize)
                    .and_then(|module| module.tables.get(i.imm() as usize))
                    .ok_or_else(|| JsError::validation("Wasm table index out of bounds".into()))?;
                let element_type = self
                    .wasm_tables
                    .get(table.raw() as usize)
                    .ok_or_else(|| JsError::validation("Wasm table index out of bounds".into()))?
                    .borrow()
                    .element_type;
                let destination =
                    self.wasm_table_operand(module, i.imm(), self.read(f, i.register_a()))?;
                let value = self.decode_wasm_scalar(self.read(f, i.register_b()), element_type)?;
                let length =
                    self.wasm_table_operand(module, i.imm(), self.read(f, i.register_c()))?;
                self.wasm_table_fill(module, i.imm(), destination, value, length)?;
            }
            Op::WasmTableCopy => {
                let module = self.active_wasm_module.ok_or_else(|| {
                    JsError::validation("Wasm table copy has no active module".into())
                })?;
                let destination_table = i.imm() & 0xffff;
                let source_table = i.imm() >> 16;
                let destination = self.wasm_table_operand(
                    module,
                    destination_table,
                    self.read(f, i.register_a()),
                )?;
                let source =
                    self.wasm_table_operand(module, source_table, self.read(f, i.register_b()))?;
                let length = self.wasm_table_operand(
                    module,
                    destination_table,
                    self.read(f, i.register_c()),
                )?;
                self.wasm_table_copy(
                    module,
                    destination_table,
                    source_table,
                    destination,
                    source,
                    length,
                )?;
            }
            Op::WasmTableInit => {
                let module = self.active_wasm_module.ok_or_else(|| {
                    JsError::validation("Wasm table.init has no active module".into())
                })?;
                let table_index = i.imm() >> 16;
                let destination =
                    self.wasm_table_operand(module, table_index, self.read(f, i.register_a()))?;
                let source = self
                    .read(f, i.register_b())
                    .as_int()
                    .ok_or_else(|| JsError::validation("invalid Wasm element index".into()))?
                    as u32;
                let length = self
                    .read(f, i.register_c())
                    .as_int()
                    .ok_or_else(|| JsError::validation("invalid Wasm table length".into()))?
                    as u32;
                self.wasm_table_init(
                    module,
                    table_index,
                    i.imm() & 0xffff,
                    destination,
                    source,
                    length,
                )?;
            }
            Op::WasmElemDrop => {
                let module = self.active_wasm_module.ok_or_else(|| {
                    JsError::validation("Wasm elem.drop has no active module".into())
                })?;
                self.wasm_element_drop(module, i.imm())?;
            }
            Op::WasmV128 => {
                let module = self.active_wasm_module.ok_or_else(|| {
                    JsError::validation("Wasm SIMD operation has no active module".into())
                })?;
                let value = self.execute_wasm_v128(
                    module,
                    i.imm(),
                    self.read(f, i.result_register()),
                    self.read(f, i.register_b()),
                    self.read(f, i.register_c()),
                )?;
                self.write(f, i.result_register(), value);
            }
            Op::WasmV128MemoryLoad => {
                let module = self.active_wasm_module.ok_or_else(|| {
                    JsError::validation("Wasm SIMD load has no active module".into())
                })?;
                let site_index = i.imm();
                let value = self.wasm_v128_memory_load(
                    module,
                    site_index,
                    self.read(f, i.register_b()),
                    self.read(f, i.register_c()),
                )?;
                let value = self.encode_wasm_scalar(value);
                self.write(f, i.result_register(), value);
            }
            Op::WasmV128MemoryStore => {
                let module = self.active_wasm_module.ok_or_else(|| {
                    JsError::validation("Wasm SIMD store has no active module".into())
                })?;
                self.wasm_v128_memory_store(
                    module,
                    i.imm(),
                    self.read(f, i.register_a()),
                    self.read(f, i.register_b()),
                )?;
            }
            Op::WasmRefIsNull => {
                let value = self.read(f, i.register_b());
                self.write(
                    f,
                    i.result_register(),
                    Value::integer(i32::from(value == Value::NULL)),
                );
            }
            Op::WasmRefAsNonNull => {
                let value = self.read(f, i.register_b());
                if value == Value::NULL {
                    return Err(JsError::wasm_trap_error(crate::WasmTrap::NullReference));
                }
                self.write(f, i.result_register(), value);
            }
            Op::WasmRefEq => {
                let left = self.read(f, i.register_b());
                let right = self.read(f, i.register_c());
                let equal = match (self.heap.get(left), self.heap.get(right)) {
                    (Some(Cell::WasmBits64(left)), Some(Cell::WasmBits64(right))) => left == right,
                    _ => left == right,
                };
                self.write(f, i.result_register(), Value::integer(i32::from(equal)));
            }
            Op::WasmRefI31 => {
                let value = self
                    .read(f, i.register_b())
                    .as_int()
                    .ok_or_else(|| JsError::validation("invalid Wasm i31 operand".into()))?;
                let reference = self
                    .heap
                    .alloc(Cell::WasmBits64(u64::from(value as u32 & 0x7fff_ffff)));
                self.write(f, i.result_register(), reference);
            }
            Op::WasmMultiValuePack => {
                let count = usize::try_from(i.imm())
                    .map_err(|_| JsError::validation("invalid Wasm result count".into()))?;
                let values = (0..count)
                    .map(|offset| {
                        let register = i.register_b().checked_add(u16::try_from(offset).ok()?)?;
                        Some(self.read(f, register))
                    })
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| {
                        JsError::validation("Wasm result register out of bounds".into())
                    })?;
                let value = self.heap.alloc(Cell::WasmMultiValue(values));
                self.write(f, i.result_register(), value);
            }
            Op::WasmMultiValueGet => {
                let value = self.read(f, i.register_b());
                let Some(Cell::WasmMultiValue(values)) = self.heap.get(value) else {
                    return Err(JsError::validation(
                        "invalid Wasm multi-value result".into(),
                    ));
                };
                let value = values
                    .get(i.imm() as usize)
                    .copied()
                    .ok_or_else(|| JsError::validation("Wasm result index out of bounds".into()))?;
                self.write(f, i.result_register(), value);
            }
            Op::WasmI31Get => {
                let reference = self.read(f, i.register_b());
                if reference == Value::NULL {
                    return Err(JsError::wasm_trap_error(crate::WasmTrap::NullI31Reference));
                }
                let Some(Cell::WasmBits64(bits)) = self.heap.get(reference) else {
                    return Err(JsError::validation("invalid Wasm i31 reference".into()));
                };
                let bits = *bits as u32 & 0x7fff_ffff;
                let value = if i.imm() == 1 {
                    bits as i32
                } else if bits & 0x4000_0000 != 0 {
                    (bits | 0x8000_0000) as i32
                } else {
                    bits as i32
                };
                self.write(f, i.result_register(), Value::integer(value));
            }
            Op::WasmRefGetDesc => {
                let reference = self.read(f, i.register_b());
                if reference == Value::NULL {
                    return Err(JsError::wasm_trap_error(crate::WasmTrap::NullReference));
                }
                let module = self.active_wasm_module.ok_or_else(|| {
                    JsError::validation("Wasm descriptor lookup has no active module".into())
                })?;
                let descriptor_type = self
                    .wasm_modules
                    .get(module.raw() as usize)
                    .and_then(|module| module.gc_descriptors.get(i.imm() as usize))
                    .and_then(|metadata| metadata.descriptor_type)
                    .ok_or_else(|| JsError::validation("Wasm type has no descriptor".into()))?;
                let _ = descriptor_type;
                let raw = reference
                    .as_int()
                    .map(|value| value as u32)
                    .ok_or_else(|| JsError::wasm_trap_error(crate::WasmTrap::CastFailure))?;
                let descriptor = match self.wasm_gc_objects.get(&raw) {
                    Some(crate::vm::wasm::WasmGcObject::Struct { descriptor, .. }) => *descriptor,
                    Some(crate::vm::wasm::WasmGcObject::Array { .. }) => {
                        return Err(JsError::wasm_trap_error(crate::WasmTrap::CastFailure));
                    }
                    None => return Err(JsError::wasm_trap_error(crate::WasmTrap::CastFailure)),
                };
                self.write(f, i.result_register(), descriptor);
            }
            Op::WasmRefTestDescEq => {
                let reference = self.read(f, i.register_b());
                let descriptor = self.read(f, i.register_c());
                if descriptor == Value::NULL {
                    return Err(JsError::wasm_trap_error(
                        crate::WasmTrap::NullDescriptorReference,
                    ));
                }
                let type_matches = self.wasm_reference_matches(reference, i.imm());
                let descriptor_matches = if reference == Value::NULL {
                    true
                } else if type_matches {
                    let raw = reference.as_int().map(|value| value as u32);
                    raw.and_then(|raw| self.wasm_gc_objects.get(&raw))
                        .is_some_and(|object| match object {
                            crate::vm::wasm::WasmGcObject::Struct {
                                descriptor: actual, ..
                            } => *actual == descriptor,
                            crate::vm::wasm::WasmGcObject::Array { .. } => false,
                        })
                } else {
                    false
                };
                self.write(
                    f,
                    i.result_register(),
                    Value::integer(i32::from(type_matches && descriptor_matches)),
                );
            }
            Op::WasmRefCastDescEq => {
                let reference = self.read(f, i.register_b());
                let descriptor = self.read(f, i.register_c());
                if descriptor == Value::NULL {
                    return Err(JsError::wasm_trap_error(
                        crate::WasmTrap::NullDescriptorReference,
                    ));
                }
                let type_matches = self.wasm_reference_matches(reference, i.imm());
                let descriptor_matches = if reference == Value::NULL {
                    true
                } else if type_matches {
                    reference
                        .as_int()
                        .map(|value| value as u32)
                        .and_then(|raw| self.wasm_gc_objects.get(&raw))
                        .is_some_and(|object| match object {
                            crate::vm::wasm::WasmGcObject::Struct {
                                descriptor: actual, ..
                            } => *actual == descriptor,
                            crate::vm::wasm::WasmGcObject::Array { .. } => false,
                        })
                } else {
                    false
                };
                if !type_matches || !descriptor_matches {
                    return Err(JsError::wasm_trap_error(crate::WasmTrap::CastFailure));
                }
                self.write(f, i.result_register(), reference);
            }
            Op::WasmGcAlloc => {
                let kind = i.imm() & crate::wasm::WASM_GC_ALLOC_KIND_MASK;
                let (type_index, segment_index) = match kind {
                    crate::wasm::WASM_GC_ALLOC_ARRAY_DATA
                    | crate::wasm::WASM_GC_ALLOC_ARRAY_ELEM => (
                        i.imm() >> crate::wasm::WASM_GC_ALLOC_SEGMENT_TYPE_SHIFT,
                        Some(
                            (i.imm() >> crate::wasm::WASM_GC_ALLOC_SEGMENT_INDEX_SHIFT)
                                & crate::wasm::WASM_GC_ALLOC_SEGMENT_INDEX_MASK,
                        ),
                    ),
                    _ => (i.imm() >> crate::wasm::WASM_GC_ALLOC_KIND_BITS, None),
                };
                let module = self.active_wasm_module.ok_or_else(|| {
                    JsError::validation("Wasm GC allocation has no active module".into())
                })?;
                let gc_type = self
                    .wasm_modules
                    .get(module.raw() as usize)
                    .and_then(|module| module.gc_types.get(type_index as usize))
                    .and_then(Option::as_ref)
                    .cloned()
                    .ok_or_else(|| {
                        JsError::validation("Wasm GC type index out of bounds".into())
                    })?;
                let (class, object) = match (kind, gc_type) {
                    (
                        crate::wasm::WASM_GC_ALLOC_STRUCT
                        | crate::wasm::WASM_GC_ALLOC_STRUCT_DEFAULT
                        | crate::wasm::WASM_GC_ALLOC_STRUCT_DESC
                        | crate::wasm::WASM_GC_ALLOC_STRUCT_DEFAULT_DESC,
                        crate::WasmGcType::Struct(fields),
                    ) => {
                        let defaults = matches!(
                            kind,
                            crate::wasm::WASM_GC_ALLOC_STRUCT_DEFAULT
                                | crate::wasm::WASM_GC_ALLOC_STRUCT_DEFAULT_DESC
                        );
                        let descriptor = match kind {
                            crate::wasm::WASM_GC_ALLOC_STRUCT_DESC => self.read(f, i.register_c()),
                            crate::wasm::WASM_GC_ALLOC_STRUCT_DEFAULT_DESC => {
                                self.read(f, i.register_b())
                            }
                            _ => Value::NULL,
                        };
                        if matches!(
                            kind,
                            crate::wasm::WASM_GC_ALLOC_STRUCT_DESC
                                | crate::wasm::WASM_GC_ALLOC_STRUCT_DEFAULT_DESC
                        ) && descriptor == Value::NULL
                        {
                            return Err(JsError::wasm_trap_error(
                                crate::WasmTrap::NullDescriptorReference,
                            ));
                        }
                        let mut values = Vec::new();
                        values.try_reserve_exact(fields.len()).map_err(|_| {
                            JsError::wasm_trap_error(crate::WasmTrap::ArrayOutOfBounds)
                        })?;
                        for (offset, field) in fields.into_iter().enumerate() {
                            let value = if defaults {
                                self.encode_wasm_scalar(field.ty.zero())
                            } else {
                                let source = usize::from(i.register_b()) + offset;
                                let source = u16::try_from(source).map_err(|_| {
                                    JsError::validation(
                                        "Wasm struct fields exceed registers".into(),
                                    )
                                })?;
                                self.read(f, source)
                            };
                            values.push(wasm_gc_store_value(value, field)?);
                        }
                        (
                            crate::wasm::WASM_GC_REFERENCE_STRUCT_CLASS,
                            crate::vm::wasm::WasmGcObject::Struct {
                                module_index: module.raw(),
                                type_index,
                                values,
                                descriptor,
                            },
                        )
                    }
                    (
                        crate::wasm::WASM_GC_ALLOC_ARRAY_DEFAULT
                        | crate::wasm::WASM_GC_ALLOC_ARRAY
                        | crate::wasm::WASM_GC_ALLOC_ARRAY_FIXED
                        | crate::wasm::WASM_GC_ALLOC_ARRAY_DATA
                        | crate::wasm::WASM_GC_ALLOC_ARRAY_ELEM,
                        crate::WasmGcType::Array(field),
                    ) => {
                        let length = wasm_gc_array_length(self.read(f, i.register_c()))?;
                        let mut values = Vec::new();
                        values.try_reserve_exact(length).map_err(|_| {
                            JsError::wasm_trap_error(crate::WasmTrap::ArrayOutOfBounds)
                        })?;
                        match kind {
                            crate::wasm::WASM_GC_ALLOC_ARRAY_DEFAULT => {
                                values.resize(length, self.encode_wasm_scalar(field.ty.zero()));
                            }
                            crate::wasm::WASM_GC_ALLOC_ARRAY => {
                                let initial =
                                    wasm_gc_store_value(self.read(f, i.register_b()), field)?;
                                values.resize(length, initial);
                            }
                            crate::wasm::WASM_GC_ALLOC_ARRAY_FIXED => {
                                for offset in 0..length {
                                    let source = usize::from(i.register_b()) + offset;
                                    let source = u16::try_from(source).map_err(|_| {
                                        JsError::validation(
                                            "Wasm array fields exceed registers".into(),
                                        )
                                    })?;
                                    values.push(wasm_gc_store_value(self.read(f, source), field)?);
                                }
                            }
                            crate::wasm::WASM_GC_ALLOC_ARRAY_DATA => {
                                let segment_index = segment_index.ok_or_else(|| {
                                    JsError::validation("missing Wasm data segment index".into())
                                })?;
                                let source = wasm_gc_array_length(self.read(f, i.register_b()))?;
                                let width = wasm_gc_data_width(field)?;
                                let byte_length = length.checked_mul(width).ok_or_else(|| {
                                    JsError::wasm_trap_error(crate::WasmTrap::MemoryOutOfBounds)
                                })?;
                                let end = source.checked_add(byte_length).ok_or_else(|| {
                                    JsError::wasm_trap_error(crate::WasmTrap::MemoryOutOfBounds)
                                })?;
                                let segment = self
                                    .wasm_modules
                                    .get(module.raw() as usize)
                                    .and_then(|module| {
                                        module.data_segments.get(segment_index as usize)
                                    })
                                    .ok_or_else(|| {
                                        JsError::wasm_trap_error(crate::WasmTrap::MemoryOutOfBounds)
                                    })?;
                                let initial_values = match segment {
                                    Some(bytes) if length == 0 => {
                                        wasm_gc_range(
                                            source,
                                            0,
                                            bytes.len(),
                                            crate::WasmTrap::MemoryOutOfBounds,
                                        )?;
                                        Vec::new()
                                    }
                                    Some(bytes) => {
                                        let slice = bytes.get(source..end).ok_or_else(|| {
                                            JsError::wasm_trap_error(
                                                crate::WasmTrap::MemoryOutOfBounds,
                                            )
                                        })?;
                                        slice
                                            .chunks_exact(width)
                                            .map(|bytes| wasm_gc_data_value(field, bytes))
                                            .collect::<Result<Vec<_>, _>>()?
                                    }
                                    None if length == 0 => {
                                        wasm_gc_range(
                                            source,
                                            0,
                                            0,
                                            crate::WasmTrap::MemoryOutOfBounds,
                                        )?;
                                        Vec::new()
                                    }
                                    None => {
                                        return Err(JsError::wasm_trap_error(
                                            crate::WasmTrap::MemoryOutOfBounds,
                                        ));
                                    }
                                };
                                for value in initial_values {
                                    values.push(self.encode_wasm_scalar(value));
                                }
                            }
                            crate::wasm::WASM_GC_ALLOC_ARRAY_ELEM => {
                                let segment_index = segment_index.ok_or_else(|| {
                                    JsError::validation("missing Wasm element segment index".into())
                                })?;
                                let source = wasm_gc_array_length(self.read(f, i.register_b()))?;
                                let segment = self
                                    .wasm_modules
                                    .get(module.raw() as usize)
                                    .and_then(|module| {
                                        module.element_segments.get(segment_index as usize)
                                    })
                                    .ok_or_else(|| {
                                        JsError::wasm_trap_error(crate::WasmTrap::TableOutOfBounds)
                                    })?;
                                let initial_values = match segment.values.as_ref() {
                                    Some(elements) if length == 0 => {
                                        wasm_gc_range(
                                            source,
                                            0,
                                            elements.len(),
                                            crate::WasmTrap::TableOutOfBounds,
                                        )?;
                                        Vec::new()
                                    }
                                    Some(elements) => {
                                        let range = wasm_gc_range(
                                            source,
                                            length,
                                            elements.len(),
                                            crate::WasmTrap::TableOutOfBounds,
                                        )?;
                                        elements[range].to_vec()
                                    }
                                    None if length == 0 => {
                                        wasm_gc_range(
                                            source,
                                            0,
                                            0,
                                            crate::WasmTrap::TableOutOfBounds,
                                        )?;
                                        Vec::new()
                                    }
                                    None => {
                                        return Err(JsError::wasm_trap_error(
                                            crate::WasmTrap::TableOutOfBounds,
                                        ));
                                    }
                                };
                                for value in initial_values {
                                    values.push(self.encode_wasm_scalar(value));
                                }
                            }
                            _ => unreachable!("array kind checked by match"),
                        }
                        (
                            crate::wasm::WASM_GC_REFERENCE_ARRAY_CLASS,
                            crate::vm::wasm::WasmGcObject::Array {
                                module_index: module.raw(),
                                type_index,
                                values,
                            },
                        )
                    }
                    _ => {
                        return Err(JsError::validation(
                            "invalid Wasm GC allocation kind".into(),
                        ));
                    }
                };
                let reference = (class << crate::wasm::WASM_GC_REFERENCE_CLASS_SHIFT)
                    | (self.wasm_gc_next_ref & crate::wasm::WASM_GC_REFERENCE_ID_MASK);
                self.wasm_gc_next_ref = self.wasm_gc_next_ref.wrapping_add(1).max(1);
                self.wasm_gc_ref_types.insert(reference, type_index);
                self.wasm_gc_objects.insert(reference, object);
                self.write(f, i.result_register(), Value::integer(reference as i32));
            }
            Op::WasmGcAccess => {
                let selector = i.imm() >> crate::wasm::WASM_GC_ACCESS_SELECTOR_SHIFT;
                let field_index = (i.imm() & crate::wasm::WASM_GC_ACCESS_FIELD_MASK) as usize;
                match selector {
                    crate::wasm::WASM_GC_ACCESS_STRUCT_GET
                    | crate::wasm::WASM_GC_ACCESS_STRUCT_GET_S
                    | crate::wasm::WASM_GC_ACCESS_STRUCT_GET_U => {
                        let reference = self.read(f, i.register_b());
                        if reference == Value::NULL {
                            return Err(JsError::wasm_trap_error(
                                crate::WasmTrap::NullStructReference,
                            ));
                        }
                        let raw =
                            reference
                                .as_int()
                                .map(|value| value as u32)
                                .ok_or_else(|| {
                                    JsError::wasm_trap_error(crate::WasmTrap::CastFailure)
                                })?;
                        let field = self.wasm_gc_field(raw, field_index, false)?;
                        let value = self
                            .wasm_gc_object_values(raw, false)?
                            .get(field_index)
                            .copied()
                            .ok_or_else(|| {
                                JsError::validation("Wasm struct field is out of bounds".into())
                            })?;
                        let mode = selector - crate::wasm::WASM_GC_ACCESS_STRUCT_GET;
                        self.write(f, i.register_a(), wasm_gc_load_value(value, field, mode));
                    }
                    crate::wasm::WASM_GC_ACCESS_ARRAY_GET
                    | crate::wasm::WASM_GC_ACCESS_ARRAY_GET_S
                    | crate::wasm::WASM_GC_ACCESS_ARRAY_GET_U => {
                        let reference = self.read(f, i.register_b());
                        if reference == Value::NULL {
                            return Err(JsError::wasm_trap_error(
                                crate::WasmTrap::NullArrayReference,
                            ));
                        }
                        let raw =
                            reference
                                .as_int()
                                .map(|value| value as u32)
                                .ok_or_else(|| {
                                    JsError::wasm_trap_error(crate::WasmTrap::CastFailure)
                                })?;
                        let field = self.wasm_gc_field(raw, 0, true)?;
                        let index = self.read(f, i.register_c()).as_int().unwrap_or_default() as u32
                            as usize;
                        let value = self
                            .wasm_gc_object_values(raw, true)?
                            .get(index)
                            .copied()
                            .ok_or_else(|| {
                                JsError::wasm_trap_error(crate::WasmTrap::ArrayOutOfBounds)
                            })?;
                        let mode = selector - crate::wasm::WASM_GC_ACCESS_ARRAY_GET;
                        self.write(f, i.register_a(), wasm_gc_load_value(value, field, mode));
                    }
                    crate::wasm::WASM_GC_ACCESS_STRUCT_SET => {
                        let reference = self.read(f, i.register_a());
                        if reference == Value::NULL {
                            return Err(JsError::wasm_trap_error(
                                crate::WasmTrap::NullStructReference,
                            ));
                        }
                        let raw =
                            reference
                                .as_int()
                                .map(|value| value as u32)
                                .ok_or_else(|| {
                                    JsError::wasm_trap_error(crate::WasmTrap::CastFailure)
                                })?;
                        let field = self.wasm_gc_field(raw, field_index, false)?;
                        if !field.mutable {
                            return Err(JsError::validation(
                                "Wasm struct field is immutable".into(),
                            ));
                        }
                        let value = wasm_gc_store_value(self.read(f, i.register_b()), field)?;
                        let Some(crate::vm::wasm::WasmGcObject::Struct { values, .. }) =
                            self.wasm_gc_objects.get_mut(&raw)
                        else {
                            return Err(JsError::wasm_trap_error(crate::WasmTrap::CastFailure));
                        };
                        let slot = values.get_mut(field_index).ok_or_else(|| {
                            JsError::validation("Wasm struct field is out of bounds".into())
                        })?;
                        *slot = value;
                    }
                    crate::wasm::WASM_GC_ACCESS_ARRAY_SET => {
                        let reference = self.read(f, i.register_a());
                        if reference == Value::NULL {
                            return Err(JsError::wasm_trap_error(
                                crate::WasmTrap::NullArrayReference,
                            ));
                        }
                        let raw =
                            reference
                                .as_int()
                                .map(|value| value as u32)
                                .ok_or_else(|| {
                                    JsError::wasm_trap_error(crate::WasmTrap::CastFailure)
                                })?;
                        let field = self.wasm_gc_field(raw, 0, true)?;
                        if !field.mutable {
                            return Err(JsError::validation(
                                "Wasm array field is immutable".into(),
                            ));
                        }
                        let index = self.read(f, i.register_b()).as_int().unwrap_or_default() as u32
                            as usize;
                        let value = wasm_gc_store_value(self.read(f, i.register_c()), field)?;
                        let Some(crate::vm::wasm::WasmGcObject::Array { values, .. }) =
                            self.wasm_gc_objects.get_mut(&raw)
                        else {
                            return Err(JsError::wasm_trap_error(crate::WasmTrap::CastFailure));
                        };
                        let slot = values.get_mut(index).ok_or_else(|| {
                            JsError::wasm_trap_error(crate::WasmTrap::ArrayOutOfBounds)
                        })?;
                        *slot = value;
                    }
                    crate::wasm::WASM_GC_ACCESS_ARRAY_LEN => {
                        let reference = self.read(f, i.register_b());
                        if reference == Value::NULL {
                            return Err(JsError::wasm_trap_error(
                                crate::WasmTrap::NullArrayReference,
                            ));
                        }
                        let raw =
                            reference
                                .as_int()
                                .map(|value| value as u32)
                                .ok_or_else(|| {
                                    JsError::wasm_trap_error(crate::WasmTrap::CastFailure)
                                })?;
                        let length = self.wasm_gc_object_values(raw, true)?.len();
                        self.write(f, i.register_a(), Value::integer(length as u32 as i32));
                    }
                    crate::wasm::WASM_GC_ACCESS_ARRAY_FILL => {
                        let reference = self.read(f, i.register_a());
                        if reference == Value::NULL {
                            return Err(JsError::wasm_trap_error(
                                crate::WasmTrap::NullArrayReference,
                            ));
                        }
                        let raw =
                            reference
                                .as_int()
                                .map(|value| value as u32)
                                .ok_or_else(|| {
                                    JsError::wasm_trap_error(crate::WasmTrap::CastFailure)
                                })?;
                        let field = self.wasm_gc_field(raw, 0, true)?;
                        if !field.mutable {
                            return Err(JsError::validation(
                                "Wasm array field is immutable".into(),
                            ));
                        }
                        let start = self.read(f, i.register_b()).as_int().unwrap_or_default() as u32
                            as usize;
                        let count_register =
                            (i.imm() & crate::wasm::WASM_GC_ACCESS_REGISTER_MASK) as u16;
                        let count = wasm_gc_array_length(self.read(f, count_register))?;
                        let value = wasm_gc_store_value(self.read(f, i.register_c()), field)?;
                        let Some(crate::vm::wasm::WasmGcObject::Array { values, .. }) =
                            self.wasm_gc_objects.get_mut(&raw)
                        else {
                            return Err(JsError::wasm_trap_error(crate::WasmTrap::CastFailure));
                        };
                        let range = wasm_gc_range(
                            start,
                            count,
                            values.len(),
                            crate::WasmTrap::ArrayOutOfBounds,
                        )?;
                        values[range].fill(value);
                    }
                    crate::wasm::WASM_GC_ACCESS_ARRAY_COPY => {
                        let destination = self.read(f, i.register_a());
                        if destination == Value::NULL {
                            return Err(JsError::wasm_trap_error(
                                crate::WasmTrap::NullArrayReference,
                            ));
                        }
                        let source = self.read(f, i.register_c());
                        if source == Value::NULL {
                            return Err(JsError::wasm_trap_error(
                                crate::WasmTrap::NullArrayReference,
                            ));
                        }
                        let destination_raw = destination
                            .as_int()
                            .map(|value| value as u32)
                            .ok_or_else(|| {
                                JsError::wasm_trap_error(crate::WasmTrap::CastFailure)
                            })?;
                        let source_raw =
                            source.as_int().map(|value| value as u32).ok_or_else(|| {
                                JsError::wasm_trap_error(crate::WasmTrap::CastFailure)
                            })?;
                        let destination_field = self.wasm_gc_field(destination_raw, 0, true)?;
                        if !destination_field.mutable {
                            return Err(JsError::validation(
                                "Wasm array field is immutable".into(),
                            ));
                        }
                        let destination_start =
                            self.read(f, i.register_b()).as_int().unwrap_or_default() as u32
                                as usize;
                        let source_index_register =
                            (i.imm() & crate::wasm::WASM_GC_ACCESS_REGISTER_MASK) as u16;
                        let length_register = ((i.imm()
                            >> crate::wasm::WASM_GC_ACCESS_SECOND_REGISTER_SHIFT)
                            & crate::wasm::WASM_GC_ACCESS_REGISTER_MASK)
                            as u16;
                        let source_start =
                            self.read(f, source_index_register)
                                .as_int()
                                .unwrap_or_default() as u32 as usize;
                        let count = wasm_gc_array_length(self.read(f, length_register))?;
                        let destination_length =
                            self.wasm_gc_object_values(destination_raw, true)?.len();
                        let source_values = self.wasm_gc_object_values(source_raw, true)?;
                        let destination_range = wasm_gc_range(
                            destination_start,
                            count,
                            destination_length,
                            crate::WasmTrap::ArrayOutOfBounds,
                        )?;
                        let source_range = wasm_gc_range(
                            source_start,
                            count,
                            source_values.len(),
                            crate::WasmTrap::ArrayOutOfBounds,
                        )?;
                        let copied = source_values[source_range].to_vec();
                        let copied = copied
                            .into_iter()
                            .map(|value| wasm_gc_store_value(value, destination_field))
                            .collect::<Result<Vec<_>, _>>()?;
                        let Some(crate::vm::wasm::WasmGcObject::Array { values, .. }) =
                            self.wasm_gc_objects.get_mut(&destination_raw)
                        else {
                            return Err(JsError::wasm_trap_error(crate::WasmTrap::CastFailure));
                        };
                        values[destination_range].copy_from_slice(&copied);
                    }
                    crate::wasm::WASM_GC_ACCESS_ARRAY_INIT_DATA => {
                        let module = self.active_wasm_module.ok_or_else(|| {
                            JsError::validation(
                                "Wasm array initialization has no active module".into(),
                            )
                        })?;
                        let reference = self.read(f, i.register_a());
                        if reference == Value::NULL {
                            return Err(JsError::wasm_trap_error(
                                crate::WasmTrap::NullArrayReference,
                            ));
                        }
                        let raw =
                            reference
                                .as_int()
                                .map(|value| value as u32)
                                .ok_or_else(|| {
                                    JsError::wasm_trap_error(crate::WasmTrap::CastFailure)
                                })?;
                        let field = self.wasm_gc_field(raw, 0, true)?;
                        if !field.mutable {
                            return Err(JsError::validation(
                                "Wasm array field is immutable".into(),
                            ));
                        }
                        let destination_start =
                            self.read(f, i.register_b()).as_int().unwrap_or_default() as u32
                                as usize;
                        let source_start = self.read(f, i.register_c()).as_int().unwrap_or_default()
                            as u32 as usize;
                        let length_register =
                            (i.imm() & crate::wasm::WASM_GC_ACCESS_REGISTER_MASK) as u16;
                        let count = wasm_gc_array_length(self.read(f, length_register))?;
                        let destination_length = self.wasm_gc_object_values(raw, true)?.len();
                        let destination_range = wasm_gc_range(
                            destination_start,
                            count,
                            destination_length,
                            crate::WasmTrap::ArrayOutOfBounds,
                        )?;
                        let segment_index = (i.imm() >> crate::wasm::WASM_GC_ACCESS_SEGMENT_SHIFT)
                            & crate::wasm::WASM_GC_ACCESS_SEGMENT_MASK;
                        let segment = self
                            .wasm_modules
                            .get(module.raw() as usize)
                            .and_then(|module| module.data_segments.get(segment_index as usize))
                            .ok_or_else(|| {
                                JsError::wasm_trap_error(crate::WasmTrap::MemoryOutOfBounds)
                            })?;
                        let wasm_values = match segment {
                            Some(data) => wasm_gc_data_values(field, data, source_start, count)?,
                            None if count == 0 => {
                                wasm_gc_range(
                                    source_start,
                                    0,
                                    0,
                                    crate::WasmTrap::MemoryOutOfBounds,
                                )?;
                                Vec::new()
                            }
                            None => {
                                return Err(JsError::wasm_trap_error(
                                    crate::WasmTrap::MemoryOutOfBounds,
                                ));
                            }
                        };
                        let values = wasm_values
                            .into_iter()
                            .map(|value| self.encode_wasm_scalar(value))
                            .collect::<Vec<_>>();
                        let Some(crate::vm::wasm::WasmGcObject::Array { values: stored, .. }) =
                            self.wasm_gc_objects.get_mut(&raw)
                        else {
                            return Err(JsError::wasm_trap_error(crate::WasmTrap::CastFailure));
                        };
                        stored[destination_range].copy_from_slice(&values);
                    }
                    crate::wasm::WASM_GC_ACCESS_ARRAY_INIT_ELEM => {
                        let module = self.active_wasm_module.ok_or_else(|| {
                            JsError::validation(
                                "Wasm array initialization has no active module".into(),
                            )
                        })?;
                        let reference = self.read(f, i.register_a());
                        if reference == Value::NULL {
                            return Err(JsError::wasm_trap_error(
                                crate::WasmTrap::NullArrayReference,
                            ));
                        }
                        let raw =
                            reference
                                .as_int()
                                .map(|value| value as u32)
                                .ok_or_else(|| {
                                    JsError::wasm_trap_error(crate::WasmTrap::CastFailure)
                                })?;
                        let field = self.wasm_gc_field(raw, 0, true)?;
                        if !field.mutable {
                            return Err(JsError::validation(
                                "Wasm array field is immutable".into(),
                            ));
                        }
                        let destination_start =
                            self.read(f, i.register_b()).as_int().unwrap_or_default() as u32
                                as usize;
                        let source_start = self.read(f, i.register_c()).as_int().unwrap_or_default()
                            as u32 as usize;
                        let length_register =
                            (i.imm() & crate::wasm::WASM_GC_ACCESS_REGISTER_MASK) as u16;
                        let count = wasm_gc_array_length(self.read(f, length_register))?;
                        let destination_length = self.wasm_gc_object_values(raw, true)?.len();
                        let destination_range = wasm_gc_range(
                            destination_start,
                            count,
                            destination_length,
                            crate::WasmTrap::ArrayOutOfBounds,
                        )?;
                        let segment_index = (i.imm() >> crate::wasm::WASM_GC_ACCESS_SEGMENT_SHIFT)
                            & crate::wasm::WASM_GC_ACCESS_SEGMENT_MASK;
                        let segment = self
                            .wasm_modules
                            .get(module.raw() as usize)
                            .and_then(|module| module.element_segments.get(segment_index as usize))
                            .ok_or_else(|| {
                                JsError::wasm_trap_error(crate::WasmTrap::TableOutOfBounds)
                            })?;
                        let wasm_values = match segment.values.as_ref() {
                            Some(elements) => {
                                let range = wasm_gc_range(
                                    source_start,
                                    count,
                                    elements.len(),
                                    crate::WasmTrap::TableOutOfBounds,
                                )?;
                                elements[range].to_vec()
                            }
                            None if count == 0 => {
                                wasm_gc_range(
                                    source_start,
                                    0,
                                    0,
                                    crate::WasmTrap::TableOutOfBounds,
                                )?;
                                Vec::new()
                            }
                            None => {
                                return Err(JsError::wasm_trap_error(
                                    crate::WasmTrap::TableOutOfBounds,
                                ));
                            }
                        };
                        let values = wasm_values
                            .into_iter()
                            .map(|value| self.encode_wasm_scalar(value))
                            .map(|value| wasm_gc_store_value(value, field))
                            .collect::<Result<Vec<_>, _>>()?;
                        let Some(crate::vm::wasm::WasmGcObject::Array { values: stored, .. }) =
                            self.wasm_gc_objects.get_mut(&raw)
                        else {
                            return Err(JsError::wasm_trap_error(crate::WasmTrap::CastFailure));
                        };
                        stored[destination_range].copy_from_slice(&values);
                    }
                    _ => {
                        return Err(JsError::validation(
                            "invalid Wasm GC field operation".into(),
                        ));
                    }
                }
            }
            Op::WasmRefConvert => {
                let value = self.read(f, i.register_b());
                let converted = if value == Value::NULL {
                    value
                } else if i.imm() == 0 {
                    let raw = value.as_int().ok_or_else(|| {
                        JsError::validation("invalid Wasm extern reference".into())
                    })? as u32;
                    if raw >> 28 == 1 {
                        let handle = raw & crate::wasm::WASM_REFERENCE_HANDLE_MASK;
                        self.wasm_externref_bridge
                            .get(&handle)
                            .copied()
                            .unwrap_or_else(|| {
                                Value::integer(
                                    (crate::wasm::WASM_ANYREF_EXTERN_TAG | handle) as i32,
                                )
                            })
                    } else {
                        value
                    }
                } else if let Some(raw) = value.as_int() {
                    let raw = raw as u32;
                    match raw >> 28 {
                        1 => value,
                        2 => Value::integer(
                            (crate::wasm::WASM_EXTERNREF_TAG
                                | (raw & crate::wasm::WASM_REFERENCE_HANDLE_MASK))
                                as i32,
                        ),
                        _ => {
                            let mut handle =
                                self.wasm_gc_next_ref & crate::wasm::WASM_REFERENCE_HANDLE_MASK;
                            while handle == 0 || self.wasm_externref_bridge.contains_key(&handle) {
                                self.wasm_gc_next_ref =
                                    self.wasm_gc_next_ref.wrapping_add(1).max(1);
                                handle =
                                    self.wasm_gc_next_ref & crate::wasm::WASM_REFERENCE_HANDLE_MASK;
                            }
                            self.wasm_gc_next_ref = self.wasm_gc_next_ref.wrapping_add(1).max(1);
                            self.wasm_externref_bridge.insert(handle, value);
                            Value::integer((crate::wasm::WASM_EXTERNREF_TAG | handle) as i32)
                        }
                    }
                } else if matches!(self.heap.get(value), Some(Cell::WasmBits64(_))) {
                    let mut handle =
                        self.wasm_gc_next_ref & crate::wasm::WASM_REFERENCE_HANDLE_MASK;
                    while handle == 0 || self.wasm_externref_bridge.contains_key(&handle) {
                        self.wasm_gc_next_ref = self.wasm_gc_next_ref.wrapping_add(1).max(1);
                        handle = self.wasm_gc_next_ref & crate::wasm::WASM_REFERENCE_HANDLE_MASK;
                    }
                    self.wasm_gc_next_ref = self.wasm_gc_next_ref.wrapping_add(1).max(1);
                    self.wasm_externref_bridge.insert(handle, value);
                    Value::integer((crate::wasm::WASM_EXTERNREF_TAG | handle) as i32)
                } else {
                    return Err(JsError::validation(
                        "invalid Wasm reference conversion".into(),
                    ));
                };
                self.write(f, i.result_register(), converted);
            }
            Op::WasmRefTest => {
                let value = self.read(f, i.register_b());
                self.write(
                    f,
                    i.result_register(),
                    Value::integer(i32::from(self.wasm_reference_matches(value, i.imm()))),
                );
            }
            Op::WasmRefCast => {
                let value = self.read(f, i.register_b());
                if !self.wasm_reference_matches(value, i.imm()) {
                    return Err(JsError::wasm_trap_error(crate::WasmTrap::CastFailure));
                }
                self.write(f, i.result_register(), value);
            }
            Op::WasmMemoryFill => {
                let module = self.active_wasm_module.ok_or_else(|| {
                    JsError::validation("Wasm memory fill has no active module".into())
                })?;
                let destination =
                    self.wasm_memory_operand(module, i.imm(), self.read(f, i.register_a()))?;
                let value = self
                    .read(f, i.register_b())
                    .as_int()
                    .ok_or_else(|| JsError::validation("invalid Wasm fill value".into()))?;
                let length =
                    self.wasm_memory_operand(module, i.imm(), self.read(f, i.register_c()))?;
                self.wasm_memory_fill(module, i.imm(), destination, value, length)?;
            }
            Op::WasmMemoryCopy => {
                let module = self.active_wasm_module.ok_or_else(|| {
                    JsError::validation("Wasm memory copy has no active module".into())
                })?;
                let destination_memory = i.imm() & 0xffff;
                let source_memory = i.imm() >> 16;
                let destination = self.wasm_memory_operand(
                    module,
                    destination_memory,
                    self.read(f, i.register_a()),
                )?;
                let source =
                    self.wasm_memory_operand(module, source_memory, self.read(f, i.register_b()))?;
                let length = self.wasm_memory_operand(
                    module,
                    destination_memory,
                    self.read(f, i.register_c()),
                )?;
                self.wasm_memory_copy(
                    module,
                    destination_memory,
                    source_memory,
                    destination,
                    source,
                    length,
                )?;
            }
            Op::WasmMemoryInit => {
                let module = self.active_wasm_module.ok_or_else(|| {
                    JsError::validation("Wasm memory.init has no active module".into())
                })?;
                let memory_index = i.imm() >> 16;
                let destination =
                    self.wasm_memory_operand(module, memory_index, self.read(f, i.register_a()))?;
                let source = self
                    .read(f, i.register_b())
                    .as_int()
                    .ok_or_else(|| JsError::validation("invalid Wasm data offset".into()))?
                    as u32;
                let length =
                    self.read(f, i.register_c()).as_int().ok_or_else(|| {
                        JsError::validation("invalid Wasm memory.init length".into())
                    })? as u32 as u64;
                self.wasm_memory_init(
                    module,
                    memory_index,
                    i.imm() & 0xffff,
                    destination,
                    source,
                    length,
                )?;
            }
            Op::WasmDataDrop => {
                let module = self.active_wasm_module.ok_or_else(|| {
                    JsError::validation("Wasm data.drop has no active module".into())
                })?;
                self.wasm_data_drop(module, i.imm())?;
            }
            Op::WasmRefFunc => {
                let module = self.active_wasm_module.ok_or_else(|| {
                    JsError::validation("Wasm ref.func has no active module".into())
                })?;
                let crate::WasmValue::FuncRef(Some(reference)) =
                    self.wasm_function_ref_value(module, i.closure_function_index())?
                else {
                    unreachable!("non-null Wasm function reference")
                };
                let value = Value::integer(reference as i32);
                self.write(f, i.result_register(), value);
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
                let value = self.encode_wasm_scalar(value);
                self.write(f, i.result_register(), value);
            }
            Op::WasmI64Unary => {
                let value = self.wasm_i64_operand(self.read(f, i.register_b()))?;
                let operator = crate::wasm::integer::I64UnaryOperator::from_tag(i.imm())
                    .expect("validated Wasm unary operator");
                let value = self.encode_wasm_scalar(operator.apply(value));
                self.write(f, i.result_register(), value);
            }
            Op::WasmWideArithmetic => {
                let operator = crate::wasm::wide::WideArithmeticOperator::from_tag(i.imm())
                    .expect("validated Wasm wide arithmetic operator");
                let input = i.register_b();
                let left_low = self.wasm_i64_operand(self.read(f, input))?;
                let left_high = if operator.input_count() == 4 {
                    Some(self.wasm_i64_operand(self.read(f, input + 1))?)
                } else {
                    None
                };
                let right_low_index = input + u16::from(operator.input_count() == 4) * 2;
                let right_low = if operator.input_count() == 4 {
                    self.wasm_i64_operand(self.read(f, input + 2))?
                } else {
                    self.wasm_i64_operand(self.read(f, input + 1))?
                };
                let right_high = if operator.input_count() == 4 {
                    Some(self.wasm_i64_operand(self.read(f, right_low_index + 1))?)
                } else {
                    None
                };
                let (low, high) = operator.apply(left_low, left_high, right_low, right_high);
                let low = self.encode_wasm_scalar(crate::WasmValue::I64(low));
                let high = self.encode_wasm_scalar(crate::WasmValue::I64(high));
                let output = i.result_register();
                self.write(f, output, low);
                self.write(f, output + 1, high);
            }
            Op::WasmAtomic => {
                let module = self.active_wasm_module.ok_or_else(|| {
                    JsError::validation("Wasm atomic operation has no active module".into())
                })?;
                let base = i.register_a();
                let input_count = self.wasm_atomic_input_count(module, i.imm())?;
                let inputs = (0..input_count)
                    .map(|offset| self.read(f, base + offset))
                    .collect::<Vec<_>>();
                if let Some(result) = self.wasm_atomic(module, i.imm(), &inputs)? {
                    self.write(f, base, result);
                }
            }
            Op::WasmScalarConvert => {
                let operator = crate::wasm::conversion::ScalarConversionOperator::from_tag(i.imm())
                    .expect("validated Wasm scalar conversion");
                let value =
                    self.decode_wasm_scalar(self.read(f, i.register_b()), operator.source_type())?;
                let value = operator.apply(value).map_err(JsError::wasm_trap_error)?;
                debug_assert_eq!(value.ty(), operator.result_type());
                let value = self.encode_wasm_scalar(value);
                self.write(f, i.result_register(), value);
            }
            Op::WasmF32Binary => {
                let left = self.wasm_f32_operand(self.read(f, i.register_b()))?;
                let right = self.wasm_f32_operand(self.read(f, i.register_c()))?;
                let operator = crate::wasm::float::F32BinaryOperator::from_tag(i.imm())
                    .expect("validated Wasm float operator");
                let value = self.encode_wasm_scalar(operator.apply(left, right));
                self.write(f, i.result_register(), value);
            }
            Op::WasmF32Unary => {
                let left = self.wasm_f32_operand(self.read(f, i.register_b()))?;
                let operator = crate::wasm::float::F32UnaryOperator::from_tag(i.imm())
                    .expect("validated Wasm float operator");
                let value = self.encode_wasm_scalar(operator.apply(left));
                self.write(f, i.result_register(), value);
            }
            Op::WasmF64Binary => {
                let left = self.wasm_f64_operand(self.read(f, i.register_b()))?;
                let right = self.wasm_f64_operand(self.read(f, i.register_c()))?;
                let operator = crate::wasm::float::F64BinaryOperator::from_tag(i.imm())
                    .expect("validated Wasm float operator");
                let value = self.encode_wasm_scalar(operator.apply(left, right));
                self.write(f, i.result_register(), value);
            }
            Op::WasmF64Unary => {
                let left = self.wasm_f64_operand(self.read(f, i.register_b()))?;
                let operator = crate::wasm::float::F64UnaryOperator::from_tag(i.imm())
                    .expect("validated Wasm float operator");
                let value = self.encode_wasm_scalar(operator.apply(left));
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
                self.profile.branch_value(value.profile_kind(), truthy);
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
                    CallArguments::from_values(
                        (0..window.count).map(|x| self.read(f, window.base + x)),
                    )
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
                if p.kind == crate::bytecode::ProgramKind::Wasm && i.returns_from_frame() {
                    // Native/host calls need argument roots, not the discarded guest activation.
                    self.with_stack.truncate(self.frames[f].with_base);
                    let frame = &mut self.frames[f];
                    frame.context = CallContext::Internal;
                    frame.locals.fill(Value::UNDEFINED);
                    frame.registers.fill(Value::UNDEFINED);
                    frame.dynamic_bindings.clear();
                    frame.active_iterators.clear();
                    frame.env = Value::NULL;
                    frame.this = Value::UNDEFINED;
                    frame.captured = false;
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
                let imported_wasm_function = (p.kind == crate::bytecode::ProgramKind::Wasm)
                    .then(|| self.active_wasm_module)
                    .flatten()
                    .and_then(|module| self.wasm_modules.get(module.raw() as usize))
                    .and_then(|module| module.imported_functions.get(function_index as usize))
                    .copied()
                    .flatten();
                if let Some(target) = imported_wasm_function {
                    if i.returns_from_frame() {
                        self.tail_call_wasm_function(target, f, args)?;
                        self.profile.terminal_call(0);
                        return Ok(StepResult::TailCall);
                    }
                    let value = self.call_wasm_import(target, args)?;
                    if i.returns_from_frame() {
                        self.profile.terminal_call(0);
                        return Ok(StepResult::Return(value));
                    }
                    self.write(f, i.result_register(), value);
                    return Ok(StepResult::Continue);
                }
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
            Op::WasmCallIndirect => {
                let module = self.active_wasm_module.ok_or_else(|| {
                    JsError::validation("Wasm indirect call has no active module".into())
                })?;
                let target_value = self.read(f, i.register_b());
                let stored = self
                    .wasm_modules
                    .get(module.raw() as usize)
                    .ok_or_else(|| JsError::validation("unknown Wasm module".into()))?;
                let site = *stored
                    .indirect_sites
                    .get(usize::from(
                        i.field_value(crate::bytecode::InstructionField::C),
                    ))
                    .ok_or_else(|| {
                        JsError::validation("Wasm indirect call site out of bounds".into())
                    })?;
                let function_index = if site.table_index == u32::MAX {
                    if target_value == Value::NULL {
                        return Err(JsError::wasm_trap_error(
                            crate::WasmTrap::NullFunctionReference,
                        ));
                    }
                    target_value
                        .as_int()
                        .and_then(|index| u32::try_from(index).ok())
                        .ok_or_else(|| {
                            JsError::validation("invalid Wasm function reference".into())
                        })?
                } else {
                    let table_slot =
                        self.wasm_table_operand(module, site.table_index, target_value)?;
                    let table_id =
                        *stored
                            .tables
                            .get(site.table_index as usize)
                            .ok_or_else(|| {
                                JsError::validation("Wasm table index out of bounds".into())
                            })?;
                    self.wasm_tables
                        .get(table_id.raw() as usize)
                        .and_then(|table| table.borrow().get(table_slot))
                        .and_then(|value| match value {
                            crate::WasmValue::FuncRef(Some(index)) => Some(index),
                            _ => None,
                        })
                        .ok_or_else(|| {
                            JsError::wasm_trap_error_with_message(
                                crate::WasmTrap::UndefinedElement,
                                format!("undefined element (uninitialized element {table_slot})"),
                            )
                        })?
                };
                let expected = stored
                    .type_signatures
                    .get(site.type_index as usize)
                    .cloned()
                    .ok_or_else(|| JsError::validation("Wasm call type out of bounds".into()))?;
                let function_target = self
                    .wasm_function_ref_handles
                    .get(&function_index)
                    .copied()
                    .unwrap_or(crate::WasmFunctionRef {
                        module,
                        function_index,
                    });
                let target_module = self
                    .wasm_modules
                    .get(function_target.module.raw() as usize)
                    .ok_or_else(|| JsError::validation("unknown Wasm function module".into()))?;
                let actual = target_module
                    .signatures
                    .get(function_target.function_index as usize)
                    .cloned()
                    .ok_or_else(|| {
                        JsError::validation("Wasm table function out of bounds".into())
                    })?;
                let type_matches = if function_target.module == module {
                    self.wasm_function_type_matches(
                        module,
                        function_target.function_index,
                        site.type_index,
                    )
                    .unwrap_or(expected == actual)
                } else {
                    expected == actual
                };
                if !type_matches {
                    return Err(JsError::wasm_trap_error(
                        crate::WasmTrap::IndirectCallTypeMismatch,
                    ));
                }
                let imported_target = target_module
                    .imported_functions
                    .get(function_target.function_index as usize)
                    .copied()
                    .flatten();
                let window = i.call_window();
                let arguments = CallArguments::from_values(
                    (0..window.count).map(|offset| self.read(f, window.base + offset)),
                );
                if i.returns_from_frame() {
                    self.tail_call_wasm_function(function_target, f, arguments.as_slice())?;
                    self.profile.terminal_call(0);
                    return Ok(StepResult::TailCall);
                }
                let parent = self.capture_env(f, 0).unwrap_or(self.frames[f].env);
                self.frames[f].pc = *pc;
                let value = if function_target.module != module {
                    self.call_wasm_import(function_target, arguments.as_slice())?
                } else if let Some(target) = imported_target {
                    self.call_wasm_import(target, arguments.as_slice())?
                } else {
                    self.call_user_maybe_async(
                        p,
                        function_target.function_index,
                        parent,
                        Value::UNDEFINED,
                        arguments.as_slice(),
                        CallContext::Internal,
                    )?
                };
                if i.returns_from_frame() {
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
                self.load_local_binding(p, frame, operand.payload() as usize, None)
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
