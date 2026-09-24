use super::*;

#[derive(Clone, Copy)]
struct BindingIterator {
    iterator: Register,
    next: Register,
    done: Register,
}

#[derive(Clone, Copy)]
enum AssignmentReference {
    Name {
        atom: Atom,
        environment: Option<Register>,
    },
    ThisField(Atom),
    Field {
        object: Register,
        atom: Atom,
        mirror_global: bool,
    },
    SuperField {
        base: Register,
        key: Register,
        receiver: Register,
    },
    PrivateField {
        object: Register,
        atom: Atom,
    },
    Index {
        object: Register,
        key: Register,
    },
    SuperIndex {
        base: Register,
        key: Register,
        receiver: Register,
    },
}

impl FunctionCompiler<'_, '_> {
    pub(super) fn assignment(&mut self, value: &AssignmentExpression<'_>) -> Register {
        if let Some(target) = value.left.as_simple_assignment_target() {
            let reference = self.prepare_assignment_reference(target);
            let operator = value.operator as u8;
            if operator == 0 {
                let right = self.expression(&value.right);
                if Self::anonymous_function_definition(&value.right)
                    && let Some(name) = self.assignment_identifier_name(&value.left)
                {
                    self.emit(Op::SetFunctionName, right, 0, 0, name);
                }
                self.store_assignment_reference(reference, right);
                return right;
            }

            let reference = self.canonicalize_assignment_reference(reference, true);
            let old = self.load_assignment_reference(reference);
            if (13..=15).contains(&operator) {
                let skip = match operator {
                    13 => {
                        let evaluate_right = self.emit(Op::JumpFalse, old, 0, 0, 0);
                        let skip = self.emit(Op::Jump, 0, 0, 0, 0);
                        self.patch(evaluate_right);
                        skip
                    }
                    14 => self.emit(Op::JumpFalse, old, 0, 0, 0),
                    15 => {
                        let null = self.literal(Constant::Null);
                        let is_null =
                            self.emit_binary(0, Operand::register(old), Operand::register(null));
                        let not_null = self.emit(Op::JumpFalse, is_null, 0, 0, 0);
                        let evaluate_null = self.emit(Op::Jump, 0, 0, 0, 0);
                        self.patch(not_null);
                        let undefined = self.literal(Constant::Undefined);
                        let is_undefined = self.emit_binary(
                            0,
                            Operand::register(old),
                            Operand::register(undefined),
                        );
                        let skip = self.emit(Op::JumpFalse, is_undefined, 0, 0, 0);
                        self.patch(evaluate_null);
                        skip
                    }
                    _ => unreachable!(),
                };
                let right = self.expression(&value.right);
                if Self::anonymous_function_definition(&value.right)
                    && let Some(name) = self.assignment_identifier_name(&value.left)
                {
                    self.emit(Op::SetFunctionName, right, 0, 0, name);
                }
                self.emit(Op::Move, old, right, 0, 0);
                self.store_assignment_reference(reference, old);
                self.patch(skip);
                return old;
            }

            let right = self.expression(&value.right);
            let result = self.emit_binary(
                u32::from(operator + 7),
                Operand::register(old),
                Operand::register(right),
            );
            self.store_assignment_reference(reference, result);
            result
        } else if value.operator == AssignmentOperator::Assign {
            let right = self.expression(&value.right);
            self.assign_pattern(&value.left, right);
            right
        } else {
            let right = self.expression(&value.right);
            self.owner
                .reject(value.span, "compound assignment pattern unsupported");
            right
        }
    }

