use super::*;

impl FunctionCompiler<'_, '_> {
    pub(super) fn class_expression(&mut self, class: &Class<'_>) -> Register {
        self.lower_class(class, false, None)
    }

    pub(super) fn named_class_expression(&mut self, class: &Class<'_>, name: &str) -> Register {
        self.lower_class(class, false, Some(name))
    }

    pub(super) fn class_declaration(&mut self, class: &Class<'_>) {
        self.lower_class(class, true, None);
    }

    fn lower_class(
        &mut self,
        class: &Class<'_>,
        bind_name: bool,
        inferred_name: Option<&str>,
    ) -> Register {
        // Emit before heritage/TDZ evaluation, then fill from the class's allocated
        // locals. This preserves earlier class evaluations before their slots change.
        let first_class_slot = self.locals.len();
        let clone_plan = self.environment_clones.len();
        self.environment_clones.push(Vec::new());
        self.emit(Op::CloneEnv, 0, 0, 0, clone_plan as u32);
        let class_binding = class.id.as_ref().map(|identifier| {
            let source = self.owner.atom(identifier.name.as_str());
            let binding = self.hidden_local(&format!(
                "{}\0quench:class-binding:{}",
                identifier.name.as_str(),
                self.function_id
            ));
            let slot = self.local_slots[&binding];
            self.emit(Op::InitializeTdz, 0, 0, 0, u32::from(slot));
            self.push_immutable_lexical_bindings(
                FxHashMap::from_iter([(source, binding)]),
                FxHashSet::from_iter([source]),
            );
            binding
        });
        let heritage = class.heritage.as_ref().map(|heritage| {
            let outer_strict = std::mem::replace(&mut self.strict, true);
            let value = self.expression(&heritage.expression);
            self.strict = outer_strict;
            value
        });
        if let Some(heritage) = heritage {
            self.emit(Op::ValidateClassHeritage, heritage, 0, 0, 0);
        }
        let super_atom = heritage.map(|_| {
            let atom = self.hidden_local(&format!("\0quench:class-super:{}", class.span.start));
            let super_binding = self.owner.atom("\0quench:super");
            self.push_lexical_bindings(FxHashMap::from_iter([(super_binding, atom)]));
            atom
        });
        for element in &class.body.body {
            match element {
                ClassElement::PropertyDefinition(field) if field.computed => {
                    self.hidden_local(&computed_field_key_name(field.span.start));
                }
                ClassElement::MethodDefinition(method) if method.computed => {
                    self.hidden_local(&computed_field_key_name(method.span.start));
                }
                ClassElement::AccessorProperty(accessor) if accessor.computed => {
                    self.hidden_local(&computed_field_key_name(accessor.span.start));
                }
                _ => {}
            }
            if let ClassElement::AccessorProperty(accessor) = element {
                self.owner.reserve_auto_accessor_name(accessor.span);
                if !accessor.decorators.is_empty() {
                    self.owner
                        .reject(accessor.span, "decorated class accessors are unsupported");
                }
            }
        }
        let mut method_home_atoms = FxHashMap::default();
        for element in &class.body.body {
            if let ClassElement::MethodDefinition(method) = element {
                let atom = self.hidden_local(&format!("\0quench:home:{}", method.span.start));
                method_home_atoms.insert(method.span.start, atom);
            }
        }
        let fields: Vec<_> = class
            .body
            .body
            .iter()
            .filter_map(|element| match element {
                ClassElement::PropertyDefinition(field) => {
                    Some(ClassField::Property(field.as_ref()))
                }
                ClassElement::AccessorProperty(accessor) => {
                    let backing = self.owner.private_name_atom(accessor.span);
                    Some(ClassField::AutoAccessor { accessor, backing })
                }
                _ => None,
            })
            .collect();
        let instance_fields: Vec<_> = fields
            .iter()
            .copied()
            .filter(|field| !class_field_static(*field))
            .collect();
        let instance_private_method_spans: Vec<_> = class
            .body
            .body
            .iter()
            .flat_map(|element| {
                let (name, backing) = match element {
                    ClassElement::MethodDefinition(method) if !method.r#static => (
                        matches!(&method.key, PropertyKey::PrivateIdentifier(_))
                            .then(|| method.key.span()),
                        None,
                    ),
                    ClassElement::AccessorProperty(accessor) if !accessor.r#static => (
                        matches!(&accessor.key, PropertyKey::PrivateIdentifier(_))
                            .then(|| accessor.key.span()),
                        Some(accessor.span),
                    ),
                    _ => (None, None),
                };
                name.into_iter().chain(backing)
            })
            .collect();
        let mut installed_private_names = FxHashSet::default();
        let instance_private_methods: Vec<_> = instance_private_method_spans
            .iter()
            .map(|span| self.owner.private_name_atom(*span))
            .filter(|atom| installed_private_names.insert(*atom))
            .collect();
        let constructor = class.body.body.iter().find_map(|element| match element {
            ClassElement::MethodDefinition(method)
                if method.kind == MethodDefinitionKind::Constructor =>
            {
                Some((method, class.span))
            }
            _ => None,
        });
        let constructor_home_atom = constructor
            .map(|(method, _)| method_home_atoms[&method.span.start])
            .unwrap_or_else(|| {
                self.hidden_local(&format!(
                    "\0quench:home:implicit-constructor:{}",
                    class.span.start
                ))
            });
        let class_home_atom =
            self.hidden_local(&format!("\0quench:home:class:{}", class.span.start));
        for field in &fields {
            if class_field_value(*field).is_some() {
                self.hidden_local(&class_field_initializer_name(
                    class_field_span(*field).start,
                ));
            }
        }
        self.environment_clones[clone_plan] = (first_class_slot..self.locals.len())
            .map(|slot| slot as u16)
            .collect();
        let scopes = self.capture_scopes();
        for field in &fields {
            let Some(expression) = class_field_value(*field) else {
                continue;
            };
            let id = self.owner.compile_function(
                None,
                &[],
                FunctionBody::Expression(expression),
                &scopes,
                Some(self.function_id),
                FunctionOptions {
                    class_field_initializer: true,
                    non_constructible: true,
                    super_home: true,
                    super_home_atom: Some(if class_field_static(*field) {
                        class_home_atom
                    } else {
                        constructor_home_atom
                    }),
                    super_static: class_field_static(*field),
                    strict: true,
                    with_depth: self.with_depth,
                    ..FunctionOptions::default()
                },
            );
            let initializer = self.reg();
            self.emit(Op::MakeClosure, initializer, 0, 0, id);
            let atom = self.owner.atom(&class_field_initializer_name(
                class_field_span(*field).start,
            ));
            self.store_atom(atom, initializer);
        }
        let implicit_super = constructor.is_none() && heritage.is_some();
        let constructor_id = constructor
            .map(|(method, source_span)| {
                self.owner.compile_class_method(
                    method,
                    Some(source_span),
                    &scopes,
                    Some(self.function_id),
                    heritage.is_none().then_some(instance_fields.as_slice()),
                    heritage
                        .is_none()
                        .then_some(instance_private_methods.as_slice()),
                    heritage.is_some(),
                    method_home_atoms[&method.span.start],
                    self.with_depth,
                )
            })
            .unwrap_or_else(|| {
                let params = if implicit_super {
                    vec!["\0quench:derived-args".to_owned()]
                } else {
                    vec![]
                };
                self.owner.compile_function(
                    None,
                    &params,
                    FunctionBody::Statements(&[]),
                    &scopes,
                    Some(self.function_id),
                    FunctionOptions {
                        defaults: None,
                        source_text: self.owner.source_text(class.span),
                        name_binding: None,
                        async_function: false,
                        generator: false,
                        class_constructor: true,
                        derived_constructor: implicit_super,
                        non_constructible: false,
                        class_field_initializer: false,
                        instance_fields: Some(&instance_fields),
                        instance_private_methods: Some(&instance_private_methods),
                        super_static: false,
                        super_home: true,
                        super_home_atom: Some(constructor_home_atom),
                        rest_override: implicit_super,
                        implicit_super,
                        with_depth: self.with_depth,
                        strict: true,
                    },
                )
            });
        if heritage.is_some()
            && (!instance_fields.is_empty() || !instance_private_methods.is_empty())
        {
            let initializer = self.owner.compile_function(
                None,
                &[],
                FunctionBody::Statements(&[]),
                &scopes,
                Some(self.function_id),
                FunctionOptions {
                    instance_fields: Some(&instance_fields),
                    instance_private_methods: Some(&instance_private_methods),
                    class_field_initializer: true,
                    non_constructible: true,
                    super_home: true,
                    super_home_atom: Some(constructor_home_atom),
                    strict: true,
                    with_depth: self.with_depth,
                    ..FunctionOptions::default()
                },
            );
            self.owner.functions[constructor_id as usize]
                .as_mut()
                .expect("compiled class constructor")
                .instance_initializer = Some(initializer);
        }
        let class_value = self.reg();
        self.emit(Op::MakeClosure, class_value, 0, 0, constructor_id);
        if let Some(super_atom) = super_atom {
            self.store_atom(super_atom, class_value);
        }
        self.store_atom(class_home_atom, class_value);
        if let Some(name) = class
            .id
            .as_ref()
            .map(|identifier| identifier.name.as_str())
            .or(inferred_name)
        {
            let atom = self.owner.atom(name);
            self.emit(Op::SetFunctionName, class_value, 0, 0, atom);
        }
        let prototype = self.reg();
        let prototype_atom = self.owner.atom("prototype");
        let prototype_cache = self.owner.cache_site();
        self.emit(
            Op::GetField,
            prototype,
            FieldBase::register(class_value).0,
            prototype_cache,
            prototype_atom,
        );
        self.define_class_method(prototype, class_value, None, Some("constructor"));
        self.store_atom(constructor_home_atom, prototype);

        for element in &class.body.body {
            let (name, is_static) = match element {
                ClassElement::PropertyDefinition(field) => match &field.key {
                    PropertyKey::PrivateIdentifier(identifier) => {
                        (Some(identifier.span), field.r#static)
                    }
                    _ => (None, false),
                },
                ClassElement::MethodDefinition(method) => match &method.key {
                    PropertyKey::PrivateIdentifier(identifier) => {
                        (Some(identifier.span), method.r#static)
                    }
                    _ => (None, false),
                },
                ClassElement::AccessorProperty(accessor) => {
                    (Some(accessor.span), accessor.r#static)
                }
                _ => (None, false),
            };
            let accessor_name = match element {
                ClassElement::AccessorProperty(accessor) => {
                    matches!(&accessor.key, PropertyKey::PrivateIdentifier(_))
                        .then(|| accessor.key.span())
                }
                _ => None,
            };
            for name in name.into_iter().chain(accessor_name) {
                let target = if is_static { class_value } else { prototype };
                let atom = self.owner.private_name_atom(name);
                if self.owner.private_name_is_overridden(atom) {
                    continue;
                }
                self.emit(Op::MarkPrivateName, 0, target, target, atom);
            }
        }

        if let Some(base) = heritage {
            let base_prototype = self.reg();
            let null = self.literal(Constant::Null);
            let is_null = self.emit_binary(
                BinaryOperator::StrictEquality as u32,
                Operand::register(base),
                Operand::register(null),
            );
            let get_constructor_prototype = self.emit(Op::JumpFalse, is_null, 0, 0, 0);
            self.emit(Op::Move, base_prototype, null, 0, 0);
            let skip_constructor_prototype = self.emit(Op::Jump, 0, 0, 0, 0);
            self.patch(get_constructor_prototype);
            let prototype_atom = self.owner.atom("prototype");
            let prototype_cache = self.owner.cache_site();
            self.emit(
                Op::GetField,
                base_prototype,
                FieldBase::register(base).0,
                prototype_cache,
                prototype_atom,
            );
            self.patch(skip_constructor_prototype);
            self.set_prototype(prototype, base_prototype);
            let set_constructor_parent = self.emit(Op::JumpFalse, is_null, 0, 0, 0);
            let skip_constructor_parent = self.emit(Op::Jump, 0, 0, 0, 0);
            self.patch(set_constructor_parent);
            self.set_prototype(class_value, base);
            self.patch(skip_constructor_parent);
        }

        for element in &class.body.body {
            let (key, span) = match element {
                ClassElement::PropertyDefinition(field) if field.computed => {
                    (field.key.as_expression(), field.span)
                }
                ClassElement::MethodDefinition(method) if method.computed => {
                    (method.key.as_expression(), method.span)
                }
                ClassElement::AccessorProperty(accessor) if accessor.computed => {
                    (accessor.key.as_expression(), accessor.span)
                }
                _ => continue,
            };
            let Some(key) = key else {
                self.owner
                    .reject(span, "computed class element key is unsupported");
                continue;
            };
            let outer_strict = std::mem::replace(&mut self.strict, true);
            let raw_key = self.expression(key);
            self.strict = outer_strict;
            let key_value = self.reg();
            self.emit(Op::ToPropertyKey, key_value, raw_key, 0, 0);
            let key_atom = self.owner.atom(&computed_field_key_name(span.start));
            self.store_atom(key_atom, key_value);
        }

        for element in &class.body.body {
            if let ClassElement::AccessorProperty(accessor) = element {
                let target = if accessor.r#static {
                    class_value
                } else {
                    prototype
                };
                let computed_key = if accessor.computed {
                    Some(self.load_name(&computed_field_key_name(accessor.span.start)))
                } else {
                    Self::unit_key(self, &accessor.key)
                };
                let name_text = if computed_key.is_none() {
                    let Some(name) = class_field_storage_name(self.owner, &accessor.key) else {
                        self.owner
                            .reject(accessor.span, "class accessor key is unsupported");
                        continue;
                    };
                    Some(name)
                } else {
                    None
                };
                let Some((getter_id, setter_id)) = self.owner.compile_auto_accessor_methods(
                    accessor,
                    &scopes,
                    Some(self.function_id),
                    class_home_atom,
                    self.with_depth,
                ) else {
                    continue;
                };
                for (function_id, kind, prefix) in [
                    (
                        getter_id,
                        "get",
                        crate::bytecode::FUNCTION_NAME_PREFIX_GETTER,
                    ),
                    (
                        setter_id,
                        "set",
                        crate::bytecode::FUNCTION_NAME_PREFIX_SETTER,
                    ),
                ] {
                    let function = self.reg();
                    self.emit(Op::MakeClosure, function, 0, 0, function_id);
                    if let Some(key) = computed_key {
                        self.emit(Op::SetFunctionNameKey, function, key, 0, prefix);
                    } else if let Some(name) = name_text.as_deref() {
                        let visible_name = match &accessor.key {
                            PropertyKey::PrivateIdentifier(identifier) => {
                                format!("#{}", identifier.name)
                            }
                            _ => name.to_owned(),
                        };
                        let atom = self.owner.atom(&format!("{kind} {visible_name}"));
                        self.emit(Op::SetFunctionName, function, 0, 0, atom);
                    }
                    self.define_class_accessor(
                        target,
                        function,
                        computed_key,
                        name_text.as_deref(),
                        kind,
                    );
                }
                continue;
            }
            let ClassElement::MethodDefinition(method) = element else {
                if !matches!(
                    element,
                    ClassElement::PropertyDefinition(_)
                        | ClassElement::AccessorProperty(_)
                        | ClassElement::StaticBlock(_)
                ) {
                    self.owner
                        .reject(element.span(), "class element is unsupported");
                }
                continue;
            };
            let accessor_name = match method.kind {
                MethodDefinitionKind::Get => Some("get"),
                MethodDefinitionKind::Set => Some("set"),
                MethodDefinitionKind::Method => None,
                MethodDefinitionKind::Constructor => None,
            };
            if method.kind == MethodDefinitionKind::Constructor {
                continue;
            }
            if method.kind != MethodDefinitionKind::Method && accessor_name.is_none() {
                self.owner
                    .reject(method.span, "class accessors are unsupported");
                continue;
            }
            let target = if method.r#static {
                class_value
            } else {
                prototype
            };
            let home_atom = method_home_atoms[&method.span.start];
            let computed_key = if method.computed {
                Some(self.load_name(&computed_field_key_name(method.span.start)))
            } else {
                Self::unit_key(self, &method.key)
            };
            let name_text = if computed_key.is_none() {
                let Some(name) = class_method_name(&method.key) else {
                    self.owner
                        .reject(method.span, "class method key is unsupported");
                    continue;
                };
                Some(
                    if let PropertyKey::PrivateIdentifier(identifier) = &method.key {
                        self.owner.private_name_text(identifier.span)
                    } else {
                        name
                    },
                )
            } else {
                None
            };
            self.store_atom(home_atom, target);
            let function_id = self.owner.compile_class_method(
                method,
                Some(element.span()),
                &scopes,
                Some(self.function_id),
                None,
                None,
                false,
                home_atom,
                self.with_depth,
            );
            let function = self.reg();
            self.emit(Op::MakeClosure, function, 0, 0, function_id);
            if let Some(accessor) = accessor_name {
                if let Some(key) = computed_key {
                    let prefix = match accessor {
                        "get" => crate::bytecode::FUNCTION_NAME_PREFIX_GETTER,
                        "set" => crate::bytecode::FUNCTION_NAME_PREFIX_SETTER,
                        _ => crate::bytecode::FUNCTION_NAME_PREFIX_NONE,
                    };
                    self.emit(Op::SetFunctionNameKey, function, key, 0, prefix);
                }
                self.define_class_accessor(
                    target,
                    function,
                    computed_key,
                    name_text.as_deref(),
                    accessor,
                );
            } else {
                self.define_class_method(target, function, computed_key, name_text.as_deref());
            }
        }

        if let Some(binding) = class_binding {
            self.initialize_class_binding(binding, class_value);
        }

        for element in &class.body.body {
            match element {
                ClassElement::PropertyDefinition(_) | ClassElement::AccessorProperty(_) => {
                    let field = match element {
                        ClassElement::PropertyDefinition(field) if field.r#static => {
                            ClassField::Property(field)
                        }
                        ClassElement::AccessorProperty(accessor) if accessor.r#static => {
                            let backing = self.owner.private_name_atom(accessor.span);
                            ClassField::AutoAccessor { accessor, backing }
                        }
                        _ => continue,
                    };
                    let span = class_field_span(field);
                    let initializer = class_field_value(field);
                    let key = class_field_key(field);
                    let computed_key = if class_field_computed(field) {
                        Some(self.load_name(&computed_field_key_name(span.start)))
                    } else {
                        Self::unit_key(self, key)
                    };
                    let name = if let ClassField::AutoAccessor { backing, .. } = field {
                        Some(backing)
                    } else if computed_key.is_none() {
                        let Some(name) = class_field_storage_name(self.owner, key) else {
                            self.owner.reject(span, "class field key is unsupported");
                            continue;
                        };
                        Some(self.owner.atom(&name))
                    } else {
                        None
                    };
                    let value = if initializer.is_some() {
                        let atom = self.owner.atom(&class_field_initializer_name(span.start));
                        let callee = self.load_atom(atom);
                        let value = self.reg();
                        self.emit(Op::Call, value, callee, class_value, 0);
                        value
                    } else {
                        self.literal(Constant::Undefined)
                    };
                    self.set_class_field_function_name(field, value);
                    if let Some(key) =
                        computed_key.filter(|_| matches!(field, ClassField::Property(_)))
                    {
                        self.emit(Op::SetIndex, value, class_value, key, 1);
                    } else {
                        let cache = self.owner.cache_site();
                        self.emit(Op::SetField, value, class_value, cache, name.unwrap());
                    }
                }
                ClassElement::StaticBlock(block) => {
                    let function_id = self.owner.compile_class_static_block(
                        block,
                        &scopes,
                        Some(self.function_id),
                        self.with_depth,
                    );
                    let function = self.reg();
                    self.emit(Op::MakeClosure, function, 0, 0, function_id);
                    let result = self.reg();
                    self.emit(Op::Call, result, function, class_value, 0);
                }
                _ => {}
            }
        }
        if super_atom.is_some() {
            self.pop_lexical_scope();
        }
        if class_binding.is_some() {
            self.pop_lexical_scope();
        }
        if bind_name {
            if let Some(name) = &class.id {
                let atom = self.owner.atom(name.name.as_str());
                self.initialize_atom(atom, class_value);
            } else {
                self.owner
                    .reject(class.span, "class declaration requires a name");
            }
        }
        class_value
    }

    fn initialize_class_binding(&mut self, binding: Atom, value: Register) {
        let slot = self.local_slots[&binding];
        self.emit(Op::StoreLocal, value, 0, u16::from(true), u32::from(slot));
    }

    fn define_class_method(
        &mut self,
        target: Register,
        function: Register,
        computed_key: Option<Register>,
        name: Option<&str>,
    ) {
        if let Some(key) = computed_key {
            self.emit(
                Op::SetFunctionNameKey,
                function,
                key,
                0,
                crate::bytecode::FUNCTION_NAME_PREFIX_NONE,
            );
        }
        let key =
            computed_key.unwrap_or_else(|| self.literal(Constant::String(name.unwrap().into())));
        let mode = if name.is_some_and(|name| name.starts_with("\0quench:private:")) {
            crate::bytecode::PropertyDefinitionMode::ReadonlyMethod
        } else {
            crate::bytecode::PropertyDefinitionMode::Method
        };
        self.emit(Op::DefinePropertyRecord, function, target, key, mode.word());
    }

    fn define_class_accessor(
        &mut self,
        target: Register,
        function: Register,
        computed_key: Option<Register>,
        name: Option<&str>,
        accessor: &str,
    ) {
        let key = computed_key.unwrap_or_else(|| {
            self.literal(Constant::String(name.expect("named class accessor").into()))
        });
        let mode = match accessor {
            "get" => crate::bytecode::PropertyDefinitionMode::Getter,
            "set" => crate::bytecode::PropertyDefinitionMode::Setter,
            _ => unreachable!("class accessor kind"),
        };
        self.emit(Op::DefinePropertyRecord, function, target, key, mode.word());
    }

    fn set_prototype(&mut self, target: Register, prototype: Register) {
        let object = self.load_name("Object");
        let setter = self.reg();
        let atom = self.owner.atom("setPrototypeOf");
        let cache = self.owner.cache_site();
        self.emit(
            Op::GetField,
            setter,
            FieldBase::register(object).0,
            cache,
            atom,
        );
        let base = self.next_reg;
        let target_arg = self.reg();
        self.emit(Op::Move, target_arg, target, 0, 0);
        let prototype_arg = self.reg();
        self.emit(Op::Move, prototype_arg, prototype, 0, 0);
        let result = self.reg();
        self.emit(
            Op::Call,
            result,
            setter,
            object,
            crate::bytecode::ImmediateLayout::call_immediate(base, 2, false, false),
        );
    }

    pub(super) fn emit_instance_fields(
        &mut self,
        fields: &[ClassField<'_>],
        private_methods: &[Atom],
    ) {
        let this = self.reg();
        self.emit(Op::LoadThis, this, 0, 0, 0);
        let home_atom = self
            .super_home_atom
            .expect("class constructor carries its private home object");
        let home_name = self.owner.atoms[home_atom as usize].to_string();
        let home = self.load_name(&home_name);
        for atom in private_methods {
            self.emit(Op::MarkPrivateName, 0, this, home, *atom);
        }
        for field in fields {
            let span = class_field_span(*field);
            let initializer = class_field_value(*field);
            let backing = match field {
                ClassField::AutoAccessor { backing, .. } => Some(*backing),
                ClassField::Property(_) => None,
            };
            let computed_key = match field {
                ClassField::Property(field) if field.computed => {
                    Some(self.load_name(&computed_field_key_name(span.start)))
                }
                ClassField::Property(field) => self.unit_key(&field.key),
                ClassField::AutoAccessor { .. } => None,
            };
            let name = match field {
                ClassField::Property(field) if computed_key.is_none() => {
                    match class_field_storage_name(self.owner, &field.key) {
                        Some(name) => Some(self.owner.atom(&name)),
                        None => {
                            self.owner.reject(span, "class field key is unsupported");
                            continue;
                        }
                    }
                }
                _ => backing,
            };
            let value = if initializer.is_some() {
                let atom = self.owner.atom(&class_field_initializer_name(span.start));
                let callee = self.load_atom(atom);
                let value = self.reg();
                self.emit(Op::Call, value, callee, this, 0);
                value
            } else {
                self.literal(Constant::Undefined)
            };
            self.set_class_field_function_name(*field, value);
            let private = matches!(
                field,
                ClassField::Property(field)
                    if matches!(&field.key, PropertyKey::PrivateIdentifier(_))
            );
            if private {
                let field_atom = name.unwrap();
                let cache = self.owner.cache_site();
                self.emit(Op::SetField, value, this, cache, field_atom);
                self.emit(Op::MarkPrivateName, 0, this, home, field_atom);
            } else if let Some(key) = computed_key {
                self.emit(Op::DefineComputedField, value, this, key, 0);
            } else if matches!(field, ClassField::AutoAccessor { .. }) {
                let field_atom = name.unwrap();
                let cache = self.owner.cache_site();
                self.emit(Op::SetField, value, this, cache, field_atom);
            } else {
                let field_atom = name.unwrap();
                self.emit(Op::DefineField, value, this, 0, field_atom);
            }
        }
    }

    fn set_class_field_function_name(&mut self, field: ClassField<'_>, value: Register) {
        if !class_field_value(field).is_some_and(Self::anonymous_function_definition) {
            return;
        }
        let key = class_field_key(field);
        let computed_key = if class_field_computed(field) {
            Some(self.load_name(&computed_field_key_name(class_field_span(field).start)))
        } else {
            self.unit_key(key)
        };
        if let Some(key) = computed_key {
            self.emit(
                Op::SetFunctionNameKey,
                value,
                key,
                0,
                crate::bytecode::FUNCTION_NAME_PREFIX_NONE,
            );
        } else {
            let name = match key {
                PropertyKey::PrivateIdentifier(identifier) => Some(format!("#{}", identifier.name)),
                _ => class_method_name(key),
            };
            if let Some(name) = name {
                let atom = self.owner.atom(&name);
                self.emit(Op::SetFunctionName, value, 0, 0, atom);
            }
        }
    }

    fn unit_key(&mut self, key: &PropertyKey<'_>) -> Option<Register> {
        let PropertyKey::StringLiteral(value) = key else {
            return None;
        };
        match super::string::constant(value) {
            Constant::StringUnits(units) => Some(self.literal(Constant::StringUnits(units))),
            _ => None,
        }
    }
}

