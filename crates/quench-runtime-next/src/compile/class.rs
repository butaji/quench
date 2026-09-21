use super::*;

impl FunctionCompiler<'_, '_> {
    pub(super) fn class_expression(&mut self, class: &Class<'_>) -> Register {
        self.lower_class(class, false)
    }

    pub(super) fn class_declaration(&mut self, class: &Class<'_>) {
        self.lower_class(class, true);
    }

    fn lower_class(&mut self, class: &Class<'_>, bind_name: bool) -> Register {
        if class.heritage.is_some() {
            self.owner
                .reject(class.span, "class heritage is not supported yet");
        }
        let scopes = self.capture_scopes();
        let constructor = class.body.body.iter().find_map(|element| match element {
            ClassElement::MethodDefinition(method)
                if method.kind == MethodDefinitionKind::Constructor =>
            {
                Some(method)
            }
            _ => None,
        });
        let constructor_id = constructor
            .map(|method| {
                self.owner
                    .compile_class_method(method, &scopes, Some(self.function_id))
            })
            .unwrap_or_else(|| {
                self.owner
                    .compile_function(None, &[], &[], &scopes, Some(self.function_id))
            });
        let class_value = self.reg();
        self.emit(Op::MakeClosure, class_value, 0, 0, constructor_id);

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

        for element in &class.body.body {
            let ClassElement::MethodDefinition(method) = element else {
                self.owner.reject(
                    element.span(),
                    "class fields and static blocks are unsupported",
                );
                continue;
            };
            if method.kind == MethodDefinitionKind::Constructor {
                continue;
            }
            if method.kind != MethodDefinitionKind::Method || method.computed {
                self.owner.reject(
                    method.span,
                    "class accessors and computed methods are unsupported",
                );
                continue;
            }
            let Some(name) = class_method_name(&method.key) else {
                self.owner
                    .reject(method.span, "class method key is unsupported");
                continue;
            };
            let function_id =
                self.owner
                    .compile_class_method(method, &scopes, Some(self.function_id));
            let function = self.reg();
            self.emit(Op::MakeClosure, function, 0, 0, function_id);
            let target = if method.r#static {
                class_value
            } else {
                prototype
            };
            let atom = self.owner.atom(name);
            let cache = self.owner.cache_site();
            self.emit(Op::SetField, function, target, cache, atom);
        }
        class_value
    }

    fn capture_scopes(&self) -> Vec<Rc<FxHashMap<Atom, u16>>> {
        let mut scopes = vec![Rc::clone(&self.local_slots)];
        scopes.extend(self.scopes.iter().cloned());
        scopes
    }
}

impl Compiler<'_> {
    fn compile_class_method(
        &mut self,
        method: &MethodDefinition<'_>,
        scopes: &[Rc<FxHashMap<Atom, u16>>],
        parent: Option<u32>,
    ) -> u32 {
        let params = FunctionCompiler::params_from_formals(&method.value.params, self);
        let body = method
            .value
            .body
            .as_ref()
            .map_or(&[][..], |body| body.statements.as_slice());
        let name = class_method_name(&method.key);
        self.compile_function(name, &params, body, scopes, parent)
    }
}

fn class_method_name<'a>(key: &'a PropertyKey<'a>) -> Option<&'a str> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.as_str()),
        PropertyKey::StringLiteral(value) => Some(value.value.as_str()),
        _ => None,
    }
}