    fn load_assignment_reference(&mut self, reference: AssignmentReference) -> Register {
        match reference {
            AssignmentReference::Name { atom, environment } => {
                if let Some(environment) = environment {
                    let value = self.reg();
                    let cache = self.owner.cache_site();
                    self.emit(
                        Op::GetField,
                        value,
                        FieldBase::register(environment).0,
                        cache,
                        atom,
                    );
                    value
                } else {
                    self.load_atom(atom)
                }
            }
            AssignmentReference::ThisField(atom) => {
                let object = self.reg();
                self.emit(Op::LoadThis, object, 0, 0, 0);
                let value = self.reg();
                let cache = self.owner.cache_site();
                self.emit(
                    Op::GetField,
                    value,
                    FieldBase::register(object).0,
                    cache,
                    atom,
                );
                value
            }
            AssignmentReference::Field { object, atom, .. } => {
                let value = self.reg();
                let cache = self.owner.cache_site();
                self.emit(
                    Op::GetField,
                    value,
                    FieldBase::register(object).0,
                    cache,
                    atom,
                );
                value
            }
            AssignmentReference::SuperField {
                base,
                key,
                receiver,
            } => self.super_get(base, key, receiver),
            AssignmentReference::PrivateField { object, atom } => {
                let value = self.reg();
                let cache = self.owner.cache_site();
                self.emit(
                    Op::GetField,
                    value,
                    FieldBase::register(object).0,
                    cache,
                    atom,
                );
                value
            }
            AssignmentReference::Index { object, key } => {
                let value = self.reg();
                self.emit(Op::GetIndex, value, object, key, 0);
                value
            }
            AssignmentReference::SuperIndex {
                base,
                key,
                receiver,
            } => self.super_get(base, key, receiver),
        }
    }

    fn assignment_identifier_name(&mut self, target: &AssignmentTarget<'_>) -> Option<Atom> {
        let SimpleAssignmentTarget::AssignmentTargetIdentifier(identifier) =
            target.as_simple_assignment_target()?
        else {
            return None;
        };
        let span = target.span();
        let source = self
            .owner
            .text
            .get(span.start as usize..span.end as usize)?
            .trim();
        let before = self.owner.text[..span.start as usize]
            .trim_end()
            .chars()
            .next_back();
        let after = self.owner.text[span.end as usize..]
            .trim_start()
            .chars()
            .next();
        if before == Some('(') && after == Some(')') {
            return None;
        }
        (source == identifier.name.as_str()).then(|| self.owner.atom(source))
    }

    fn canonicalize_assignment_reference(
        &mut self,
        reference: AssignmentReference,
        require_object: bool,
    ) -> AssignmentReference {
        match reference {
            AssignmentReference::Index { object, key } => {
                if require_object {
                    self.emit(Op::RequireObjectCoercible, 0, object, 0, 0);
                }
                let converted = self.reg();
                self.emit(Op::ToPropertyKey, converted, key, 0, 0);
                AssignmentReference::Index {
                    object,
                    key: converted,
                }
            }
            AssignmentReference::SuperIndex {
                base,
                key,
                receiver,
            } => {
                if require_object {
                    self.emit(Op::RequireObjectCoercible, 0, base, 0, 0);
                }
                let converted = self.reg();
                self.emit(Op::ToPropertyKey, converted, key, 0, 0);
                AssignmentReference::SuperIndex {
                    base,
                    key: converted,
                    receiver,
                }
            }
            other => other,
        }
    }

