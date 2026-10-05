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
            let receiver = self.reg();
            self.emit(Op::LoadThis, receiver, 0, 0, 0);
            let key = self.expression(&value.expression);
            let base = self.expression(&value.object);
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
        let parent_chain = self.optional_chain_end_edges.take();
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
        self.optional_chain_end_edges = parent_chain;
        result
    }

    pub(super) fn chain_callee(&mut self, value: &ChainElement<'_>) -> (Register, Register) {
        let parent_chain = self.optional_chain_end_edges.take();
        let callee = self.literal(Constant::Undefined);
        let receiver = self.literal(Constant::Undefined);
        self.optional_chain_end_edges = Some(Vec::new());
        let (value, this) = match value {
            ChainElement::StaticMemberExpression(member) => self.static_member_callee(member),
            ChainElement::ComputedMemberExpression(member) => self.computed_member_callee(member),
            ChainElement::PrivateFieldExpression(member) => self.private_field_callee(member),
            _ => {
                let value = self.chain_element_expression(value);
                let this = self.literal(Constant::Undefined);
                (value, this)
            }
        };
        self.emit(Op::Move, callee, value, 0, 0);
        self.emit(Op::Move, receiver, this, 0, 0);
        let end = self.code.len() as u32;
        for edge in self.optional_chain_end_edges.take().unwrap_or_default() {
            self.patch_instruction(edge, end);
        }
        self.optional_chain_end_edges = parent_chain;
        (callee, receiver)
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
            ChainElement::PrivateFieldExpression(item) => self.private_field_callee(item).0,
        }
    }

    pub(super) fn delete_chain(&mut self, value: &ChainElement<'_>) -> Register {
        let parent_chain = self.optional_chain_end_edges.take();
        let result = self.literal(Constant::Boolean(true));
        self.optional_chain_end_edges = Some(Vec::new());
        let reference = match value {
            ChainElement::StaticMemberExpression(member) => {
                let receiver = self.expression(&member.object);
                if member.optional {
                    let (_, _, jump) = self.emit_optional_prefix(receiver);
                    self.patch_instruction(jump, self.code.len() as u32);
                }
                let key = self.literal(Constant::String(member.property.name.to_string()));
                Some((receiver, key))
            }
            ChainElement::ComputedMemberExpression(member) => {
                let receiver = self.expression(&member.object);
                if member.optional {
                    let (_, _, jump) = self.emit_optional_prefix(receiver);
                    self.patch_instruction(jump, self.code.len() as u32);
                }
                let key = self.expression_outside_optional_chain(&member.expression);
                Some((receiver, key))
            }
            _ => {
                self.chain_element_expression(value);
                None
            }
        };
        if let Some((receiver, key)) = reference {
            self.emit(Op::Delete, result, receiver, key, u32::from(self.strict));
        }
        let end = self.code.len() as u32;
        for edge in self.optional_chain_end_edges.take().unwrap_or_default() {
            self.patch_instruction(edge, end);
        }
        self.optional_chain_end_edges = parent_chain;
        result
    }

    pub(super) fn optional_call(&mut self, value: &CallExpression<'_>) -> Register {
        if matches!(&value.callee, Expression::Super(_)) {
            return self.call(value);
        }
        let (callee, this) = self.callee(&value.callee);
        let (dst, end) = if value.optional {
            let (dst, end, jump_call) = self.emit_optional_prefix(callee);
            self.patch_instruction(jump_call, self.code.len() as u32);
            (dst, end)
        } else {
            (self.reg(), None)
        };
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
