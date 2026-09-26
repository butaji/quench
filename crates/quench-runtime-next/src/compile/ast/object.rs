use super::*;

impl FunctionCompiler<'_, '_> {
    pub(super) fn object_expression(&mut self, value: &ObjectExpression<'_>) -> Register {
        let dst = self.reg();
        if let [
            ObjectPropertyKind::ObjectProperty(first),
            ObjectPropertyKind::ObjectProperty(second),
        ] = value.properties.as_slice()
            && first.kind == PropertyKind::Init
            && second.kind == PropertyKind::Init
            && !(Self::static_key(&first.key) == Some("__proto__") && !first.shorthand)
            && !(Self::static_key(&second.key) == Some("__proto__") && !second.shorthand)
            && !Self::anonymous_function_definition(&first.value)
            && !Self::anonymous_function_definition(&second.value)
            && let (Some(first_key), Some(second_key)) =
                (Self::static_key(&first.key), Self::static_key(&second.key))
        {
            let first_value = self.expression(&first.value);
            let second_value = self.expression(&second.value);
            let site = self.owner.object_sites.len() as u32;
            let atoms = [self.owner.atom(first_key), self.owner.atom(second_key)];
            self.owner.object_sites.push(ObjectSite { atoms });
            self.emit(Op::MakeObject2, dst, first_value, second_value, site);
            return dst;
        }
        self.emit(Op::MakeObject, dst, 0, 0, 0);
        let super_atom = self.hidden_local("\0rqj:super");
        self.store_atom(super_atom, dst);
        for property in &value.properties {
            let property = match property {
                ObjectPropertyKind::ObjectProperty(property) => property,
                ObjectPropertyKind::SpreadProperty(spread) => {
                    self.object_spread(dst, &spread.argument);
                    continue;
                }
            };
            if property.kind == PropertyKind::Init
                && !property.computed
                && !property.shorthand
                && Self::static_key(&property.key) == Some("__proto__")
            {
                let prototype = self.expression(&property.value);
                self.object_literal_prototype(dst, prototype);
                continue;
            }
            if let Some(accessor) = match property.kind {
                PropertyKind::Get => Some("get"),
                PropertyKind::Set => Some("set"),
                PropertyKind::Init => None,
            } {
                let computed = property.computed;
                let key = if computed {
                    self.computed_object_key(&property.key).map(|raw_key| {
                        let key = self.reg();
                        self.emit(Op::ToPropertyKey, key, raw_key, 0, 0);
                        key
                    })
                } else {
                    Self::static_key(&property.key)
                        .map(|name| self.literal(Constant::String(name.into())))
                        .or_else(|| {
                            property.key.as_expression().map(|expression| {
                                let raw_key = self.expression(expression);
                                let key = self.reg();
                                self.emit(Op::ToPropertyKey, key, raw_key, 0, 0);
                                key
                            })
                        })
                };
                let Some(key) = key else {
                    self.owner
                        .reject(property.span, "object accessor key unsupported");
                    continue;
                };
                let item = self.object_method(&property.value, super_atom, property.span);
                if computed {
                    self.emit(
                        Op::SetFunctionNameKey,
                        item,
                        key,
                        0,
                        if accessor == "get" { 1 } else { 2 },
                    );
                } else if let Some(name) = Self::static_key(&property.key) {
                    let name = self.owner.atom(&format!("{accessor} {name}"));
                    self.emit(Op::SetFunctionName, item, 0, 0, name);
                } else {
                    self.emit(
                        Op::SetFunctionNameKey,
                        item,
                        key,
                        0,
                        if accessor == "get" { 1 } else { 2 },
                    );
                }
                self.define_accessor(dst, key, item, accessor);
                continue;
            }
            if property.computed {
                let Some(key) = self.computed_object_key(&property.key) else {
                    self.owner
                        .reject(property.span, "computed object key expression unsupported");
                    continue;
                };
                let property_key = self.reg();
                self.emit(Op::ToPropertyKey, property_key, key, 0, 0);
                let item = self.object_property_value(
                    &property.value,
                    property.method,
                    super_atom,
                    property.span,
                );
                if Self::anonymous_function_definition(&property.value) {
                    self.emit(Op::SetFunctionNameKey, item, property_key, 0, 0);
                }
                self.emit(Op::DefineComputedField, item, dst, property_key, 0);
                continue;
            }
            if let PropertyKey::StringLiteral(value) = &property.key {
                let key = super::super::string::constant(value);
                if matches!(&key, Constant::StringUnits(_)) {
                    let key = self.literal(key);
                    let item = self.object_property_value(
                        &property.value,
                        property.method,
                        super_atom,
                        property.span,
                    );
                    if Self::anonymous_function_definition(&property.value) {
                        self.emit(Op::SetFunctionNameKey, item, key, 0, 0);
                    }
                    self.emit(Op::DefineComputedField, item, dst, key, 0);
                    continue;
                }
            }
            let key = match &property.key {
                PropertyKey::StaticIdentifier(id) => id.name.as_str(),
                PropertyKey::StringLiteral(value) => value.value.as_str(),
                _ => {
                    if let Some(expression) = property.key.as_expression() {
                        let key = self.expression(expression);
                        let item = self.object_property_value(
                            &property.value,
                            property.method,
                            super_atom,
                            property.span,
                        );
                        if Self::anonymous_function_definition(&property.value) {
                            let property_key = self.reg();
                            self.emit(Op::ToPropertyKey, property_key, key, 0, 0);
                            self.emit(Op::SetFunctionNameKey, item, property_key, 0, 0);
                            self.emit(Op::DefineComputedField, item, dst, property_key, 0);
                            continue;
                        }
                        self.emit(Op::DefineComputedField, item, dst, key, 0);
                        continue;
                    }
                    self.owner
                        .reject(property.span, "computed object keys are unsupported");
                    continue;
                }
            };
            let item = self.object_property_value(
                &property.value,
                property.method,
                super_atom,
                property.span,
            );
            let atom = self.owner.atom(key);
            if Self::anonymous_function_definition(&property.value) {
                self.emit(Op::SetFunctionName, item, 0, 0, atom);
            }
            self.emit(Op::DefineField, item, dst, 0, atom);
        }
        dst
    }