    pub(super) fn assign_pattern(&mut self, target: &AssignmentTarget<'_>, value: Register) {
        if let Some(simple) = target.as_simple_assignment_target() {
            self.assign_target(simple, value, 0);
            return;
        }
        match target {
            AssignmentTarget::ArrayAssignmentTarget(array) => {
                let iterator = self.binding_iterator(value);
                let (protected_start, error_atom) =
                    self.begin_binding_iterator_protection(iterator);
                for element in &array.elements {
                    if let Some(element) = element {
                        self.assign_iterator_element(iterator, element);
                    } else {
                        self.binding_iterator_step(iterator);
                    }
                }
                if let Some(rest) = &array.rest {
                    if let Some(target) = rest.target.as_simple_assignment_target() {
                        let reference = self.prepare_assignment_reference(target);
                        let reference = self.canonicalize_assignment_reference(reference, false);
                        let rest_value = self.binding_iterator_rest(iterator);
                        self.store_assignment_reference(reference, rest_value);
                    } else {
                        let rest_value = self.binding_iterator_rest(iterator);
                        self.assign_pattern(&rest.target, rest_value);
                    }
                }
                self.end_binding_iterator_protection(iterator, protected_start, error_atom);
            }
            AssignmentTarget::ObjectAssignmentTarget(object) => {
                self.require_object_coercible(value);
                let mut excluded = Vec::with_capacity(object.properties.len());
                for property in &object.properties {
                    let selected = self.reg();
                    match property {
                        AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(property) => {
                            let atom = self.owner.atom(property.binding.name.as_str());
                            self.resolve_assignment_name(atom);
                            excluded.push(
                                self.literal(Constant::String(property.binding.name.to_string())),
                            );
                            let cache = self.owner.cache_site();
                            self.emit(
                                Op::GetField,
                                selected,
                                FieldBase::register(value).0,
                                cache,
                                atom,
                            );
                            if let Some(init) = &property.init {
                                self.assign_default_value_named(selected, init, Some(atom));
                            }
                            self.store_atom(atom, selected);
                        }
                        AssignmentTargetProperty::AssignmentTargetPropertyProperty(property) => {
                            if let Some(expression) = property.name.as_expression() {
                                let raw_key = self.expression(expression);
                                let key = self.reg();
                                self.emit(Op::ToPropertyKey, key, raw_key, 0, 0);
                                let reference =
                                    self.prepare_maybe_default_reference(&property.binding);
                                excluded.push(key);
                                self.emit(Op::GetIndex, selected, value, key, 0);
                                self.assign_maybe_default_prepared(
                                    &property.binding,
                                    selected,
                                    reference,
                                );
                            } else if let Some(name) = Self::binding_key(&property.name) {
                                let atom = self.owner.atom(name);
                                let reference =
                                    self.prepare_maybe_default_reference(&property.binding);
                                excluded.push(self.literal(Constant::String(name.to_owned())));
                                let cache = self.owner.cache_site();
                                self.emit(
                                    Op::GetField,
                                    selected,
                                    FieldBase::register(value).0,
                                    cache,
                                    atom,
                                );
                                self.assign_maybe_default_prepared(
                                    &property.binding,
                                    selected,
                                    reference,
                                );
                            } else {
                                self.owner
                                    .reject(property.span(), "destructuring key is unsupported");
                                continue;
                            }
                        }
                    }
                }
                if let Some(rest) = &object.rest {
                    let rest_value = self.assignment_object_rest(value, &excluded);
                    self.assign_pattern(&rest.target, rest_value);
                }
            }
            _ => self
                .owner
                .reject(target.span(), "assignment pattern unsupported"),
        }
    }

    fn assign_iterator_element(
        &mut self,
        iterator: BindingIterator,
        element: &AssignmentTargetMaybeDefault<'_>,
    ) {
        if let AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(default) = element
            && let Some(target) = default.binding.as_simple_assignment_target()
        {
            let reference = self.prepare_assignment_reference(target);
            let value = self.binding_iterator_step(iterator);
            let name = match target {
                SimpleAssignmentTarget::AssignmentTargetIdentifier(identifier) => {
                    Some(self.owner.atom(identifier.name.as_str()))
                }
                _ => None,
            };
            self.assign_default_value_named(value, &default.init, name);
            self.store_assignment_reference(reference, value);
            return;
        }
        if let Some(target) = element.as_assignment_target()
            && let Some(simple) = target.as_simple_assignment_target()
        {
            let reference = self.prepare_assignment_reference(simple);
            let value = self.binding_iterator_step(iterator);
            self.store_assignment_reference(reference, value);
            return;
        }
        let value = self.binding_iterator_step(iterator);
        self.assign_maybe_default(element, value);
    }

