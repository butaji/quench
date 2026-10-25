use super::*;
use rustc_hash::FxHashSet;
use smallvec::SmallVec;

impl FunctionCompiler<'_, '_> {
    pub(super) fn object_expression(&mut self, value: &ObjectExpression<'_>) -> Register {
        let dst = self.reg();
        if let [
            ObjectPropertyKind::ObjectProperty(first),
            ObjectPropertyKind::ObjectProperty(second),
        ] = value.properties.as_slice()
            && first.kind == PropertyKind::Init
            && second.kind == PropertyKind::Init
            && !first.computed
            && !second.computed
            && !Self::is_object_literal_prototype(first)
            && !Self::is_object_literal_prototype(second)
            && !Self::anonymous_function_definition(&first.value)
            && !Self::anonymous_function_definition(&second.value)
            && let (Some(first_key), Some(second_key)) =
                (Self::static_key(&first.key), Self::static_key(&second.key))
            // A shape gains only keys it lacks; `{ a, a }` defines one property.
            && first_key != second_key
        {
            let first_value = self.expression(&first.value);
            let second_value = self.expression(&second.value);
            let site = self.owner.object_sites.len() as u32;
            let atoms =
                smallvec::smallvec![self.owner.atom(first_key), self.owner.atom(second_key)];
            self.owner.object_sites.push(ObjectSite { atoms });
            self.emit(Op::MakeObject2, dst, first_value, second_value, site);
            return dst;
        }
        if let Some(properties) = Self::static_object_data_properties(value)
            && properties.len() > crate::bytecode::INLINE_OBJECT_SITE_ATOMS
            && u16::try_from(properties.len()).is_ok()
            && {
                let mut keys = FxHashSet::default();
                properties.iter().all(|(_, key)| keys.insert(*key))
            }
        {
            let atoms = properties
                .iter()
                .map(|(_, key)| self.owner.atom(key))
                .collect::<SmallVec<[_; crate::bytecode::INLINE_OBJECT_SITE_ATOMS]>>();
            let site = self.owner.object_sites.len() as u32;
            self.owner.object_sites.push(ObjectSite { atoms });
            let count = properties.len() as u16;
            let value_registers = (0..count).map(|_| self.reg()).collect::<Vec<_>>();
            // Each value's temporaries are dead once it moves into its register.
            let temporaries = self.next_reg;
            for ((property, _), destination) in properties.into_iter().zip(value_registers.iter()) {
                self.next_reg = temporaries;
                let value = self.expression(&property.value);
                self.emit(Op::Move, *destination, value, 0, 0);
            }
            self.next_reg = temporaries;
            self.emit(Op::MakeObjectLiteral, dst, value_registers[0], count, site);
            return dst;
        }
        self.emit(Op::MakeObject, dst, 0, 0, 0);
        let super_atom = value
            .properties
            .iter()
            .any(|property| {
                matches!(property, ObjectPropertyKind::ObjectProperty(property)
                if property.method || property.kind != PropertyKind::Init)
            })
            .then(|| self.hidden_local(&format!("\0quench:object-home:{}", value.span.start)));
        if let Some(super_atom) = super_atom {
            // Each evaluation's methods find their own home only through `super`: a literal
            // whose methods cannot reference it shares the binding, rather than cloning the
            // environment, which every later closure of the scope would then capture.
            if Self::object_methods_may_reference_super(value) {
                self.clone_environment_slots(vec![self.local_slots[&super_atom]]);
            }
            self.store_atom(super_atom, dst);
        }
        // Each property's temporaries are dead once it is defined on `dst`.
        let temporaries = self.next_reg;
        for property in &value.properties {
            self.next_reg = temporaries;
            let property = match property {
                ObjectPropertyKind::ObjectProperty(property) => property,
                ObjectPropertyKind::SpreadProperty(spread) => {
                    self.object_spread(dst, &spread.argument);
                    continue;
                }
            };
            if Self::is_object_literal_prototype(property) {
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
                let item = self.object_method(
                    &property.value,
                    super_atom.expect("object accessor has a home binding"),
                    property.span,
                );
                if computed {
                    self.emit(
                        Op::SetFunctionNameKey,
                        item,
                        key,
                        0,
                        if accessor == "get" {
                            crate::bytecode::FUNCTION_NAME_PREFIX_GETTER
                        } else {
                            crate::bytecode::FUNCTION_NAME_PREFIX_SETTER
                        },
                    );
                } else if let Some(name) = Self::static_key(&property.key) {
                    let name = self.owner.atom(&format!("{accessor} {name}"));
                    self.emit_function_name(item, name);
                } else {
                    self.emit(
                        Op::SetFunctionNameKey,
                        item,
                        key,
                        0,
                        if accessor == "get" {
                            crate::bytecode::FUNCTION_NAME_PREFIX_GETTER
                        } else {
                            crate::bytecode::FUNCTION_NAME_PREFIX_SETTER
                        },
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
                self.emit_function_name(item, atom);
            }
            let cache = self.owner.cache_site();
            self.emit(Op::DefineField, item, dst, cache, atom);
        }
        self.next_reg = temporaries;
        dst
    }

    pub(super) fn static_object_data_properties<'property, 'ast>(
        value: &'property ObjectExpression<'ast>,
    ) -> Option<Vec<(&'property ObjectProperty<'ast>, &'property str)>> {
        value
            .properties
            .iter()
            .map(|property| match property {
                ObjectPropertyKind::ObjectProperty(property)
                    if property.kind == PropertyKind::Init
                        && !property.method
                        && !property.computed
                        && !Self::is_object_literal_prototype(property)
                        && !Self::anonymous_function_definition(&property.value) =>
                {
                    Some((&**property, Self::static_key(&property.key)?))
                }
                _ => None,
            })
            .collect()
    }

    fn is_object_literal_prototype(property: &ObjectProperty<'_>) -> bool {
        property.kind == PropertyKind::Init
            && !property.method
            && !property.computed
            && !property.shorthand
            && Self::static_key(&property.key) == Some("__proto__")
    }

    fn object_literal_prototype(&mut self, object: Register, prototype: Register) {
        let callee = self.load_name("\0quench:object-literal-prototype");
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

    /// Whether a method or accessor of `value` may read its home object: some `super`
    /// within it, nested functions and classes included, or a direct `eval`, whose code
    /// may name `super`.
    fn object_methods_may_reference_super(value: &ObjectExpression<'_>) -> bool {
        use oxc_ast_visit::{Visit, walk};
        struct SuperFinder(bool);
        impl<'a> Visit<'a> for SuperFinder {
            fn visit_super(&mut self, _: &oxc_ast::ast::Super) {
                self.0 = true;
            }
            fn visit_call_expression(&mut self, call: &oxc_ast::ast::CallExpression<'a>) {
                if super::super::early::is_direct_eval_call(call) {
                    self.0 = true;
                } else {
                    walk::walk_call_expression(self, call);
                }
            }
        }
        let mut finder = SuperFinder(false);
        for property in &value.properties {
            if let ObjectPropertyKind::ObjectProperty(property) = property
                && (property.method || property.kind != PropertyKind::Init)
            {
                finder.visit_expression(&property.value);
            }
        }
        finder.0
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
        let context = super::super::replay::ObjectMethodContext {
            property_span: source_span,
            home_atom: super_atom,
            scopes: self.capture_scopes(),
            parent: self.function_id,
            with_depth: self.with_depth,
            inherited_strict: self.strict,
        };
        let id = self
            .owner
            .compile_replayable_object_method(function, context);
        let dst = self.reg();
        self.emit(Op::MakeClosure, dst, 0, 0, id);
        dst
    }

    fn object_property_value(
        &mut self,
        value: &Expression<'_>,
        is_method: bool,
        super_atom: Option<Atom>,
        source_span: Span,
    ) -> Register {
        if is_method {
            self.object_method(
                value,
                super_atom.expect("object method has a home binding"),
                source_span,
            )
        } else {
            self.expression(value)
        }
    }

    fn define_accessor(&mut self, target: Register, key: Register, function: Register, kind: &str) {
        let mode = match kind {
            "get" => crate::bytecode::PropertyDefinitionMode::EnumerableGetter,
            "set" => crate::bytecode::PropertyDefinitionMode::EnumerableSetter,
            _ => unreachable!("object accessor kind"),
        };
        self.emit(Op::DefinePropertyRecord, function, target, key, mode.word());
    }

    fn object_spread(&mut self, target: Register, source: &Expression<'_>) {
        let source = self.expression(source);
        let exclusions = self.literal(Constant::Undefined);
        self.emit(Op::CopyDataProperties, target, source, exclusions, 0);
    }

    pub(super) fn computed_object_key(&mut self, key: &PropertyKey<'_>) -> Option<Register> {
        key.as_expression()
            .map(|expression| self.expression(expression))
    }
}
