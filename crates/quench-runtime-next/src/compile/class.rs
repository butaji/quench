use super::*;

impl FunctionCompiler<'_, '_> {
    pub(super) fn class_expression(&mut self, class: &Class<'_>) -> Register {
        self.lower_class(class, false)
    }

    pub(super) fn class_declaration(&mut self, class: &Class<'_>) {
        self.lower_class(class, true);
    }

    fn lower_class(&mut self, class: &Class<'_>, bind_name: bool) -> Register {
        let heritage = class
            .heritage
            .as_ref()
            .map(|heritage| self.expression(&heritage.expression));
        let super_atom = heritage.map(|_| self.hidden_local("\0rqj:super"));
        let scopes = self.capture_scopes();
        let instance_fields: Vec<_> = class
            .body
            .body
            .iter()
            .filter_map(|element| match element {
                ClassElement::PropertyDefinition(field) if !field.r#static => Some(field.as_ref()),
                _ => None,
            })
            .collect();
        let constructor = class.body.body.iter().find_map(|element| match element {
            ClassElement::MethodDefinition(method)
                if method.kind == MethodDefinitionKind::Constructor =>
            {
                Some(method)
            }
            _ => None,
        });
        let implicit_super = constructor.is_none() && heritage.is_some();
        let constructor_id = constructor
            .map(|method| {
                self.owner.compile_class_method(
                    method,
                    &scopes,
                    Some(self.function_id),
                    Some(&instance_fields),
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
                        async_function: false,
                        generator: false,
                        instance_fields: Some(&instance_fields),
                        super_static: false,
                        rest_override: implicit_super,
                        implicit_super,
                    },
                )
            });
        let class_value = self.reg();
        self.emit(Op::MakeClosure, class_value, 0, 0, constructor_id);