    fn prepare_assignment_reference(
        &mut self,
        target: &SimpleAssignmentTarget<'_>,
    ) -> AssignmentReference {
        match target {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(identifier) => {
                let atom = self.owner.atom(identifier.name.as_str());
                let capture_global_reference = self.needs_strict_global_reference_capture(atom);
                let resolve_reference = self.with_depth != 0 || capture_global_reference;
                let environment = if resolve_reference {
                    let resolved = self.reg();
                    let cache = self.owner.cache_site();
                    self.emit(
                        Op::ResolveName,
                        resolved,
                        u16::from(capture_global_reference),
                        cache,
                        atom,
                    );
                    Some(resolved)
                } else {
                    None
                };
                AssignmentReference::Name { atom, environment }
            }
            SimpleAssignmentTarget::StaticMemberExpression(member) => {
                let atom = self.owner.atom(member.property.name.as_str());
                if matches!(&member.object, Expression::Super(_)) {
                    let base = self.expression(&member.object);
                    let receiver = self.load_this_value();
                    let key = self.literal(Constant::String(member.property.name.to_string()));
                    AssignmentReference::SuperField {
                        base,
                        key,
                        receiver,
                    }
                } else if matches!(&member.object, Expression::ThisExpression(_)) {
                    AssignmentReference::ThisField(atom)
                } else {
                    let object = self.expression(&member.object);
                    AssignmentReference::Field {
                        object,
                        atom,
                        mirror_global: matches!(&member.object, Expression::Identifier(id) if id.name == "globalThis"),
                    }
                }
            }
            SimpleAssignmentTarget::PrivateFieldExpression(member) => {
                let atom = self.owner.private_name_atom(member.field.span);
                let object = self.expression(&member.object);
                AssignmentReference::PrivateField { object, atom }
            }
            SimpleAssignmentTarget::ComputedMemberExpression(member) => {
                if matches!(&member.object, Expression::Super(_)) {
                    let receiver = self.load_this_value();
                    let key = self.expression(&member.expression);
                    // Evaluate the key expression before GetSuperBase, but
                    // retain the resulting base before ToPropertyKey (which
                    // occurs when the reference is consumed).
                    let base = self.expression(&member.object);
                    AssignmentReference::SuperIndex {
                        base,
                        key,
                        receiver,
                    }
                } else {
                    let object = self.expression(&member.object);
                    let key = self.expression(&member.expression);
                    AssignmentReference::Index { object, key }
                }
            }
            _ => {
                self.owner
                    .reject(target.span(), "assignment target unsupported");
                AssignmentReference::Name {
                    atom: self.owner.atom("undefined"),
                    environment: None,
                }
            }
        }
    }

    fn store_assignment_reference(&mut self, reference: AssignmentReference, value: Register) {
        match reference {
            AssignmentReference::Name {
                atom,
                environment: Some(environment),
            } => {
                self.emit(
                    Op::StoreResolvedName,
                    value,
                    environment,
                    u16::from(self.strict),
                    atom,
                );
            }
            AssignmentReference::Name {
                atom,
                environment: None,
            } => self.store_atom(atom, value),
            AssignmentReference::ThisField(atom) => {
                let site = self.owner.cache_site();
                self.emit(Op::SetThisField, value, 0, site, atom);
            }
            AssignmentReference::Field {
                object,
                atom,
                mirror_global,
            } => {
                let site = self.owner.cache_site();
                self.emit(Op::SetField, value, object, site, atom);
                if mirror_global {
                    self.store_atom(atom, value);
                }
            }
            AssignmentReference::SuperField {
                base,
                key,
                receiver,
            }
            | AssignmentReference::SuperIndex {
                base,
                key,
                receiver,
            } => self.super_set(base, key, value, receiver),
            AssignmentReference::PrivateField { object, atom } => {
                self.emit(Op::CheckPrivate, object, 0, 0, atom);
                let site = self.owner.cache_site();
                self.emit(Op::SetField, value, object, site, atom);
            }
            AssignmentReference::Index { object, key } => {
                self.emit(Op::SetIndex, value, object, key, 0);
            }
        }
    }

