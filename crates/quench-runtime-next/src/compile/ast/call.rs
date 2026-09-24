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
            } else if let Expression::ParenthesizedExpression(parenthesized) = &value.argument
                && let Expression::Identifier(identifier) = &parenthesized.expression
            {
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
            Expression::ParenthesizedExpression(parenthesized) => {
                return self.delete_expression(&parenthesized.expression);
            }
            Expression::StaticMemberExpression(member)
                if matches!(&member.object, Expression::Super(_)) =>
            {
                return self.throw_super_delete(None);
            }
            Expression::ComputedMemberExpression(member)
                if matches!(&member.object, Expression::Super(_)) =>
            {
                return self.throw_super_delete(Some(&member.expression));
            }
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
            Expression::Identifier(identifier) => {
                if self.strict {
                    self.owner
                        .reject(identifier.span, "delete of an unqualified identifier");
                    return self.literal(Constant::Undefined);
                }
                let atom = self.owner.atom(identifier.name.as_str());
                let result = self.reg();
                self.emit(Op::DeleteName, result, 0, 0, atom);
                return result;
            }
            other => {
                let _ = self.expression(other);
                return self.literal(Constant::Boolean(true));
            }
        };
        let dst = self.reg();
        self.emit(Op::Delete, dst, target, key, u32::from(self.strict));
        dst
    }

    fn throw_super_delete(&mut self, key: Option<&Expression<'_>>) -> Register {
        let receiver = self.reg();
        self.emit(Op::LoadThis, receiver, 0, 0, 0);
        if let Some(key) = key {
            self.expression(key);
        }
        let constructor = self.load_name("ReferenceError");
        let error = self.reg();
        self.emit(Op::Construct, error, constructor, constructor, 0);
        self.emit(Op::Throw, error, 0, 0, 0);
        self.literal(Constant::Undefined)
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
            if self.is_direct_eval_reference(&value.callee) {
                return self.direct_eval_spread_call(value);
            }
            if matches!(&value.callee, Expression::Super(_)) {
                let arguments = self.spread_arguments(&value.arguments);
                let superclass = self.load_name("\0rqj:super");
                let result = self.reg();
                self.emit(
                    Op::Construct,
                    result,
                    superclass,
                    arguments,
                    crate::bytecode::ImmediateLayout::construct_immediate(1, true, true),
                );
                self.after_super_call(result);
                return result;
            }
            let (callee, this) = self.callee(&value.callee);
            let result = self.spread_call(callee, this, &value.arguments);
            return result;
        }
        let (callee, this) = self.callee(&value.callee);
        let (base, count) = self.arguments(&value.arguments);
        let dst = self.reg();
        let direct_eval = self.is_direct_eval_reference(&value.callee);
        if direct_eval
            && self.parameter_context
            && value.arguments.first().is_some_and(|argument| {
                matches!(argument, Argument::StringLiteral(literal) if literal.value.contains("var arguments"))
            })
        {
            self.parameter_eval_arguments_error = true;
        }
        self.dynamic_eval |= direct_eval;
        let this = if direct_eval {
            self.this_override.unwrap_or(this)
        } else {
            this
        };
        if matches!(&value.callee, Expression::Super(_)) {
            self.emit(
                Op::Construct,
                dst,
                callee,
                base,
                crate::bytecode::ImmediateLayout::construct_immediate(count, true, false),
            );
            self.after_super_call(dst);
        } else {
            self.emit(
                Op::Call,
                dst,
                callee,
                this,
                crate::bytecode::ImmediateLayout::call_immediate(
                    base,
                    count,
                    direct_eval,
                    direct_eval && self.parameter_context,
                ),
            );
        }
        dst
    }

    fn is_direct_eval_reference(&mut self, callee: &Expression<'_>) -> bool {
        matches!(callee, Expression::Identifier(identifier)
            if identifier.name == "eval"
                && !self.local_slots.contains_key(&self.owner.atom("eval"))
                && !self.scopes.iter().any(|scope| scope.contains_key(&self.owner.atom("eval"))))
    }

    fn direct_eval_spread_call(&mut self, value: &CallExpression<'_>) -> Register {
        let (callee, this) = self.callee(&value.callee);
        let expanded_arguments = self.spread_arguments(&value.arguments);
        let base = self.next_reg;
        let argument_array = self.reg();
        self.emit(Op::Move, argument_array, expanded_arguments, 0, 0);
        let result = self.reg();
        let parameter_eval = self.parameter_context;
        self.dynamic_eval = true;
        self.emit(
            Op::CallDirectEvalArray,
            result,
            callee,
            this,
            crate::bytecode::ImmediateLayout::call_immediate(base, 1, true, parameter_eval),
        );
        result
    }

    fn after_super_call(&mut self, result: Register) {
        self.emit(Op::SuperCallCheck, 0, 0, 0, 0);
        if self.defer_instance_fields {
            self.emit(Op::InitializeThis, result, 0, 0, 0);
            let edge = self.emit(Op::Jump, 0, 0, 0, 0);
            self.deferred_instance_field_edges
                .push((edge, self.code.len() as u32));
        } else if self.super_call_binds_this {
            self.emit(Op::InitializeThis, result, 0, 0, 0);
        }
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
                crate::bytecode::ImmediateLayout::call_immediate(base, 2, false, false),
            );
            return dst;
        }
        let (base, count) = self.arguments(&value.arguments);
        let dst = self.reg();
        self.emit(Op::Construct, dst, callee, base, u32::from(count));
        dst
    }

    pub(crate) fn callee(&mut self, value: &Expression<'_>) -> (Register, Register) {
        match value {
            Expression::Super(_) => {
                let callee = self.load_name("\0rqj:super");
                let this = self.literal(Constant::Undefined);
                (callee, this)
            }
            Expression::StaticMemberExpression(item) => {
                if matches!(&item.object, Expression::Super(_)) {
                    let base = self.expression(&item.object);
                    let target = if self.super_static || self.super_home {
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
                    let this = self.load_this_value();
                    return (callee, this);
                }
                let this = self.expression(&item.object);
                let dst = self.reg();
                let atom = self.owner.atom(item.property.name.as_str());
                let cache = self.owner.cache_site();
                self.emit(Op::GetField, dst, FieldBase::register(this).0, cache, atom);
                (dst, this)
            }
            Expression::PrivateFieldExpression(item) => {
                let this = self.expression(&item.object);
                let atom = self.owner.private_name_atom(item.field.span);
                let dst = self.reg();
                let cache = self.owner.cache_site();
                self.emit(Op::GetField, dst, FieldBase::register(this).0, cache, atom);
                (dst, this)
            }
            Expression::ChainExpression(item) => match &item.expression {
                ChainElement::StaticMemberExpression(member) => {
                    let this = self.expression(&member.object);
                    let callee = if member.optional {
                        let (dst, end, jump) = self.emit_optional_prefix(this);
                        let atom = self.owner.atom(member.property.name.as_str());
                        let cache = self.owner.cache_site();
                        self.patch_instruction(jump, self.code.len() as u32);
                        self.emit(Op::GetField, dst, FieldBase::register(this).0, cache, atom);
                        if let Some(end) = end {
                            self.patch(end);
                        }
                        dst
                    } else {
                        let dst = self.reg();
                        let atom = self.owner.atom(member.property.name.as_str());
                        let cache = self.owner.cache_site();
                        self.emit(Op::GetField, dst, FieldBase::register(this).0, cache, atom);
                        dst
                    };
                    (callee, this)
                }
                ChainElement::ComputedMemberExpression(member) => {
                    let this = self.expression(&member.object);
                    let callee = if member.optional {
                        let (dst, end, jump) = self.emit_optional_prefix(this);
                        self.patch_instruction(jump, self.code.len() as u32);
                        let key = self.expression_outside_optional_chain(&member.expression);
                        self.emit(Op::GetIndex, dst, this, key, 0);
                        if let Some(end) = end {
                            self.patch(end);
                        }
                        dst
                    } else {
                        let key = self.expression_outside_optional_chain(&member.expression);
                        let dst = self.reg();
                        self.emit(Op::GetIndex, dst, this, key, 0);
                        dst
                    };
                    (callee, this)
                }
                _ => {
                    let callee = self.expression(value);
                    let this = self.literal(Constant::Undefined);
                    (callee, this)
                }
            },
            Expression::ParenthesizedExpression(item) => self.callee(&item.expression),
            Expression::ComputedMemberExpression(item) => {
                let this = if matches!(&item.object, Expression::Super(_))
                    && !self.super_static
                    && !self.super_home
                {
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
                    let receiver = self.load_this_value();
                    (dst, receiver)
                } else {
                    (dst, this)
                }
            }
            Expression::Identifier(identifier) if self.with_depth != 0 => {
                let callee = self.expression(value);
                let this = self.reg();
                let atom = self.owner.atom(identifier.name.as_str());
                let cache = self.owner.cache_site();
                self.emit(Op::ResolveNameThis, this, 0, cache, atom);
                (callee, this)
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
        self.emit(
            Op::Call,
            result,
            apply,
            callee,
            crate::bytecode::ImmediateLayout::call_immediate(base, 2, false, false),
        );
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
                    let expanded = self.reg();
                    self.emit(Op::SpreadToArray, expanded, value, 0, 0);
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
                crate::bytecode::ImmediateLayout::call_immediate(base, count, false, false),
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