    fn object_literal_prototype(&mut self, object: Register, prototype: Register) {
        let callee = self.load_name("\0rqj:object-literal-prototype");
        let this = self.literal(Constant::Undefined);
        let start = self.next_reg;
        for argument in [object, prototype] {
            let slot = self.reg();
            self.emit(Op::Move, slot, argument, 0, 0);
        }
        let result = self.reg();
        self.emit(
            Op::Call,
            result,
            callee,
            this,
            crate::bytecode::ImmediateLayout::call_immediate(start, 2, false, false),
        );
    }

    fn object_method(
        &mut self,
        value: &Expression<'_>,
        super_atom: Atom,
        source_span: Span,
    ) -> Register {
        let Expression::FunctionExpression(function) = value else {
            return self.expression(value);
        };
        let params = Self::params_from_formals(&function.params, self.owner);
        let body = function
            .body
            .as_ref()
            .map_or(&[][..], |body| body.statements.as_slice());
        let scopes = self.capture_scopes();
        let id = self.owner.compile_function(
            None,
            &params,
            body,
            &scopes,
            Some(self.function_id),
            FunctionOptions {
                defaults: Some(&function.params),
                source_text: self
                    .owner
                    .text
                    .get(source_span.start as usize..source_span.end as usize)
                    .map(str::to_owned),
                async_function: function.r#async,
                generator: function.generator,
                with_depth: self.with_depth,
                non_constructible: true,
                super_home: true,
                super_home_atom: Some(super_atom),
                strict: self.strict
                    || function.body.as_ref().is_some_and(|body| {
                        body.directives
                            .iter()
                            .any(|directive| directive.directive == "use strict")
                    }),
                ..FunctionOptions::default()
            },
        );
        let dst = self.reg();
        self.emit(Op::MakeClosure, dst, 0, 0, id);
        dst
    }

    fn object_property_value(
        &mut self,
        value: &Expression<'_>,
        is_method: bool,
        super_atom: Atom,
        source_span: Span,
    ) -> Register {
        if is_method {
            self.object_method(value, super_atom, source_span)
        } else {
            self.expression(value)
        }
    }

    fn define_accessor(&mut self, target: Register, key: Register, function: Register, kind: &str) {
        let descriptor = self.reg();
        self.emit(Op::MakeObject, descriptor, 0, 0, 0);
        let field = self.owner.atom(kind);
        let cache = self.owner.cache_site();
        self.emit(Op::SetField, function, descriptor, cache, field);
        for (field, value) in [
            ("enumerable", Constant::Boolean(true)),
            ("configurable", Constant::Boolean(true)),
        ] {
            let atom = self.owner.atom(field);
            let value = self.literal(value);
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

    fn object_spread(&mut self, target: Register, source: &Expression<'_>) {
        let source = self.expression(source);
        let null = self.literal(Constant::Null);
        let is_null = self.emit_binary(2, Operand::register(source), Operand::register(null));
        let continue_non_null = self.emit(Op::JumpFalse, is_null, 0, 0, 0);
        let skip_null = self.emit(Op::Jump, 0, 0, 0, 0);
        self.patch(continue_non_null);
        let undefined = self.literal(Constant::Undefined);
        let is_undefined =
            self.emit_binary(2, Operand::register(source), Operand::register(undefined));
        let continue_non_undefined = self.emit(Op::JumpFalse, is_undefined, 0, 0, 0);
        let skip_undefined = self.emit(Op::Jump, 0, 0, 0, 0);
        self.patch(continue_non_undefined);
        let object = self.load_name("Object");
        let assign = self.reg();
        let assign_atom = self.owner.atom("assign");
        let assign_cache = self.owner.cache_site();
        self.emit(
            Op::GetField,
            assign,
            FieldBase::register(object).0,
            assign_cache,
            assign_atom,
        );
        let base = self.next_reg;
        let target_arg = self.reg();
        self.emit(Op::Move, target_arg, target, 0, 0);
        let source_arg = self.reg();
        self.emit(Op::Move, source_arg, source, 0, 0);
        let result = self.reg();
        self.emit(
            Op::Call,
            result,
            assign,
            object,
            crate::bytecode::ImmediateLayout::call_immediate(base, 2, false, false),
        );
        self.patch_instruction(skip_null, self.code.len() as u32);
        self.patch_instruction(skip_undefined, self.code.len() as u32);
    }

    pub(super) fn computed_object_key(&mut self, key: &PropertyKey<'_>) -> Option<Register> {
        key.as_expression()
            .map(|expression| self.expression(expression))
    }
}
