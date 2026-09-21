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
                if array.rest.is_some() {
                    self.owner
                        .reject(pattern.span(), "array rest is unsupported");
                }
                for (index, element) in array.elements.iter().enumerate() {
                    let Some(element) = element else { continue };
                    let key = self.literal(Constant::Number(index as f64));
                    let dst = self.reg();
                    self.emit(Op::GetIndex, dst, value, key, 0);
                    self.bind_pattern(element, dst);
                }
            }
            BindingPattern::AssignmentPattern(_) => {
                self.owner
                    .reject(pattern.span(), "destructuring defaults are unsupported");
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
