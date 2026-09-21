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
        for property in &value.properties {
            let ObjectPropertyKind::ObjectProperty(property) = property else {
                self.owner.reject(property.span(), "spread is unsupported");
                continue;
            };
            if property.computed {
                let Some(key) = self.computed_object_key(&property.key) else {
                    self.owner
                        .reject(property.span, "computed object key expression unsupported");
                    continue;
                };
                let item = self.expression(&property.value);
                self.emit(Op::SetIndex, item, dst, key, 0);
                continue;
            }
            let key = match &property.key {
                PropertyKey::StaticIdentifier(id) => id.name.as_str(),
                PropertyKey::StringLiteral(value) => value.value.as_str(),
                _ => {
                    self.owner
                        .reject(property.span, "computed object keys are unsupported");
                    continue;
                }
            };
            let item = self.expression(&property.value);
            let atom = self.owner.atom(key);
            let site = self.owner.cache_site();
            self.emit(Op::SetField, item, dst, site, atom);
        }
        dst
    }

    pub(super) fn computed_object_key(&mut self, key: &PropertyKey<'_>) -> Option<Register> {
        Some(match key {
            PropertyKey::Identifier(identifier) => self.load_name(identifier.name.as_str()),
            PropertyKey::StringLiteral(value) => {
                self.literal(Constant::String(value.value.to_string()))
            }
            PropertyKey::NumericLiteral(value) => self.literal(Constant::Number(value.value)),
            PropertyKey::BooleanLiteral(value) => self.literal(Constant::Boolean(value.value)),
            PropertyKey::NullLiteral(_) => self.literal(Constant::Null),
            _ => return None,
        })
    }
}
