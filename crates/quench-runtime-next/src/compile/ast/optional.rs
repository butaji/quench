use super::*;

impl FunctionCompiler<'_, '_> {
    pub(super) fn static_member(&mut self, value: &StaticMemberExpression<'_>) -> Register {
        if value.optional {
            self.optional_static_get(&value.object, value.property.name.as_str())
        } else if matches!(&value.object, Expression::Super(_)) {
            let base = self.expression(&value.object);
            let key = self.literal(Constant::String(value.property.name.to_string()));
            let receiver = self.reg();
            self.emit(Op::LoadThis, receiver, 0, 0, 0);
            self.super_get(base, key, receiver)
        } else {
            self.static_get(&value.object, value.property.name.as_str())
        }
    }

    pub(super) fn computed_member(&mut self, value: &ComputedMemberExpression<'_>) -> Register {
        if value.optional {
            self.optional_computed_get(&value.object, &value.expression)
        } else if matches!(&value.object, Expression::Super(_)) {
            let base = self.expression(&value.object);
            let receiver = self.reg();
            self.emit(Op::LoadThis, receiver, 0, 0, 0);
            let key = self.expression(&value.expression);
            self.super_get(base, key, receiver)
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
        if let Some(end) = end {
            self.patch(end);
        }
        dst
    }

    pub(super) fn optional_computed_get(
        &mut self,
        object: &Expression<'_>,
        key: &Expression<'_>,
    ) -> Register {
        let base = self.expression(object);
        let (dst, end, jump_property) = self.emit_optional_prefix(base);
        self.patch_instruction(jump_property, self.code.len() as u32);
        let key = self.expression_outside_optional_chain(key);
        self.emit(Op::GetIndex, dst, base, key, 0);
        if let Some(end) = end {
            self.patch(end);
        }
        dst
    }

    pub(super) fn expression_outside_optional_chain(
        &mut self,
        expression: &Expression<'_>,
    ) -> Register {
        let chain = self.optional_chain_end_edges.take();
        let value = self.expression(expression);
        self.optional_chain_end_edges = chain;
        value
    }

    pub(super) fn emit_optional_prefix(
        &mut self,
        value: Register,
    ) -> (Register, Option<usize>, usize) {
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
        let end = if let Some(edges) = &mut self.optional_chain_end_edges {
            edges.push(end);
            None
        } else {
            Some(end)
        };
        (dst, end, jump_not_undefined)
    }

    pub(super) fn chain_expression(&mut self, value: &ChainElement<'_>) -> Register {
        if self.optional_chain_end_edges.is_some() {
            return self.chain_element_expression(value);
        }
        let result = self.reg();
        let undefined = self.literal(Constant::Undefined);
        self.emit(Op::Move, result, undefined, 0, 0);
        self.optional_chain_end_edges = Some(Vec::new());
        let value = self.chain_element_expression(value);
        self.emit(Op::Move, result, value, 0, 0);
        let end = self.code.len() as u32;
        for edge in self.optional_chain_end_edges.take().unwrap_or_default() {
            self.patch_instruction(edge, end);
        }
        result
    }

    fn chain_element_expression(&mut self, value: &ChainElement<'_>) -> Register {
        match value {
            ChainElement::CallExpression(item) => self.optional_call(item),
            ChainElement::TSNonNullExpression(item) => self.expression(&item.expression),
            ChainElement::StaticMemberExpression(item) => {
                if item.optional {
                    self.optional_static_get(&item.object, item.property.name.as_str())
                } else {
                    self.static_get(&item.object, item.property.name.as_str())
                }
            }
            ChainElement::ComputedMemberExpression(item) => {
                if item.optional {
                    self.optional_computed_get(&item.object, &item.expression)
                } else {
                    self.computed_get(&item.object, &item.expression)
                }
            }
            ChainElement::PrivateFieldExpression(item) => {
                let receiver = self.expression(&item.object);
                let (dst, end, jump_property) = self.emit_optional_prefix(receiver);
                self.patch_instruction(jump_property, self.code.len() as u32);
                let atom = self.owner.private_name_atom(item.field.span);
                let cache = self.owner.cache_site();
                self.emit(
                    Op::GetField,
                    dst,
                    FieldBase::register(receiver).0,
                    cache,
                    atom,
                );
                if let Some(end) = end {
                    self.patch(end);
                }
                dst
            }
        }
    }

    pub(super) fn optional_call(&mut self, value: &CallExpression<'_>) -> Register {
        if matches!(&value.callee, Expression::Super(_)) {
            return self.call(value);
        }
        if let Expression::StaticMemberExpression(item) = &value.callee
            && item.optional
        {
            let receiver = self.expression(&item.object);
            let (dst, end, jump_property) = self.emit_optional_prefix(receiver);
            if Self::has_spread(&value.arguments) {
                let callee = self.reg();
                let atom = self.owner.atom(item.property.name.as_str());
                let cache = self.owner.cache_site();
                self.patch_instruction(jump_property, self.code.len() as u32);
                self.emit(Op::GetField, callee, receiver, cache, atom);
                let result = self.spread_call(callee, receiver, &value.arguments);
                self.emit(Op::Move, dst, result, 0, 0);
                if let Some(end) = end {
                    self.patch(end);
                }
                return dst;
            }
            self.patch_instruction(jump_property, self.code.len() as u32);
            let chain = self.optional_chain_end_edges.take();
            let args = self.argument_registers(&value.arguments);
            self.optional_chain_end_edges = chain;
            let atom = self.owner.atom(item.property.name.as_str());
            let cache = self.owner.cache_site();
            let meta = self.owner.method_sites.len() as u32;
            self.owner.method_sites.push((atom, cache, args, None));
            self.emit(Op::CallMethod, dst, receiver, 0, meta);
            if let Some(end) = end {
                self.patch(end);
            }
            return dst;
        }
        let (callee, this) = self.callee(&value.callee);
        let (dst, end, jump_call) = self.emit_optional_prefix(callee);
        self.patch_instruction(jump_call, self.code.len() as u32);
        if Self::has_spread(&value.arguments) {
            let chain = self.optional_chain_end_edges.take();
            let result = self.spread_call(callee, this, &value.arguments);
            self.optional_chain_end_edges = chain;
            self.emit(Op::Move, dst, result, 0, 0);
            if let Some(end) = end {
                self.patch(end);
            }
            return dst;
        }
        let chain = self.optional_chain_end_edges.take();
        let (base, count) = self.arguments(&value.arguments);
        self.optional_chain_end_edges = chain;
        self.emit(
            Op::Call,
            dst,
            callee,
            this,
            crate::bytecode::ImmediateLayout::call_immediate(base, count, false, false),
        );
        if let Some(end) = end {
            self.patch(end);
        }
        dst
    }
}