    fn assign_maybe_default(&mut self, target: &AssignmentTargetMaybeDefault<'_>, value: Register) {
        match target {
            AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(default) => {
                let name =
                    if let Some(SimpleAssignmentTarget::AssignmentTargetIdentifier(identifier)) =
                        default.binding.as_simple_assignment_target()
                    {
                        Some(self.owner.atom(identifier.name.as_str()))
                    } else {
                        None
                    };
                self.assign_default_value_named(value, &default.init, name);
                self.assign_pattern(&default.binding, value);
            }
            _ => {
                let Some(target) = target.as_assignment_target() else {
                    self.owner
                        .reject(target.span(), "assignment target unsupported");
                    return;
                };
                self.assign_pattern(target, value);
            }
        }
    }

    fn prepare_maybe_default_reference(
        &mut self,
        target: &AssignmentTargetMaybeDefault<'_>,
    ) -> Option<(AssignmentReference, Option<Atom>)> {
        let simple = match target {
            AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(default) => {
                default.binding.as_simple_assignment_target()
            }
            _ => target
                .as_assignment_target()
                .and_then(AssignmentTarget::as_simple_assignment_target),
        }?;
        let name = match simple {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(identifier) => {
                Some(self.owner.atom(identifier.name.as_str()))
            }
            _ => None,
        };
        Some((self.prepare_assignment_reference(simple), name))
    }

    fn assign_maybe_default_prepared(
        &mut self,
        target: &AssignmentTargetMaybeDefault<'_>,
        value: Register,
        reference: Option<(AssignmentReference, Option<Atom>)>,
    ) {
        let Some((reference, name)) = reference else {
            self.assign_maybe_default(target, value);
            return;
        };
        if let AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(default) = target {
            self.assign_default_value_named(value, &default.init, name);
        }
        self.store_assignment_reference(reference, value);
    }

    fn assign_default_value_named(
        &mut self,
        value: Register,
        init: &Expression<'_>,
        name: Option<Atom>,
    ) {
        let undefined = self.literal(Constant::Undefined);
        let missing = self.emit_binary(2, Operand::register(value), Operand::register(undefined));
        let skip = self.emit(Op::JumpFalse, missing, 0, 0, 0);
        let fallback = self.expression(init);
        if let Some(name) = name
            && Self::anonymous_function_definition(init)
        {
            self.emit(Op::SetFunctionName, fallback, 0, 0, name);
        }
        self.emit(Op::Move, value, fallback, 0, 0);
        self.patch(skip);
    }

    fn assignment_object_rest(&mut self, source: Register, excluded: &[Register]) -> Register {
        self.emit_copy_data_properties(source, excluded)
    }

    pub(super) fn bind_pattern(&mut self, pattern: &BindingPattern<'_>, value: Register) {
        self.bind_pattern_with_reference(pattern, value, None);
    }

