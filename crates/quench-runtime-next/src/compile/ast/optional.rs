use super::*;

impl FunctionCompiler<'_, '_> {
    pub(super) fn static_member(&mut self, value: &StaticMemberExpression<'_>) -> Register {
        if value.optional {
            self.optional_static_get(&value.object, value.property.name.as_str())
        } else {
            self.static_get(&value.object, value.property.name.as_str())
        }
    }

    pub(super) fn computed_member(&mut self, value: &ComputedMemberExpression<'_>) -> Register {
        if value.optional {
            self.optional_computed_get(&value.object, &value.expression)
        } else {
            self.computed_get(&value.object, &value.expression)
        }
    }

    pub(super) fn optional_static_get(&mut self, object: &Expression<'_>, key: &str) -> Register {
        let base = self.expression(object);
        let (dst, end, jump_property) = self.emit_optional_prefix(base);
        let atom = self.owner.atom(key);
        let cache = self.owner.cache_site();
        self.patch_instruction(jump_property, self.code.len() as u32);
        self.emit(Op::GetField, dst, FieldBase::register(base).0, cache, atom);
        self.patch(end);
        dst
    }

    pub(super) fn optional_computed_get(
        &mut self,
        object: &Expression<'_>,
        key: &Expression<'_>,
    ) -> Register {
        let base = self.expression(object);
        let (dst, end, jump_property) = self.emit_optional_prefix(base);
        let key = self.expression(key);
        self.patch_instruction(jump_property, self.code.len() as u32);
        self.emit(Op::GetIndex, dst, base, key, 0);
        self.patch(end);
        dst
    }

    fn emit_optional_prefix(&mut self, value: Register) -> (Register, usize, usize) {
        let dst = self.reg();
        let null = self.literal(Constant::Null);
        let is_null = self.emit_binary(2, Operand::register(value), Operand::register(null));
        let jump_not_null = self.emit(Op::JumpFalse, is_null, 0, 0, 0);
        let jump_null = self.emit(Op::Jump, 0, 0, 0, 0);
        let check_undefined = self.code.len() as u32;
        self.patch_instruction(jump_not_null, check_undefined);
        let undefined = self.literal(Constant::Undefined);
        let is_undefined =
            self.emit_binary(2, Operand::register(value), Operand::register(undefined));
        let jump_not_undefined = self.emit(Op::JumpFalse, is_undefined, 0, 0, 0);
        let undefined_block = self.code.len() as u32;
        self.patch_instruction(jump_null, undefined_block);
        let undefined = self.literal(Constant::Undefined);
        self.emit(Op::Move, dst, undefined, 0, 0);
        let end = self.emit(Op::Jump, 0, 0, 0, 0);
        (dst, end, jump_not_undefined)
    }

    pub(super) fn chain_expression(&mut self, value: &ChainElement<'_>) -> Register {
        match value {
            ChainElement::StaticMemberExpression(item) => {
                self.optional_static_get(&item.object, item.property.name.as_str())
            }
            ChainElement::ComputedMemberExpression(item) => {
                self.optional_computed_get(&item.object, &item.expression)
            }
            _ => {
                self.owner
                    .reject(value.span(), "optional chain is unsupported");
                self.literal(Constant::Undefined)
            }
        }
    }
}
