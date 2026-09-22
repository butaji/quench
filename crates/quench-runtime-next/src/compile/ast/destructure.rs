use super::*;

impl FunctionCompiler<'_, '_> {
    pub(super) fn assignment(&mut self, value: &AssignmentExpression<'_>) -> Register {
        let right = self.expression(&value.right);
        if let Some(target) = value.left.as_simple_assignment_target() {
            self.assign_target(target, right, value.operator as u8)
        } else if value.operator == AssignmentOperator::Assign {
            self.assign_pattern(&value.left, right);
            right
        } else {
            self.owner
                .reject(value.span, "compound assignment pattern unsupported");
            right
        }
    }

    pub(super) fn assign_pattern(&mut self, target: &AssignmentTarget<'_>, value: Register) {
        if let Some(simple) = target.as_simple_assignment_target() {
            self.assign_target(simple, value, 0);
            return;
        }
        match target {
            AssignmentTarget::ArrayAssignmentTarget(array) => {
                self.require_object_coercible(value);
                for (index, element) in array.elements.iter().enumerate() {
                    let Some(element) = element else { continue };
                    let key = self.literal(Constant::Number(index as f64));
                    let selected = self.reg();
                    self.emit(Op::GetIndex, selected, value, key, 0);
                    self.assign_maybe_default(element, selected);
                }
                if let Some(rest) = &array.rest {
                    let start = self.literal(Constant::Number(array.elements.len() as f64));
                    let method = self.reg();
                    let atom = self.owner.atom("slice");
                    let cache = self.owner.cache_site();
                    self.emit(
                        Op::GetField,
                        method,
                        FieldBase::register(value).0,
                        cache,
                        atom,
                    );
                    let argument = self.reg();
                    self.emit(Op::Move, argument, start, 0, 0);
                    let rest_value = self.reg();
                    self.emit(
                        Op::Call,
                        rest_value,
                        method,
                        value,
                        (u32::from(argument) << 16) | 1,
                    );
                    self.assign_pattern(&rest.target, rest_value);
                }
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
                                self.assign_default_value(selected, init);
                            }
                            self.store_atom(atom, selected);
                        }
                        AssignmentTargetProperty::AssignmentTargetPropertyProperty(property) => {
                            if let Some(expression) = property.name.as_expression() {
                                let key = self.expression(expression);
                                self.resolve_assignment_maybe_default(&property.binding);
                                excluded.push(key);
                                self.emit(Op::GetIndex, selected, value, key, 0);
                            } else if let Some(name) = Self::binding_key(&property.name) {
                                let atom = self.owner.atom(name);
                                self.resolve_assignment_maybe_default(&property.binding);
                                excluded.push(self.literal(Constant::String(name.to_owned())));
                                let cache = self.owner.cache_site();
                                self.emit(
                                    Op::GetField,
                                    selected,
                                    FieldBase::register(value).0,
                                    cache,
                                    atom,
                                );
                            } else {
                                self.owner
                                    .reject(property.span(), "destructuring key is unsupported");
                                continue;
                            }
                            self.assign_maybe_default(&property.binding, selected);
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

    fn assign_maybe_default(&mut self, target: &AssignmentTargetMaybeDefault<'_>, value: Register) {
        match target {
            AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(default) => {
                self.assign_default_value(value, &default.init);
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

    fn assign_default_value(&mut self, value: Register, init: &Expression<'_>) {
        let undefined = self.literal(Constant::Undefined);
        let missing = self.emit_binary(2, Operand::register(value), Operand::register(undefined));
        let skip = self.emit(Op::JumpFalse, missing, 0, 0, 0);
        let fallback = self.expression(init);
        self.emit(Op::Move, value, fallback, 0, 0);
        self.patch(skip);
    }

    fn assignment_object_rest(&mut self, source: Register, excluded: &[Register]) -> Register {
        let target = self.reg();
        self.emit(Op::MakeObject, target, 0, 0, 0);
        let object = self.load_name("Object");
        let keys_fn = self.reg();
        let keys_atom = self.owner.atom("keys");
        let keys_cache = self.owner.cache_site();
        self.emit(
            Op::GetField,
            keys_fn,
            FieldBase::register(object).0,
            keys_cache,
            keys_atom,
        );
        let argument = self.reg();
        self.emit(Op::Move, argument, source, 0, 0);
        let keys = self.reg();
        self.emit(
            Op::Call,
            keys,
            keys_fn,
            object,
            (u32::from(argument) << 16) | 1,
        );
        let index = self.reg();
        let zero = self.literal(Constant::Number(0.0));
        self.emit(Op::Move, index, zero, 0, 0);
        let head = self.code.len() as u32;
        let length = self.reg();
        let length_atom = self.owner.atom("length");
        let length_cache = self.owner.cache_site();
        self.emit(
            Op::GetField,
            length,
            FieldBase::register(keys).0,
            length_cache,
            length_atom,
        );
        let test = self.emit_binary(4, Operand::register(index), Operand::register(length));
        let end_edge = self.emit(Op::JumpFalse, test, 0, 0, 0);
        let key = self.reg();
        self.emit(Op::GetIndex, key, keys, index, 0);
        let mut skip_edges = Vec::with_capacity(excluded.len());
        for excluded_key in excluded {
            let matched =
                self.emit_binary(2, Operand::register(key), Operand::register(*excluded_key));
            let not_matched = self.emit(Op::JumpFalse, matched, 0, 0, 0);
            let skip = self.emit(Op::Jump, 0, 0, 0, 0);
            self.patch(not_matched);
            skip_edges.push(skip);
        }
        let item = self.reg();
        self.emit(Op::GetIndex, item, source, key, 0);
        self.emit(Op::SetIndex, item, target, key, 0);
        let update = self.code.len() as u32;
        self.patch_edges(&skip_edges, update);
        let next = self.reg();
        self.emit(Op::IncDec, next, index, 0, 0);
        self.emit(Op::Move, index, next, 0, 0);
        self.emit(Op::Jump, 0, 0, 0, head);
        let end = self.code.len() as u32;
        self.patch_to(end_edge, end);
        target
    }

    pub(super) fn bind_pattern(&mut self, pattern: &BindingPattern<'_>, value: Register) {
        match pattern {
            BindingPattern::BindingIdentifier(id) => {
                let atom = self.owner.atom(id.name.as_str());
                self.store_atom(atom, value);
            }
            BindingPattern::ObjectPattern(object) => {
                self.require_object_coercible(value);
                for property in &object.properties {
                    let dst = self.reg();
                    if let Some(expression) = property.key.as_expression() {
                        let key = self.expression(expression);
                        self.emit(Op::GetIndex, dst, value, key, 0);
                    } else if let Some(key) = Self::binding_key(&property.key) {
                        let atom = self.owner.atom(key);
                        let cache = self.owner.cache_site();
                        self.emit(Op::GetField, dst, FieldBase::register(value).0, cache, atom);
                    } else {
                        self.owner
                            .reject(property.span, "destructuring key is unsupported");
                        continue;
                    }
                    self.bind_pattern(&property.value, dst);
                }
                if let Some(rest) = &object.rest {
                    let rest_value = self.object_rest(value, object);
                    self.bind_pattern(&rest.argument, rest_value);
                }
            }
            BindingPattern::ArrayPattern(array) => {
                self.require_object_coercible(value);
                for (index, element) in array.elements.iter().enumerate() {
                    let Some(element) = element else { continue };
                    let key = self.literal(Constant::Number(index as f64));
                    let dst = self.reg();
                    self.emit(Op::GetIndex, dst, value, key, 0);
                    self.bind_pattern(element, dst);
                }
                if let Some(rest) = &array.rest {
                    let start = self.literal(Constant::Number(array.elements.len() as f64));
                    let method = self.reg();
                    let atom = self.owner.atom("slice");
                    let cache = self.owner.cache_site();
                    self.emit(
                        Op::GetField,
                        method,
                        FieldBase::register(value).0,
                        cache,
                        atom,
                    );
                    let argument = self.reg();
                    self.emit(Op::Move, argument, start, 0, 0);
                    let rest_value = self.reg();
                    self.emit(
                        Op::Call,
                        rest_value,
                        method,
                        value,
                        (u32::from(argument) << 16) | 1,
                    );
                    self.bind_pattern(&rest.argument, rest_value);
                }
            }
            BindingPattern::AssignmentPattern(assignment) => {
                let selected = self.reg();
                self.emit(Op::Move, selected, value, 0, 0);
                let undefined = self.literal(Constant::Undefined);
                let missing =
                    self.emit_binary(2, Operand::register(value), Operand::register(undefined));
                let skip = self.emit(Op::JumpFalse, missing, 0, 0, 0);
                let fallback = self.expression(&assignment.right);
                self.emit(Op::Move, selected, fallback, 0, 0);
                self.patch(skip);
                self.bind_pattern(&assignment.left, selected);
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

    fn resolve_assignment_maybe_default(
        &mut self,
        target: &AssignmentTargetMaybeDefault<'_>,
    ) {
        let target = match target {
            AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(target) => {
                &target.binding
            }
            _ => match target.as_assignment_target() {
                Some(target) => target,
                None => return,
            },
        };
        if let Some(SimpleAssignmentTarget::AssignmentTargetIdentifier(id)) =
            target.as_simple_assignment_target()
        {
            let atom = self.owner.atom(id.name.as_str());
            self.resolve_assignment_name(atom);
        }
    }

    fn resolve_assignment_name(&mut self, atom: Atom) {
        if self.with_depth != 0 {
            let _ = self.load_atom(atom);
        }
    }

    fn throw_object_coercion(&mut self) {
        let constructor = self.load_name("TypeError");
        let message = self.literal(Constant::String("cannot destructure null or undefined".into()));
        let argument = self.reg();
        self.emit(Op::Move, argument, message, 0, 0);
        let error = self.reg();
        self.emit(Op::Construct, error, constructor, argument, 1);
        self.emit(Op::Throw, error, 0, 0, 0);
    }

    fn object_rest(&mut self, source: Register, pattern: &ObjectPattern<'_>) -> Register {
        let target = self.reg();
        self.emit(Op::MakeObject, target, 0, 0, 0);

        let object = self.load_name("Object");
        let keys_fn = self.reg();
        let keys_atom = self.owner.atom("keys");
        let keys_cache = self.owner.cache_site();
        self.emit(
            Op::GetField,
            keys_fn,
            FieldBase::register(object).0,
            keys_cache,
            keys_atom,
        );
        let argument = self.reg();
        self.emit(Op::Move, argument, source, 0, 0);
        let keys = self.reg();
        self.emit(
            Op::Call,
            keys,
            keys_fn,
            object,
            (u32::from(argument) << 16) | 1,
        );
        let index = self.reg();
        let zero = self.literal(Constant::Number(0.0));
        self.emit(Op::Move, index, zero, 0, 0);
        let head = self.code.len() as u32;
        let length = self.reg();
        let length_atom = self.owner.atom("length");
        let length_cache = self.owner.cache_site();
        self.emit(
            Op::GetField,
            length,
            FieldBase::register(keys).0,
            length_cache,
            length_atom,
        );
        let test = self.emit_binary(4, Operand::register(index), Operand::register(length));
        let end_edge = self.emit(Op::JumpFalse, test, 0, 0, 0);
        let key = self.reg();
        self.emit(Op::GetIndex, key, keys, index, 0);
        let mut skip_edges = Vec::with_capacity(pattern.properties.len());
        for property in &pattern.properties {
            let Some(name) = Self::binding_key(&property.key) else {
                continue;
            };
            let excluded = self.literal(Constant::String(name.to_owned()));
            let matched = self.emit_binary(2, Operand::register(key), Operand::register(excluded));
            let not_matched = self.emit(Op::JumpFalse, matched, 0, 0, 0);
            let skip = self.emit(Op::Jump, 0, 0, 0, 0);
            self.patch(not_matched);
            skip_edges.push(skip);
        }
        let item = self.reg();
        self.emit(Op::GetIndex, item, source, key, 0);
        self.emit(Op::SetIndex, item, target, key, 0);
        let update = self.code.len() as u32;
        self.patch_edges(&skip_edges, update);
        let next = self.reg();
        self.emit(Op::IncDec, next, index, 0, 0);
        self.emit(Op::Move, index, next, 0, 0);
        self.emit(Op::Jump, 0, 0, 0, head);
        let end = self.code.len() as u32;
        self.patch_to(end_edge, end);
        target
    }
}