fn class_field_key<'a>(field: ClassField<'a>) -> &'a PropertyKey<'a> {
    match field {
        ClassField::Property(field) => &field.key,
        ClassField::AutoAccessor { accessor, .. } => &accessor.key,
    }
}

fn class_field_static(field: ClassField<'_>) -> bool {
    match field {
        ClassField::Property(field) => field.r#static,
        ClassField::AutoAccessor { accessor, .. } => accessor.r#static,
    }
}

fn class_field_computed(field: ClassField<'_>) -> bool {
    match field {
        ClassField::Property(field) => field.computed,
        ClassField::AutoAccessor { accessor, .. } => accessor.computed,
    }
}

fn class_field_span(field: ClassField<'_>) -> Span {
    match field {
        ClassField::Property(field) => field.span,
        ClassField::AutoAccessor { accessor, .. } => accessor.span,
    }
}

fn class_field_initializer_name(start: u32) -> String {
    format!("\0quench:field-initializer:{start}")
}

fn class_field_value<'a>(field: ClassField<'a>) -> Option<&'a Expression<'a>> {
    match field {
        ClassField::Property(field) => field.value.as_ref(),
        ClassField::AutoAccessor { accessor, .. } => accessor.value.as_ref(),
    }
}

impl Compiler<'_> {
    fn compile_auto_accessor_methods(
        &mut self,
        accessor: &AccessorProperty<'_>,
        scopes: &[Rc<FxHashMap<Atom, u16>>],
        parent: Option<u32>,
        home_atom: Atom,
        with_depth: u16,
    ) -> Option<(u32, u32)> {
        let backing_id = self.private_name_ids[&(accessor.span.start, accessor.span.end)];
        let prefix = " ".repeat(self.text.len().saturating_add(1));
        let source = format!(
            "{prefix}class __AutoAccessor {{ #backing; get __get() {{ return this.#backing; }} set __set(value) {{ this.#backing = value; }} }}"
        );
        let allocator = Allocator::with_capacity(source.len().saturating_mul(6));
        let parsed = Parser::new(&allocator, &source, SourceType::script()).parse();
        if parsed.stack_exhausted {
            self.errors.push(Diagnostic::stack_exhausted(self.source));
            return None;
        }
        if !parsed.diagnostics.is_empty() {
            self.reject(accessor.span, "could not lower class auto-accessor methods");
            return None;
        }
        let semantic = oxc_semantic::SemanticBuilder::new()
            .with_check_syntax_error(true)
            .build(&parsed.program);
        let generated_private_names = private_name_ids(&semantic.semantic);
        for span in generated_private_names.keys() {
            self.private_name_ids.insert(*span, backing_id);
            self.private_name_labels.insert(*span, String::new());
        }
        let Statement::ClassDeclaration(class) = &parsed.program.body[0] else {
            self.reject(accessor.span, "auto-accessor lowering produced no class");
            return None;
        };
        let methods: Vec<_> = class
            .body
            .body
            .iter()
            .filter_map(|element| match element {
                ClassElement::MethodDefinition(method)
                    if matches!(
                        method.kind,
                        MethodDefinitionKind::Get | MethodDefinitionKind::Set
                    ) =>
                {
                    Some(method.as_ref())
                }
                _ => None,
            })
            .collect();
        if methods.len() != 2 || !semantic.diagnostics.is_empty() {
            self.reject(accessor.span, "auto-accessor method lowering is invalid");
            return None;
        }
        let getter = self.compile_class_method(
            methods[0], None, scopes, parent, None, None, false, home_atom, with_depth,
        );
        let setter = self.compile_class_method(
            methods[1], None, scopes, parent, None, None, false, home_atom, with_depth,
        );
        // Generated parser names are scaffolding; the actual property key owns
        // the accessor's name, including computed and private keys.
        self.functions[getter as usize]
            .as_mut()
            .expect("compiled auto-accessor getter")
            .name = None;
        self.functions[setter as usize]
            .as_mut()
            .expect("compiled auto-accessor setter")
            .name = None;
        Some((getter, setter))
    }

    fn compile_class_method(
        &mut self,
        method: &MethodDefinition<'_>,
        source_span: Option<Span>,
        scopes: &[Rc<FxHashMap<Atom, u16>>],
        parent: Option<u32>,
        instance_fields: Option<&[ClassField<'_>]>,
        instance_private_methods: Option<&[Atom]>,
        derived_constructor: bool,
        home_atom: Atom,
        with_depth: u16,
    ) -> u32 {
        let params = FunctionCompiler::params_from_formals(&method.value.params, self);
        let body = method
            .value
            .body
            .as_ref()
            .map_or(&[][..], |body| body.statements.as_slice());
        let name = if method.kind == MethodDefinitionKind::Constructor {
            None
        } else {
            let name = match &method.key {
                PropertyKey::PrivateIdentifier(identifier) => {
                    Some(format!("#{}", identifier.name.as_str()))
                }
                _ => class_method_name(&method.key),
            };
            match (method.kind, name) {
                (MethodDefinitionKind::Get, Some(name)) => Some(format!("get {name}")),
                (MethodDefinitionKind::Set, Some(name)) => Some(format!("set {name}")),
                (_, name) => name,
            }
        };
        self.compile_function(
            name.as_deref(),
            &params,
            FunctionBody::Statements(body),
            scopes,
            parent,
            FunctionOptions {
                defaults: Some(&method.value.params),
                source_text: source_span.and_then(|span| {
                    class_method_source_span(self.text, span, method.key.span().start)
                        .and_then(|span| self.source_text(span))
                }),
                name_binding: None,
                async_function: method.value.r#async,
                generator: method.value.generator,
                class_constructor: method.kind == MethodDefinitionKind::Constructor,
                derived_constructor: method.kind == MethodDefinitionKind::Constructor
                    && derived_constructor,
                non_constructible: method.kind != MethodDefinitionKind::Constructor,
                class_field_initializer: false,
                instance_fields,
                instance_private_methods,
                super_static: method.r#static,
                super_home: true,
                super_home_atom: Some(home_atom),
                rest_override: false,
                implicit_super: false,
                with_depth,
                strict: true,
            },
        )
    }

    fn compile_class_static_block(
        &mut self,
        block: &StaticBlock<'_>,
        scopes: &[Rc<FxHashMap<Atom, u16>>],
        parent: Option<u32>,
        with_depth: u16,
    ) -> u32 {
        self.compile_function(
            None,
            &[],
            FunctionBody::Statements(&block.body),
            scopes,
            parent,
            FunctionOptions {
                defaults: None,
                source_text: None,
                name_binding: None,
                async_function: false,
                generator: false,
                class_constructor: false,
                derived_constructor: false,
                non_constructible: false,
                class_field_initializer: false,
                instance_fields: None,
                instance_private_methods: None,
                super_static: true,
                super_home: false,
                super_home_atom: None,
                rest_override: false,
                implicit_super: false,
                with_depth,
                strict: false,
            },
        )
    }
}

