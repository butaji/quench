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
        let class_binding = class.id.as_ref().map(|identifier| {
            let source = self.owner.atom(identifier.name.as_str());
            let binding = self.hidden_local(&format!(
                "{}\0rqj:class-binding:{}",
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
            let atom = self.hidden_local(&format!("\0rqj:class-super:{}", class.span.start));
            let super_binding = self.owner.atom("\0rqj:super");
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
            if let ClassElement::PropertyDefinition(field) = element
                && field.r#static
                && field.value.is_some()
            {
                self.hidden_local(&class_field_eval_marker_name(field.span.start));
            }
            if let ClassElement::AccessorProperty(accessor) = element {
                self.owner.reserve_auto_accessor_name(accessor.span);
                if !accessor.decorators.is_empty() {
                    self.owner
                        .reject(accessor.span, "decorated class accessors are unsupported");
                }
                if accessor.r#static && accessor.value.is_some() {
                    self.hidden_local(&class_field_eval_marker_name(accessor.span.start));
                }
            }
        }
        let mut method_home_atoms = FxHashMap::default();
        for element in &class.body.body {
            if let ClassElement::MethodDefinition(method) = element {
                let atom = self.hidden_local(&format!("\0rqj:home:{}", method.span.start));
                method_home_atoms.insert(method.span.start, atom);
            }
        }
        let instance_fields: Vec<_> = class
            .body
            .body
            .iter()
            .filter_map(|element| match element {
                ClassElement::PropertyDefinition(field) if !field.r#static => {
                    Some(ClassField::Property(field.as_ref()))
                }
                ClassElement::AccessorProperty(accessor) if !accessor.r#static => {
                    let backing = self.owner.private_name_atom(accessor.span);
                    Some(ClassField::AutoAccessor { accessor, backing })
                }
                _ => None,
            })
            .collect();
        let instance_private_method_spans: Vec<_> = class
            .body
            .body
            .iter()
            .filter_map(|element| match element {
                ClassElement::MethodDefinition(method)
                    if !method.r#static
                        && matches!(&method.key, PropertyKey::PrivateIdentifier(_)) =>
                {
                    Some(method.key.span())
                }
                ClassElement::AccessorProperty(accessor) if !accessor.r#static => {
                    Some(accessor.span)
                }
                _ => None,
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
                Some(method)
            }
            _ => None,
        });
        let constructor_home_atom = constructor
            .map(|method| method_home_atoms[&method.span.start])
            .unwrap_or_else(|| {
                self.hidden_local(&format!(
                    "\0rqj:home:implicit-constructor:{}",
                    class.span.start
                ))
            });
        let class_home_atom = self.hidden_local(&format!("\0rqj:home:class:{}", class.span.start));
        let scopes = self.capture_scopes();
        let implicit_super = constructor.is_none() && heritage.is_some();
        let constructor_id = constructor
            .map(|method| {
                self.owner.compile_class_method(
                    method,
                    &scopes,
                    Some(self.function_id),
                    Some(&instance_fields),
                    Some(&instance_private_methods),
                    heritage.is_some(),
                    method_home_atoms[&method.span.start],
                    self.with_depth,
                )
            })
            .unwrap_or_else(|| {
                let params = if implicit_super {
                    vec!["\0rqj:derived-args".to_owned()]
                } else {
                    vec![]
                };
                self.owner.compile_function(
                    None,
                    &params,
                    &[],
                    &scopes,
                    Some(self.function_id),
                    FunctionOptions {
                        defaults: None,
                        name_binding: None,
                        async_function: false,
                        generator: false,
                        class_constructor: true,
                        derived_constructor: false,
                        non_constructible: false,
                        class_field_initializer: false,
                        instance_fields: Some(&instance_fields),
                        instance_private_methods: Some(&instance_private_methods),
                        defer_instance_fields: false,
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
        if let Some(binding) = class_binding {
            self.initialize_class_binding(binding, class_value);
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
            if let Some(name) = name {
                let target = if is_static { class_value } else { prototype };
                let atom = self.owner.private_name_atom(name);
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
            self.set_prototype(class_value, base);
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
            let raw_key = self.expression(key);
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
                    let Some(name) = class_method_name(&accessor.key) else {
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
                        let atom = self.owner.atom(&format!("{kind} {name}"));
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

        for element in &class.body.body {
            match element {
                ClassElement::PropertyDefinition(field) if field.r#static => {
                    let computed_key = if field.computed {
                        Some(self.load_name(&computed_field_key_name(field.span.start)))
                    } else {
                        Self::unit_key(self, &field.key)
                    };
                    let name = if computed_key.is_none() {
                        let Some(name) = class_field_storage_name(self.owner, &field.key) else {
                            self.owner
                                .reject(field.span, "class field key is unsupported");
                            continue;
                        };
                        Some(self.owner.atom(&name))
                    } else {
                        None
                    };
                    let previous_this = self.this_override.replace(class_value);
                    let previous_field_initializer =
                        std::mem::replace(&mut self.class_field_initializer, true);
                    let previous_super_static = std::mem::replace(&mut self.super_static, true);
                    let previous_super_home = std::mem::replace(&mut self.super_home, true);
                    let previous_super_home_atom = self.super_home_atom.replace(class_home_atom);
                    let eval_marker = self
                        .owner
                        .atom(&class_field_eval_marker_name(field.span.start));
                    let active = self.literal(Constant::Boolean(true));
                    self.store_atom(eval_marker, active);
                    let value = match field.value.as_ref() {
                        Some(value) => self.expression(value),
                        None => self.literal(Constant::Undefined),
                    };
                    if field
                        .value
                        .as_ref()
                        .is_some_and(Self::anonymous_function_definition)
                    {
                        let name = match &field.key {
                            PropertyKey::PrivateIdentifier(identifier) => {
                                Some(self.owner.atom(&format!("#{}", identifier.name.as_str())))
                            }
                            _ => name,
                        };
                        if let Some(name) = name {
                            self.emit(Op::SetFunctionName, value, 0, 0, name);
                        } else if let Some(key) = computed_key {
                            self.emit(
                                Op::SetFunctionNameKey,
                                value,
                                key,
                                0,
                                crate::bytecode::FUNCTION_NAME_PREFIX_NONE,
                            );
                        }
                    }
                    let inactive = self.literal(Constant::Boolean(false));
                    self.store_atom(eval_marker, inactive);
                    self.this_override = previous_this;
                    self.class_field_initializer = previous_field_initializer;
                    self.super_static = previous_super_static;
                    self.super_home = previous_super_home;
                    self.super_home_atom = previous_super_home_atom;
                    if let Some(key) = computed_key {
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
                self.store_atom(atom, class_value);
            } else {
                self.owner
                    .reject(class.span, "class declaration requires a name");
            }
        }
        class_value
    }

    fn initialize_class_binding(&mut self, binding: Atom, value: Register) {
        let slot = self.local_slots[&binding];
        self.emit(Op::StoreLocal, value, 0, 0, u32::from(slot));
    }

    fn define_class_method(
        &mut self,
        target: Register,
        function: Register,
        computed_key: Option<Register>,
        name: Option<&str>,
    ) {
        let descriptor = self.reg();
        self.emit(Op::MakeObject, descriptor, 0, 0, 0);
        for (field, value) in [
            ("value", function),
            (
                "writable",
                self.literal(Constant::Boolean(
                    !name.is_some_and(|name| name.starts_with("\0rqj:private:")),
                )),
            ),
            ("enumerable", self.literal(Constant::Boolean(false))),
            ("configurable", self.literal(Constant::Boolean(true))),
        ] {
            let atom = self.owner.atom(field);
            let cache = self.owner.cache_site();
            self.emit(Op::SetField, value, descriptor, cache, atom);
        }
        let object = self.load_name("Object");
        let define = self.reg();
        let define_atom = self.owner.atom("defineProperty");
        let define_cache = self.owner.cache_site();
        self.emit(
            Op::GetField,
            define,
            FieldBase::register(object).0,
            define_cache,
            define_atom,
        );
        let key =
            computed_key.unwrap_or_else(|| self.literal(Constant::String(name.unwrap().into())));
        let base = self.next_reg;
        let target_arg = self.reg();
        self.emit(Op::Move, target_arg, target, 0, 0);
        let key_arg = self.reg();
        self.emit(Op::Move, key_arg, key, 0, 0);
        let descriptor_arg = self.reg();
        self.emit(Op::Move, descriptor_arg, descriptor, 0, 0);
        let result = self.reg();
        self.emit(
            Op::Call,
            result,
            define,
            object,
            crate::bytecode::ImmediateLayout::call_immediate(base, 3, false, false),
        );
    }

    fn define_class_accessor(
        &mut self,
        target: Register,
        function: Register,
        computed_key: Option<Register>,
        name: Option<&str>,
        accessor: &str,
    ) {
        let descriptor = self.reg();
        self.emit(Op::MakeObject, descriptor, 0, 0, 0);
        let accessor_atom = self.owner.atom(accessor);
        let descriptor_cache = self.owner.cache_site();
        self.emit(
            Op::SetField,
            function,
            descriptor,
            descriptor_cache,
            accessor_atom,
        );
        for (field, value) in [
            ("enumerable", self.literal(Constant::Boolean(false))),
            ("configurable", self.literal(Constant::Boolean(true))),
        ] {
            let atom = self.owner.atom(field);
            let cache = self.owner.cache_site();
            self.emit(Op::SetField, value, descriptor, cache, atom);
        }

        let object = self.load_name("Object");
        let define = self.reg();
        let define_atom = self.owner.atom("defineProperty");
        let define_cache = self.owner.cache_site();
        self.emit(
            Op::GetField,
            define,
            FieldBase::register(object).0,
            define_cache,
            define_atom,
        );
        let key = computed_key.unwrap_or_else(|| {
            self.literal(Constant::String(name.expect("named class accessor").into()))
        });
        let base = self.next_reg;
        let target_arg = self.reg();
        self.emit(Op::Move, target_arg, target, 0, 0);
        let key_arg = self.reg();
        self.emit(Op::Move, key_arg, key, 0, 0);
        let descriptor_arg = self.reg();
        self.emit(Op::Move, descriptor_arg, descriptor, 0, 0);
        let result = self.reg();
        self.emit(
            Op::Call,
            result,
            define,
            object,
            crate::bytecode::ImmediateLayout::call_immediate(base, 3, false, false),
        );
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
        for field in fields
            .iter()
            .filter(|field| class_field_value(**field).is_some())
        {
            self.hidden_local(&class_field_eval_marker_name(
                class_field_span(*field).start,
            ));
        }
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
            let eval_marker = self.owner.atom(&class_field_eval_marker_name(span.start));
            let has_initializer = initializer.is_some();
            if has_initializer {
                let active = self.literal(Constant::Boolean(true));
                self.store_atom(eval_marker, active);
            }
            let previous_field_initializer = if has_initializer {
                Some(std::mem::replace(&mut self.class_field_initializer, true))
            } else {
                None
            };
            let value = match initializer {
                Some(value) => self.expression(value),
                None => self.literal(Constant::Undefined),
            };
            if initializer.is_some_and(Self::anonymous_function_definition) {
                let name = match field {
                    ClassField::Property(field) => match &field.key {
                        PropertyKey::PrivateIdentifier(identifier) => {
                            Some(self.owner.atom(&format!("#{}", identifier.name.as_str())))
                        }
                        _ => name,
                    },
                    ClassField::AutoAccessor { accessor, .. } => {
                        class_method_name(&accessor.key).map(|name| self.owner.atom(&name))
                    }
                };
                if let Some(name) = name {
                    self.emit(Op::SetFunctionName, value, 0, 0, name);
                } else if let Some(key) = computed_key {
                    self.emit(
                        Op::SetFunctionNameKey,
                        value,
                        key,
                        0,
                        crate::bytecode::FUNCTION_NAME_PREFIX_NONE,
                    );
                }
            }
            if has_initializer {
                let inactive = self.literal(Constant::Boolean(false));
                self.store_atom(eval_marker, inactive);
            }
            if let Some(previous) = previous_field_initializer {
                self.class_field_initializer = previous;
            }
            if let Some(key) = computed_key {
                self.emit(Op::SetIndex, value, this, key, 0);
            } else {
                let field_atom = name.unwrap();
                let cache = self.owner.cache_site();
                self.emit(Op::SetField, value, this, cache, field_atom);
                if matches!(field, ClassField::Property(field) if matches!(&field.key, PropertyKey::PrivateIdentifier(_)))
                {
                    self.emit(Op::MarkPrivateName, 0, this, home, field_atom);
                }
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

fn class_field_span(field: ClassField<'_>) -> Span {
    match field {
        ClassField::Property(field) => field.span,
        ClassField::AutoAccessor { accessor, .. } => accessor.span,
    }
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
            methods[0], scopes, parent, None, None, false, home_atom, with_depth,
        );
        let setter = self.compile_class_method(
            methods[1], scopes, parent, None, None, false, home_atom, with_depth,
        );
        Some((getter, setter))
    }

    fn compile_class_method(
        &mut self,
        method: &MethodDefinition<'_>,
        scopes: &[Rc<FxHashMap<Atom, u16>>],
        parent: Option<u32>,
        instance_fields: Option<&[ClassField<'_>]>,
        instance_private_methods: Option<&[Atom]>,
        defer_instance_fields: bool,
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
            match &method.key {
                PropertyKey::PrivateIdentifier(identifier) => {
                    Some(format!("#{}", identifier.name.as_str()))
                }
                _ => class_method_name(&method.key),
            }
        };
        self.compile_function(
            name.as_deref(),
            &params,
            body,
            scopes,
            parent,
            FunctionOptions {
                defaults: Some(&method.value.params),
                name_binding: None,
                async_function: method.value.r#async,
                generator: method.value.generator,
                class_constructor: method.kind == MethodDefinitionKind::Constructor,
                derived_constructor: method.kind == MethodDefinitionKind::Constructor
                    && defer_instance_fields,
                non_constructible: method.kind != MethodDefinitionKind::Constructor,
                class_field_initializer: false,
                instance_fields,
                instance_private_methods,
                defer_instance_fields,
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
            &block.body,
            scopes,
            parent,
            FunctionOptions {
                defaults: None,
                name_binding: None,
                async_function: false,
                generator: false,
                class_constructor: false,
                derived_constructor: false,
                non_constructible: false,
                class_field_initializer: false,
                instance_fields: None,
                instance_private_methods: None,
                defer_instance_fields: false,
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
    format!("\0rqj:computed-field-key:{start}")
}

pub(super) fn class_field_eval_marker_name(start: u32) -> String {
    format!("\0rqj:class-field-eval:{start}")
}
