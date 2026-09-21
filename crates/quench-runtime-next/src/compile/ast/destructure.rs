use super::*;

impl FunctionCompiler<'_, '_> {
    pub(super) fn bind_pattern(&mut self, pattern: &BindingPattern<'_>, value: Register) {
        match pattern {
            BindingPattern::BindingIdentifier(id) => {
                let atom = self.owner.atom(id.name.as_str());
                self.store_atom(atom, value);
            }
            BindingPattern::ObjectPattern(object) => {
                if object.rest.is_some() {
                    self.owner
                        .reject(pattern.span(), "object rest is unsupported");
                }
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
}