    fn bind_pattern_with_reference(
        &mut self,
        pattern: &BindingPattern<'_>,
        value: Register,
        reference: Option<Register>,
    ) {
        match pattern {
            BindingPattern::BindingIdentifier(id) => {
                let atom = self.owner.atom(id.name.as_str());
                if let Some(reference) = reference {
                    self.emit(
                        Op::StoreResolvedName,
                        value,
                        reference,
                        u16::from(self.strict),
                        atom,
                    );
                } else {
                    self.initialize_atom(atom, value);
                }
            }
            BindingPattern::ObjectPattern(object) => {
                self.require_object_coercible(value);
                let mut excluded = Vec::with_capacity(object.properties.len());
                for property in &object.properties {
                    let dst = self.reg();
                    if let Some(expression) = property.key.as_expression() {
                        let key = self.expression(expression);
                        let property_key = self.reg();
                        self.emit(Op::ToPropertyKey, property_key, key, 0, 0);
                        excluded.push(property_key);
                        let reference = self.resolve_binding_pattern(&property.value);
                        self.emit(Op::GetIndex, dst, value, property_key, 0);
                        self.bind_pattern_with_reference(&property.value, dst, reference);
                        continue;
                    } else if let Some(key) = Self::binding_key(&property.key) {
                        excluded.push(self.literal(Constant::String(key.to_owned())));
                        let atom = self.owner.atom(key);
                        let reference = self.resolve_binding_pattern(&property.value);
                        let cache = self.owner.cache_site();
                        self.emit(Op::GetField, dst, FieldBase::register(value).0, cache, atom);
                        self.bind_pattern_with_reference(&property.value, dst, reference);
                        continue;
                    } else {
                        self.owner
                            .reject(property.span, "destructuring key is unsupported");
                        continue;
                    }
                }
                if let Some(rest) = &object.rest {
                    let rest_value = self.emit_copy_data_properties(value, &excluded);
                    self.bind_pattern(&rest.argument, rest_value);
                }
            }
            BindingPattern::ArrayPattern(array) => {
                let iterator = self.binding_iterator(value);
                let (protected_start, error_atom) =
                    self.begin_binding_iterator_protection(iterator);
                for element in &array.elements {
                    let dst = self.binding_iterator_step(iterator);
                    if let Some(element) = element {
                        self.bind_pattern(element, dst);
                    }
                }
                if let Some(rest) = &array.rest {
                    let rest_value = self.binding_iterator_rest(iterator);
                    self.bind_pattern(&rest.argument, rest_value);
                }
                self.end_binding_iterator_protection(iterator, protected_start, error_atom);
            }
            BindingPattern::AssignmentPattern(assignment) => {
                let selected = self.reg();
                self.emit(Op::Move, selected, value, 0, 0);
                let undefined = self.literal(Constant::Undefined);
                let missing =
                    self.emit_binary(2, Operand::register(value), Operand::register(undefined));
                let skip = self.emit(Op::JumpFalse, missing, 0, 0, 0);
                let fallback = self.expression(&assignment.right);
                if let BindingPattern::BindingIdentifier(identifier) = &assignment.left
                    && Self::anonymous_function_definition(&assignment.right)
                {
                    let name = self.owner.atom(identifier.name.as_str());
                    self.emit(Op::SetFunctionName, fallback, 0, 0, name);
                }
                self.emit(Op::Move, selected, fallback, 0, 0);
                self.patch(skip);
                self.bind_pattern_with_reference(&assignment.left, selected, reference);
            }
        }
    }

    fn binding_key<'a>(key: &'a PropertyKey<'a>) -> Option<&'a str> {
        match key {
            PropertyKey::StaticIdentifier(id) => Some(id.name.as_str()),
            PropertyKey::StringLiteral(value) => Some(value.value.as_str()),
            _ => None,
        }
    }

    fn require_object_coercible(&mut self, value: Register) {
        let null = self.literal(Constant::Null);
        let is_null = self.emit_binary(2, Operand::register(value), Operand::register(null));
        let skip_null = self.emit(Op::JumpFalse, is_null, 0, 0, 0);
        self.throw_object_coercion();
        let end = self.emit(Op::Jump, 0, 0, 0, 0);
        self.patch(skip_null);
        let undefined = self.literal(Constant::Undefined);
        let is_undefined =
            self.emit_binary(2, Operand::register(value), Operand::register(undefined));
        let skip_undefined = self.emit(Op::JumpFalse, is_undefined, 0, 0, 0);
        self.throw_object_coercion();
        self.patch(end);
        self.patch(skip_undefined);
    }

    fn resolve_binding_pattern(&mut self, pattern: &BindingPattern<'_>) -> Option<Register> {
        let target = match pattern {
            BindingPattern::BindingIdentifier(id) => Some(id.name.as_str()),
            BindingPattern::AssignmentPattern(assignment) => match &assignment.left {
                BindingPattern::BindingIdentifier(id) => Some(id.name.as_str()),
                _ => None,
            },
            _ => None,
        };
        let name = target?;
        if self.with_depth == 0 {
            return None;
        }
        let atom = self.owner.atom(name);
        let dst = self.reg();
        let cache = self.owner.cache_site();
        self.emit(Op::ResolveName, dst, 0, cache, atom);
        Some(dst)
    }

