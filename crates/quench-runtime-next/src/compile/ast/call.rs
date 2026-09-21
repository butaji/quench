use super::*;

impl FunctionCompiler<'_, '_> {
    pub(super) fn unary(&mut self, value: &UnaryExpression<'_>) -> Register {
        let input = self.expression(&value.argument);
        let dst = self.reg();
        self.emit(Op::Unary, dst, input, 0, value.operator as u32);
        dst
    }

    pub(super) fn logical(&mut self, value: &LogicalExpression<'_>) -> Register {
        let left = self.expression(&value.left);
        let dst = self.reg();
        self.emit(Op::Move, dst, left, 0, 0);
        let jump = self.emit(Op::JumpFalse, left, 0, 0, 0);
        if value.operator.is_or() {
            let skip = self.emit(Op::Jump, 0, 0, 0, 0);
            self.patch(jump);
            let right = self.expression(&value.right);
            self.emit(Op::Move, dst, right, 0, 0);
            self.patch(skip);
        } else {
            let right = self.expression(&value.right);
            self.emit(Op::Move, dst, right, 0, 0);
            self.patch(jump);
        }
        dst
    }

    pub(super) fn conditional(&mut self, value: &ConditionalExpression<'_>) -> Register {
        let dst = self.reg();
        let test = self.expression(&value.test);
        let other = self.emit(Op::JumpFalse, test, 0, 0, 0);
        let yes = self.expression(&value.consequent);
        self.emit(Op::Move, dst, yes, 0, 0);
        let end = self.emit(Op::Jump, 0, 0, 0, 0);
        self.patch(other);
        let no = self.expression(&value.alternate);
        self.emit(Op::Move, dst, no, 0, 0);
        self.patch(end);
        dst
    }

    pub(super) fn call(&mut self, value: &CallExpression<'_>) -> Register {
        if let Expression::StaticMemberExpression(item) = &value.callee {
            let (receiver, receiver_path) = match &item.object {
                Expression::ThisExpression(_) => (None, None),
                Expression::StaticMemberExpression(inner)
                    if matches!(&inner.object, Expression::ThisExpression(_)) =>
                {
                    let atom = self.owner.atom(inner.property.name.as_str());
                    let cache = self.owner.cache_site();
                    (None, Some((atom, cache)))
                }
                other => (Some(self.expression(other)), None),
            };
            let atom = self.owner.atom(item.property.name.as_str());
            let cache = self.owner.cache_site();
            let args = self.argument_registers(&value.arguments);
            let meta = self.owner.method_sites.len() as u32;
            self.owner
                .method_sites
                .push((atom, cache, args, receiver_path));
            let dst = self.reg();
            if let Some(receiver) = receiver {
                self.emit(Op::CallMethod, dst, receiver, 0, meta);
            } else {
                self.emit(Op::CallThisMethod, dst, 0, 0, meta);
            }
            return dst;
        }
        let (callee, this) = self.callee(&value.callee);
        let (base, count) = self.arguments(&value.arguments);
        let dst = self.reg();
        self.emit(
            Op::Call,
            dst,
            callee,
            this,
            (u32::from(base) << 16) | u32::from(count),
        );
        dst
    }

    pub(super) fn construct(&mut self, value: &NewExpression<'_>) -> Register {
        let callee = self.expression(&value.callee);
        let (base, count) = self.arguments(&value.arguments);
        let dst = self.reg();
        self.emit(Op::Construct, dst, callee, base, u32::from(count));
        dst
    }

    pub(super) fn callee(&mut self, value: &Expression<'_>) -> (Register, Register) {
        match value {
            Expression::StaticMemberExpression(item) => {
                let this = self.expression(&item.object);
                let dst = self.reg();
                let atom = self.owner.atom(item.property.name.as_str());
                let cache = self.owner.cache_site();
                self.emit(Op::GetField, dst, FieldBase::register(this).0, cache, atom);
                (dst, this)
            }
            Expression::ComputedMemberExpression(item) => {
                let this = self.expression(&item.object);
                let key = self.expression(&item.expression);
                let dst = self.reg();
                self.emit(Op::GetIndex, dst, this, key, 0);
                (dst, this)
            }
            _ => {
                let callee = self.expression(value);
                let this = self.literal(Constant::Undefined);
                (callee, this)
            }
        }
    }

    pub(super) fn arguments(&mut self, values: &[Argument<'_>]) -> (Register, u16) {
        if values.len() > 8 {
            let span = values.first().map_or(Span::default(), GetSpan::span);
            self.owner
                .reject(span, "at most eight call arguments are supported");
        }
        let base = self.next_reg;
        let targets: Vec<_> = (0..values.len()).map(|_| self.reg()).collect();
        for (argument, target) in values.iter().zip(targets) {
            let Some(expression) = argument.as_expression() else {
                self.owner
                    .reject(argument.span(), "spread arguments unsupported");
                continue;
            };
            let value = self.expression(expression);
            self.emit(Op::Move, target, value, 0, 0);
        }
        (base, values.len() as u16)
    }

    pub(super) fn argument_registers(&mut self, values: &[Argument<'_>]) -> Vec<Register> {
        if values.len() > 8 {
            let span = values.first().map_or(Span::default(), GetSpan::span);
            self.owner
                .reject(span, "at most eight call arguments are supported");
        }
        values
            .iter()
            .filter_map(|argument| {
                let Some(expression) = argument.as_expression() else {
                    self.owner
                        .reject(argument.span(), "spread arguments unsupported");
                    return None;
                };
                Some(self.expression(expression))
            })
            .collect()
    }
}
