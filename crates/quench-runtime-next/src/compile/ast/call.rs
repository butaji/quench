use super::*;

impl FunctionCompiler<'_, '_> {
    pub(super) fn unary(&mut self, value: &UnaryExpression<'_>) -> Register {
        if value.operator == UnaryOperator::Delete {
            return self.delete_expression(&value.argument);
        }
        let input = if value.operator == UnaryOperator::Typeof {
            if let Expression::Identifier(identifier) = &value.argument {
                let atom = self.owner.atom(identifier.name.as_str());
                let bound = self.local_slots.contains_key(&atom)
                    || self.scopes.iter().any(|scope| scope.contains_key(&atom));
                if bound {
                    self.expression(&value.argument)
                } else {
                    let input = self.reg();
                    let cache = self.owner.cache_site();
                    self.emit(Op::LoadNameTypeof, input, 0, cache, atom);
                    input
                }
            } else {
                self.expression(&value.argument)
            }
        } else {
            self.expression(&value.argument)
        };
        let dst = self.reg();
        self.emit(Op::Unary, dst, input, 0, value.operator as u32);
        dst
    }

    fn delete_expression(&mut self, argument: &Expression<'_>) -> Register {
        let (target, key) = match argument {
            Expression::StaticMemberExpression(member) => {
                let target = self.expression(&member.object);
                let key = self.literal(Constant::String(member.property.name.to_string()));
                (target, key)
            }
            Expression::ComputedMemberExpression(member) => {
                let target = self.expression(&member.object);
                let key = self.expression(&member.expression);
                (target, key)
            }
            Expression::Identifier(identifier) if identifier.name == "arguments" => {
                return self.literal(Constant::Boolean(false));
            }
            other => {
                let _ = self.expression(other);
                return self.literal(Constant::Boolean(true));
            }
        };
        if self.strict {
            let dst = self.reg();
            self.emit(Op::Delete, dst, target, key, 0);
            return dst;
        }
        let reflect = self.load_name("Reflect");
        let delete_property = self.reg();
        let atom = self.owner.atom("deleteProperty");
        let cache = self.owner.cache_site();
        self.emit(
            Op::GetField,
            delete_property,
            FieldBase::register(reflect).0,
            cache,
            atom,
        );
        let this = self.literal(Constant::Undefined);
        let base = self.next_reg;
        let target_arg = self.reg();
        self.emit(Op::Move, target_arg, target, 0, 0);
        let key_arg = self.reg();
        self.emit(Op::Move, key_arg, key, 0, 0);
        let dst = self.reg();
        self.emit(
            Op::Call,
            dst,
            delete_property,
            this,
            (u32::from(base) << 16) | 2,
        );
        dst
    }