    fn resolve_assignment_name(&mut self, atom: Atom) {
        if self.with_depth != 0 {
            let dst = self.reg();
            let cache = self.owner.cache_site();
            self.emit(Op::LoadNameTypeof, dst, 0, cache, atom);
        } else if self.needs_strict_global_reference_capture(atom) {
            let dst = self.reg();
            let cache = self.owner.cache_site();
            self.emit(Op::ResolveName, dst, 1, cache, atom);
        }
    }

    fn throw_object_coercion(&mut self) {
        let constructor = self.load_name("TypeError");
        let message = self.literal(Constant::String(
            "cannot destructure null or undefined".into(),
        ));
        let argument = self.reg();
        self.emit(Op::Move, argument, message, 0, 0);
        let error = self.reg();
        self.emit(Op::Construct, error, constructor, argument, 1);
        self.emit(Op::Throw, error, 0, 0, 0);
    }

    fn binding_iterator(&mut self, source: Register) -> BindingIterator {
        let iterator = self.reg();
        self.emit(Op::GetIterator, iterator, source, 0, 0);
        let next = self.reg();
        let next_atom = self.owner.atom("next");
        let cache = self.owner.cache_site();
        self.emit(
            Op::GetField,
            next,
            FieldBase::register(iterator).0,
            cache,
            next_atom,
        );
        let done = self.literal(Constant::Boolean(false));
        BindingIterator {
            iterator,
            next,
            done,
        }
    }

    fn binding_iterator_step(&mut self, iterator: BindingIterator) -> Register {
        let value = self.reg();
        let undefined = self.literal(Constant::Undefined);
        self.emit(Op::Move, value, undefined, 0, 0);
        let call_next = self.emit(Op::JumpFalse, iterator.done, 0, 0, 0);
        let already_done = self.emit(Op::Jump, 0, 0, 0, 0);
        self.patch(call_next);

        let result = self.reg();
        let next_start = self.code.len() as u32;
        self.emit(Op::Call, result, iterator.next, iterator.iterator, 0);
        let next_end = self.code.len() as u32;
        let after_next_error = self.emit(Op::Jump, 0, 0, 0, 0);
        let next_error_target = self.code.len() as u32;
        let error_atom = self.hidden_local("\0rqj:iterator-next-error");
        self.handlers.push(crate::bytecode::Handler {
            start: next_start,
            end: next_end,
            target: next_error_target,
            slot: self.local_slot(error_atom),
            return_target: None,
            return_slot: None,
        });
        let original_error = self.load_atom(error_atom);
        let done = self.literal(Constant::Boolean(true));
        self.emit(Op::Move, iterator.done, done, 0, 0);
        self.emit(Op::Throw, original_error, 0, 0, 0);
        self.patch(after_next_error);
        let done_value = self.reg();
        let done_atom = self.owner.atom("done");
        let done_cache = self.owner.cache_site();
        self.emit(
            Op::GetField,
            done_value,
            FieldBase::register(result).0,
            done_cache,
            done_atom,
        );
        self.emit(Op::Move, iterator.done, done_value, 0, 0);
        let read_value = self.emit(Op::JumpFalse, done_value, 0, 0, 0);
        let end_step = self.emit(Op::Jump, 0, 0, 0, 0);
        self.patch(read_value);
        let value_atom = self.owner.atom("value");
        let value_cache = self.owner.cache_site();
        self.emit(
            Op::GetField,
            value,
            FieldBase::register(result).0,
            value_cache,
            value_atom,
        );
        self.patch(already_done);
        self.patch(end_step);
        value
    }