        if let (Some(base), Some(super_atom)) = (heritage, super_atom) {
            self.store_atom(super_atom, base);
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
        let constructor_atom = self.owner.atom("constructor");
        let constructor_cache = self.owner.cache_site();
        self.emit(
            Op::SetField,
            class_value,
            prototype,
            constructor_cache,
            constructor_atom,
        );

        if let Some(base) = heritage {
            let base_prototype = self.reg();
            let prototype_atom = self.owner.atom("prototype");
            let prototype_cache = self.owner.cache_site();
            self.emit(
                Op::GetField,
                base_prototype,
                FieldBase::register(base).0,
                prototype_cache,
                prototype_atom,
            );
            self.set_prototype(prototype, base_prototype);
            self.set_prototype(class_value, base);
        }

        for element in &class.body.body {
            let ClassElement::MethodDefinition(method) = element else {
                if !matches!(
                    element,
                    ClassElement::PropertyDefinition(_) | ClassElement::StaticBlock(_)
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
            let computed_key = if method.computed {
                let Some(key) = method.key.as_expression() else {
                    self.owner
                        .reject(method.span, "computed class method key is unsupported");
                    continue;
                };
                Some(self.expression(key))
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
                    if matches!(&method.key, PropertyKey::PrivateIdentifier(_)) {
                        format!("#{name}")
                    } else {
                        name.to_owned()
                    },
                )
            } else {
                None
            };
            let name = name_text.as_deref().map(|name| self.owner.atom(name));
            let function_id =
                self.owner
                    .compile_class_method(method, &scopes, Some(self.function_id), None);
            let function = self.reg();
            self.emit(Op::MakeClosure, function, 0, 0, function_id);
            let target = if method.r#static {
                class_value
            } else {
                prototype
            };
            if let Some(accessor) = accessor_name {
                self.define_class_accessor(
                    target,
                    function,
                    computed_key,
                    name_text.as_deref(),
                    accessor,
                );
            } else if let Some(key) = computed_key {
                self.emit(Op::SetIndex, function, target, key, 0);
            } else {
                let cache = self.owner.cache_site();
                self.emit(Op::SetField, function, target, cache, name.unwrap());
            }
        }

        for element in &class.body.body {
            match element {
                ClassElement::PropertyDefinition(field) if field.r#static => {
                    let computed_key = if field.computed {
                        let Some(key) = field.key.as_expression() else {
                            self.owner
                                .reject(field.span, "computed class field key is unsupported");
                            continue;
                        };
                        Some(self.expression(key))
                    } else {
                        Self::unit_key(self, &field.key)
                    };
                    let name = if computed_key.is_none() {
                        let Some(name) = class_method_name(&field.key) else {
                            self.owner
                                .reject(field.span, "class field key is unsupported");
                            continue;
                        };
                        Some(self.owner.atom(name))
                    } else {
                        None
                    };
                    let value = match field.value.as_ref() {
                        Some(value) => self.expression(value),
                        None => self.literal(Constant::Undefined),
                    };
                    if let Some(key) = computed_key {
                        self.emit(Op::SetIndex, value, class_value, key, 0);
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
                    );
                    let function = self.reg();
                    self.emit(Op::MakeClosure, function, 0, 0, function_id);
                    let result = self.reg();
                    self.emit(Op::Call, result, function, class_value, 0);
                }
                _ => {}
            }
        }
        class_value
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
            (u32::from(base) << 16) | 3,
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
            (u32::from(base) << 16) | 2,
        );
    }

    fn capture_scopes(&self) -> Vec<Rc<FxHashMap<Atom, u16>>> {
        let mut scopes = vec![Rc::clone(&self.local_slots)];
        scopes.extend(self.scopes.iter().cloned());
        scopes
    }

    pub(super) fn emit_instance_fields(&mut self, fields: &[&PropertyDefinition<'_>]) {
        let this = self.reg();
        self.emit(Op::LoadThis, this, 0, 0, 0);
        for field in fields {
            let computed_key = if field.computed {
                let Some(key) = field.key.as_expression() else {
                    self.owner
                        .reject(field.span, "computed class field key is unsupported");
                    continue;
                };
                Some(self.expression(key))
            } else {
                self.unit_key(&field.key)
            };
            let name = if computed_key.is_none() {
                let Some(name) = class_method_name(&field.key) else {
                    self.owner
                        .reject(field.span, "class field key is unsupported");
                    continue;
                };
                Some(self.owner.atom(name))
            } else {
                None
            };
            let value = match field.value.as_ref() {
                Some(value) => self.expression(value),
                None => self.literal(Constant::Undefined),
            };
            if let Some(key) = computed_key {
                self.emit(Op::SetIndex, value, this, key, 0);
            } else {
                let cache = self.owner.cache_site();
                self.emit(Op::SetField, value, this, cache, name.unwrap());
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

impl Compiler<'_> {
    fn compile_class_method(
        &mut self,
        method: &MethodDefinition<'_>,
        scopes: &[Rc<FxHashMap<Atom, u16>>],
        parent: Option<u32>,
        instance_fields: Option<&[&PropertyDefinition<'_>]>,
    ) -> u32 {
        let params = FunctionCompiler::params_from_formals(&method.value.params, self);
        let body = method
            .value
            .body
            .as_ref()
            .map_or(&[][..], |body| body.statements.as_slice());
        let name = class_method_name(&method.key);
        self.compile_function(
            name,
            &params,
            body,
            scopes,
            parent,
            FunctionOptions {
                defaults: Some(&method.value.params),
                async_function: method.value.r#async,
                generator: method.value.generator,
                instance_fields,
                super_static: method.r#static,
                rest_override: false,
                implicit_super: false,
            },
        )
    }

    fn compile_class_static_block(
        &mut self,
        block: &StaticBlock<'_>,
        scopes: &[Rc<FxHashMap<Atom, u16>>],
        parent: Option<u32>,
    ) -> u32 {
        self.compile_function(
            None,
            &[],
            &block.body,
            scopes,
            parent,
            FunctionOptions {
                defaults: None,
                async_function: false,
                generator: false,
                instance_fields: None,
                super_static: true,
                rest_override: false,
                implicit_super: false,
            },
        )
    }
}

fn class_method_name<'a>(key: &'a PropertyKey<'a>) -> Option<&'a str> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.as_str()),
        PropertyKey::PrivateIdentifier(id) => Some(id.name.as_str()),
        PropertyKey::StringLiteral(value)
            if !matches!(super::string::constant(value), Constant::StringUnits(_)) =>
        {
            Some(value.value.as_str())
        }
        _ => None,
    }
}
