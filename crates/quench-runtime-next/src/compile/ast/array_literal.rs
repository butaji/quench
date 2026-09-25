use super::*;

impl FunctionCompiler<'_, '_> {
    pub(super) fn array_expression(&mut self, value: &ArrayExpression<'_>) -> Register {
        if value
            .elements
            .iter()
            .any(|item| matches!(item, ArrayExpressionElement::SpreadElement(_)))
        {
            return self.array_expression_with_spread(value);
        }
        let dst = self.reg();
        let constants = value
            .elements
            .iter()
            .map(|item| {
                item.as_expression()
                    .and_then(|value| binding_time::expression(value).static_value())
            })
            .collect::<Option<Vec<_>>>();
        if let Some(constants) = constants
            && !constants.is_empty()
            && constants.len() <= u16::MAX as usize
        {
            let start = self.owner.constant_run(constants);
            self.emit(
                Op::MakeConstArray,
                dst,
                value.elements.len() as u16,
                0,
                start,
            );
            return dst;
        }
        self.emit(Op::MakeArray, dst, 0, 0, value.elements.len() as u32);
        for (index, item) in value.elements.iter().enumerate() {
            let Some(expr) = item.as_expression() else {
                continue;
            };
            let item = self.expression(expr);
            self.emit(Op::DefineArrayElement, item, dst, 0, index as u32);
        }
        dst
    }

    fn array_expression_with_spread(&mut self, value: &ArrayExpression<'_>) -> Register {
        let dst = self.reg();
        self.emit(Op::MakeArray, dst, 0, 0, 0);
        let push = self.reg();
        let push_atom = self.owner.atom("push");
        let push_cache = self.owner.cache_site();
        self.emit(
            Op::GetField,
            push,
            FieldBase::register(dst).0,
            push_cache,
            push_atom,
        );
        for item in &value.elements {
            if let ArrayExpressionElement::SpreadElement(spread) = item {
                let source = self.expression(&spread.argument);
                let expanded = self.reg();
                self.emit(Op::SpreadToArray, expanded, source, 0, 0);
                let apply = self.reg();
                let apply_atom = self.owner.atom("apply");
                let apply_cache = self.owner.cache_site();
                self.emit(
                    Op::GetField,
                    apply,
                    FieldBase::register(push).0,
                    apply_cache,
                    apply_atom,
                );
                let args_base = self.next_reg;
                let dst_arg = self.reg();
                self.emit(Op::Move, dst_arg, dst, 0, 0);
                let expanded_arg = self.reg();
                self.emit(Op::Move, expanded_arg, expanded, 0, 0);
                let ignored = self.reg();
                self.emit(
                    Op::Call,
                    ignored,
                    apply,
                    push,
                    crate::bytecode::ImmediateLayout::call_immediate(args_base, 2, false, false),
                );
            } else if let Some(expression) = item.as_expression() {
                let element = self.expression(expression);
                let base = self.next_reg;
                let element_arg = self.reg();
                self.emit(Op::Move, element_arg, element, 0, 0);
                let ignored = self.reg();
                self.emit(
                    Op::Call,
                    ignored,
                    push,
                    dst,
                    crate::bytecode::ImmediateLayout::call_immediate(base, 1, false, false),
                );
            }
        }
        dst
    }
}