    fn binding_iterator_rest(&mut self, iterator: BindingIterator) -> Register {
        let rest = self.reg();
        self.emit(Op::MakeArray, rest, 0, 0, 0);
        let index = self.literal(Constant::Number(0.0));
        let head = self.code.len() as u32;
        let value = self.binding_iterator_step(iterator);
        let append = self.emit(Op::JumpFalse, iterator.done, 0, 0, 0);
        let end = self.emit(Op::Jump, 0, 0, 0, 0);
        self.patch(append);
        self.emit(Op::SetIndex, value, rest, index, 0);
        let next_index = self.reg();
        self.emit(Op::IncDec, next_index, index, 0, 0);
        self.emit(Op::Move, index, next_index, 0, 0);
        self.emit(Op::Jump, 0, 0, 0, head);
        self.patch(end);
        rest
    }

    fn binding_iterator_close(&mut self, iterator: BindingIterator) {
        let close = self.emit(Op::JumpFalse, iterator.done, 0, 0, 0);
        let end = self.emit(Op::Jump, 0, 0, 0, 0);
        self.patch(close);
        self.emit(Op::IteratorClose, 0, iterator.iterator, 0, 0);
        self.patch(end);
    }

    fn begin_binding_iterator_protection(&mut self, iterator: BindingIterator) -> (u32, Atom) {
        let iterator_atom = self.hidden_local("\0rqj:binding-iterator");
        self.store_atom(iterator_atom, iterator.iterator);
        self.emit(
            Op::IteratorCleanupPush,
            iterator.iterator,
            iterator.done,
            0,
            0,
        );
        self.iterator_closures.push(iterator_atom);
        let error_atom = self.hidden_local("\0rqj:binding-iterator-error");
        (self.code.len() as u32, error_atom)
    }

    fn end_binding_iterator_protection(
        &mut self,
        iterator: BindingIterator,
        start: u32,
        error_atom: Atom,
    ) {
        let end = self.code.len() as u32;
        let normal_exit = self.emit(Op::Jump, 0, 0, 0, 0);
        let abrupt_target = self.code.len() as u32;
        self.handlers.push(crate::bytecode::Handler {
            start,
            end,
            target: abrupt_target,
            slot: self.local_slot(error_atom),
            return_target: None,
            return_slot: None,
        });

        let close = self.emit(Op::JumpFalse, iterator.done, 0, 0, 0);
        let close_exit = self.emit(Op::Jump, 0, 0, 0, 0);
        self.patch(close);
        let close_start = self.code.len() as u32;
        self.emit(Op::IteratorClose, 0, iterator.iterator, 0, 0);
        let close_end = self.code.len() as u32;
        let close_normal = self.emit(Op::Jump, 0, 0, 0, 0);
        let ignored_atom = self.hidden_local("\0rqj:binding-iterator-close-error");
        let ignored_target = self.code.len() as u32;
        self.handlers.push(crate::bytecode::Handler {
            start: close_start,
            end: close_end,
            target: ignored_target,
            slot: self.local_slot(ignored_atom),
            return_target: None,
            return_slot: None,
        });
        let rethrow_target = self.code.len() as u32;
        let original_error = self.load_atom(error_atom);
        self.emit(Op::IteratorCleanupPop, 0, 0, 0, 0);
        self.emit(Op::Throw, original_error, 0, 0, 0);
        self.patch_to(close_exit, rethrow_target);
        self.patch_to(close_normal, rethrow_target);
        self.patch_to(normal_exit, self.code.len() as u32);
        self.binding_iterator_close(iterator);
        self.emit(Op::IteratorCleanupPop, 0, 0, 0, 0);
        self.iterator_closures.pop();
    }

    fn emit_copy_data_properties(&mut self, source: Register, excluded: &[Register]) -> Register {
        let target = self.reg();
        self.emit(Op::MakeObject, target, 0, 0, 0);
        let exclusions = self.reg();
        self.emit(Op::MakeArray, exclusions, 0, 0, 0);
        for (position, excluded_key) in excluded.iter().copied().enumerate() {
            let index = self.literal(Constant::Number(position as f64));
            self.emit(Op::SetIndex, excluded_key, exclusions, index, 0);
        }
        self.emit(Op::CopyDataProperties, target, source, exclusions, 0);
        target
    }
}