fn class_method_source_span(mut text: &str, span: Span, key_start: u32) -> Option<Span> {
    let mut start = span.start as usize;
    let key_start = key_start as usize;
    if start > key_start || key_start > text.len() {
        return None;
    }
    text = text.get(start..key_start)?;
    let leading = skip_source_trivia(text, 0);
    start += leading;
    text = text.get(leading..)?;
    if text.starts_with("static")
        && text
            .as_bytes()
            .get("static".len())
            .is_none_or(|byte| !is_identifier_continue(*byte))
    {
        let after_static = "static".len();
        start += skip_source_trivia(text, after_static);
    }
    Some(Span::new(u32::try_from(start).ok()?, span.end))
}

fn skip_source_trivia(text: &str, mut offset: usize) -> usize {
    const COMMENT_PREFIX_LEN: usize = 2;
    const BLOCK_COMMENT_SUFFIX_LEN: usize = 2;
    let bytes = text.as_bytes();
    loop {
        while bytes
            .get(offset)
            .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            offset += 1;
        }
        match (bytes.get(offset), bytes.get(offset + 1)) {
            (Some(b'/'), Some(b'*')) => {
                let Some(end) = text
                    .get(offset + COMMENT_PREFIX_LEN..)
                    .and_then(|tail| tail.find("*/"))
                else {
                    return offset;
                };
                offset += end + COMMENT_PREFIX_LEN + BLOCK_COMMENT_SUFFIX_LEN;
            }
            (Some(b'/'), Some(b'/')) => {
                let end = text
                    .get(offset + COMMENT_PREFIX_LEN..)
                    .and_then(|tail| tail.find(['\n', '\r']))
                    .map_or(text.len(), |end| offset + COMMENT_PREFIX_LEN + end);
                offset = end;
            }
            _ => return offset,
        }
    }
}

fn is_identifier_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$')
}

fn class_method_name(key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
        PropertyKey::PrivateIdentifier(id) => Some(id.name.to_string()),
        PropertyKey::StringLiteral(value)
            if !matches!(super::string::constant(value), Constant::StringUnits(_)) =>
        {
            Some(value.value.to_string())
        }
        PropertyKey::NumericLiteral(value) => Some(crate::number_to_string::format(value.value)),
        PropertyKey::BigIntLiteral(value) => Some(value.value.to_string()),
        _ => None,
    }
}

fn class_field_storage_name(compiler: &mut Compiler<'_>, key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::PrivateIdentifier(identifier) => {
            Some(compiler.private_name_text(identifier.span))
        }
        _ => class_method_name(key),
    }
}

pub(super) fn computed_field_key_name(start: u32) -> String {
    format!("\0quench:computed-field-key:{start}")
}
