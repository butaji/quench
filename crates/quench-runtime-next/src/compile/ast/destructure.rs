use super::*;

impl FunctionCompiler<'_, '_> {
    pub(super) fn bind_pattern(&mut self, pattern: &BindingPattern<'_>, value: Register) {
        match pattern {
            BindingPattern::BindingIdentifier(id) => {
                let atom = self.owner.atom(id.name.as_str());
                self.store_atom(atom, value);
            }
            BindingPattern::ObjectPattern(object) => {
                for property in &object.properties {
                    let Some(key) = Self::binding_key(&property.key) else {
                        self.owner
                            .reject(property.span, "destructuring key is unsupported");
                        continue;
                    };
                    let dst = self.reg();
                    let atom = self.owner.atom(key);
                    let cache = self.owner.cache_site();
                    self.emit(Op::GetField, dst, FieldBase::register(value).0, cache, atom);
                    self.bind_pattern(&property.value, dst);
                }
                if let Some(rest) = &object.rest {
                    let rest_value = self.object_rest(value, object);
                    self.bind_pattern(&rest.argument, rest_value);
                }
            }
            BindingPattern::ArrayPattern(array) => {
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
