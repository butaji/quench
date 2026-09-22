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
            let property = match property {
                ObjectPropertyKind::ObjectProperty(property) => property,
                ObjectPropertyKind::SpreadProperty(spread) => {
                    self.object_spread(dst, &spread.argument);
                    continue;
                }
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
            if let PropertyKey::StringLiteral(value) = &property.key {
                let key = super::super::string::constant(value);
                if matches!(&key, Constant::StringUnits(_)) {
                    let key = self.literal(key);
                    let item = self.expression(&property.value);
                    self.emit(Op::SetIndex, item, dst, key, 0);
                    continue;
                }
            }
            let key = match &property.key {
                PropertyKey::StaticIdentifier(id) => id.name.as_str(),
                PropertyKey::StringLiteral(value) => value.value.as_str(),
                _ => {
                    if let Some(expression) = property.key.as_expression() {
                        let key = self.expression(expression);
                        let item = self.expression(&property.value);
                        self.emit(Op::SetIndex, item, dst, key, 0);
                        continue;
                    }
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

    fn object_spread(&mut self, target: Register, source: &Expression<'_>) {
        let source = self.expression(source);
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
            (u32::from(base) << 16) | 2,
        );
    }

    pub(super) fn computed_object_key(&mut self, key: &PropertyKey<'_>) -> Option<Register> {
        key.as_expression()
            .map(|expression| self.expression(expression))
    }
}