    pub(super) fn logical(&mut self, value: &LogicalExpression<'_>) -> Register {
        if value.operator.is_coalesce() {
            return self.coalesce(value);
        }
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
        if Self::has_spread(&value.arguments) {
            let (callee, this) = self.callee(&value.callee);
            return self.spread_call(callee, this, &value.arguments);
        }
        if let Expression::StaticMemberExpression(item) = &value.callee
            && !matches!(&item.object, Expression::Super(_))
        {
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
        if Self::has_spread(&value.arguments) {
            let arguments = self.spread_arguments(&value.arguments);
            let reflect = self.load_name("Reflect");
            let construct = self.reg();
            let atom = self.owner.atom("construct");
            let cache = self.owner.cache_site();
            self.emit(
                Op::GetField,
                construct,
                FieldBase::register(reflect).0,
                cache,
                atom,
            );
            let base = self.next_reg;
            let callee_arg = self.reg();
            self.emit(Op::Move, callee_arg, callee, 0, 0);
            let arguments_arg = self.reg();
            self.emit(Op::Move, arguments_arg, arguments, 0, 0);
            let dst = self.reg();
            self.emit(
                Op::Call,
                dst,
                construct,
                reflect,
                (u32::from(base) << 16) | 2,
            );
            return dst;
        }
        let (base, count) = self.arguments(&value.arguments);
        let dst = self.reg();
        self.emit(Op::Construct, dst, callee, base, u32::from(count));
        dst
    }

    pub(super) fn callee(&mut self, value: &Expression<'_>) -> (Register, Register) {
        match value {
            Expression::Super(_) => {
                let callee = self.expression(value);
                let this = self.reg();
                self.emit(Op::LoadThis, this, 0, 0, 0);
                (callee, this)
            }
            Expression::StaticMemberExpression(item) => {
                if matches!(&item.object, Expression::Super(_)) {
                    let base = self.expression(&item.object);
                    let target = if self.super_static {
                        base
                    } else {
                        let prototype = self.reg();
                        let atom = self.owner.atom("prototype");
                        let cache = self.owner.cache_site();
                        self.emit(
                            Op::GetField,
                            prototype,
                            FieldBase::register(base).0,
                            cache,
                            atom,
                        );
                        prototype
                    };
                    let callee = self.reg();
                    let method_atom = self.owner.atom(item.property.name.as_str());
                    let method_cache = self.owner.cache_site();
                    self.emit(
                        Op::GetField,
                        callee,
                        FieldBase::register(target).0,
                        method_cache,
                        method_atom,
                    );
                    let this = self.reg();
                    self.emit(Op::LoadThis, this, 0, 0, 0);
                    return (callee, this);
                }
                let this = self.expression(&item.object);
                let dst = self.reg();
                let atom = self.owner.atom(item.property.name.as_str());
                let cache = self.owner.cache_site();
                self.emit(Op::GetField, dst, FieldBase::register(this).0, cache, atom);
                (dst, this)
            }
            Expression::ComputedMemberExpression(item) => {
                let this = if matches!(&item.object, Expression::Super(_)) && !self.super_static {
                    let base = self.expression(&item.object);
                    let prototype = self.reg();
                    let atom = self.owner.atom("prototype");
                    let cache = self.owner.cache_site();
                    self.emit(
                        Op::GetField,
                        prototype,
                        FieldBase::register(base).0,
                        cache,
                        atom,
                    );
                    prototype
                } else {
                    self.expression(&item.object)
                };
                let key = self.expression(&item.expression);
                let dst = self.reg();
                self.emit(Op::GetIndex, dst, this, key, 0);
                if matches!(&item.object, Expression::Super(_)) {
                    let receiver = self.reg();
                    self.emit(Op::LoadThis, receiver, 0, 0, 0);
                    (dst, receiver)
                } else {
                    (dst, this)
                }
            }
            _ => {
                let callee = self.expression(value);
                let this = self.literal(Constant::Undefined);
                (callee, this)
            }
        }
    }

    pub(super) fn arguments(&mut self, values: &[Argument<'_>]) -> (Register, u16) {
        // Evaluate first, then reserve the contiguous ABI argument window.
        // Reserving targets before expression lowering lets expression
        // temporaries occupy later argument registers.
        let expressions = values
            .iter()
            .filter_map(|argument| {
                let Some(expression) = argument.as_expression() else {
                    self.owner
                        .reject(argument.span(), "spread arguments unsupported");
                    return None;
                };
                Some(self.expression(expression))
            })
            .collect::<Vec<_>>();
        let base = self.next_reg;
        let targets: Vec<_> = (0..expressions.len()).map(|_| self.reg()).collect();
        let count = targets.len() as u16;
        for (value, target) in expressions.into_iter().zip(targets) {
            self.emit(Op::Move, target, value, 0, 0);
        }
        (base, count)
    }

    pub(super) fn has_spread(values: &[Argument<'_>]) -> bool {
        values
            .iter()
            .any(|argument| matches!(argument, Argument::SpreadElement(_)))
    }

    pub(super) fn spread_call(
        &mut self,
        callee: Register,
        this: Register,
        values: &[Argument<'_>],
    ) -> Register {
        let arguments = self.spread_arguments(values);

        let apply = self.reg();
        let apply_atom = self.owner.atom("apply");
        let apply_cache = self.owner.cache_site();
        self.emit(
            Op::GetField,
            apply,
            FieldBase::register(callee).0,
            apply_cache,
            apply_atom,
        );
        let base = self.next_reg;
        let this_arg = self.reg();
        self.emit(Op::Move, this_arg, this, 0, 0);
        let values_arg = self.reg();
        self.emit(Op::Move, values_arg, arguments, 0, 0);
        let result = self.reg();
        self.emit(Op::Call, result, apply, callee, (u32::from(base) << 16) | 2);
        result
    }

    pub(super) fn spread_arguments(&mut self, values: &[Argument<'_>]) -> Register {
        let arguments = self.reg();
        self.emit(Op::MakeArray, arguments, 0, 0, 0);

        let push = self.reg();
        let push_atom = self.owner.atom("push");
        let push_cache = self.owner.cache_site();
        self.emit(
            Op::GetField,
            push,
            FieldBase::register(arguments).0,
            push_cache,
            push_atom,
        );
        for argument in values {
            let (method, receiver, first, second) = match argument {
                Argument::SpreadElement(spread) => {
                    let value = self.expression(&spread.argument);
                    let array = self.load_name("Array");
                    let from = self.reg();
                    let from_atom = self.owner.atom("from");
                    let from_cache = self.owner.cache_site();
                    self.emit(
                        Op::GetField,
                        from,
                        FieldBase::register(array).0,
                        from_cache,
                        from_atom,
                    );
                    let base = self.next_reg;
                    let value_arg = self.reg();
                    self.emit(Op::Move, value_arg, value, 0, 0);
                    let expanded = self.reg();
                    self.emit(Op::Call, expanded, from, array, (u32::from(base) << 16) | 1);
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
                    (apply, push, arguments, Some(expanded))
                }
                argument => (
                    push,
                    arguments,
                    self.expression(argument.as_expression().expect("expression argument")),
                    None,
                ),
            };
            let base = self.next_reg;
            let first_arg = self.reg();
            self.emit(Op::Move, first_arg, first, 0, 0);
            if let Some(second) = second {
                let second_arg = self.reg();
                self.emit(Op::Move, second_arg, second, 0, 0);
            }
            let result = self.reg();
            let count = if second.is_some() { 2 } else { 1 };
            self.emit(
                Op::Call,
                result,
                method,
                receiver,
                (u32::from(base) << 16) | count,
            );
        }

        arguments
    }

    pub(super) fn argument_registers(&mut self, values: &[Argument<'_>]) -> Vec<Register> {
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
