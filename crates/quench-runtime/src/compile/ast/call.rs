use super::*;

impl FunctionCompiler<'_, '_> {
    pub(super) fn throw_invalid_call_assignment(&mut self, call: &Expression<'_>) -> Register {
        if self.strict {
            self.owner
                .reject(call.span(), "SyntaxError: invalid call assignment target");
            return self.literal(Constant::Undefined);
        }
        self.expression(call);
        let constructor = self.load_name("ReferenceError");
        let error = self.reg();
        self.emit(Op::Construct, error, constructor, constructor, 0);
        self.emit(Op::Throw, error, 0, 0, 0);
        self.literal(Constant::Undefined)
    }

    pub(super) fn unary(&mut self, value: &UnaryExpression<'_>) -> Register {
        if value.operator == UnaryOperator::Delete {
            return self.delete_expression(&value.argument);
        }
        let input = if value.operator == UnaryOperator::Typeof {
            let mut operand = &value.argument;
            while let Expression::ParenthesizedExpression(parenthesized) = operand {
                operand = &parenthesized.expression;
            }
            if let Expression::Identifier(identifier) = operand {
                let atom = self.owner.atom(identifier.name.as_str());
                let bound = self.function_scope.contains(&atom)
                    || self.active_lexical_binding(atom).is_some()
                    || self.scopes.iter().any(|scope| scope.contains_key(&atom));
                if bound {
                    self.expression(operand)
                } else {
                    let input = self.reg();
                    let cache = self.owner.cache_site();
                    self.emit(Op::LoadNameTypeof, input, 0, cache, atom);
                    input
                }
            } else {
                self.expression(operand)
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
            Expression::ChainExpression(chain) => return self.delete_chain(&chain.expression),
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
            if super::super::early::is_direct_eval_call(value) {
                return self.direct_eval_spread_call(value);
            }
            if matches!(&value.callee, Expression::Super(_)) {
                let superclass = self.super_constructor();
                let arguments = self.spread_arguments(&value.arguments);
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
        let direct_eval = super::super::early::is_direct_eval_call(value);
        if direct_eval
            && self.parameter_context
            && value.arguments.first().is_some_and(|argument| {
                matches!(argument, Argument::StringLiteral(literal) if literal.value.contains("var arguments"))
            })
        {
            self.parameter_eval_arguments_error = true;
        }
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
            let pc = self.emit(
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
            if direct_eval {
                self.record_lexical_binding_site(pc);
            }
        }
        dst
    }

    fn direct_eval_spread_call(&mut self, value: &CallExpression<'_>) -> Register {
        let (callee, this) = self.callee(&value.callee);
        let expanded_arguments = self.spread_arguments(&value.arguments);
        let base = self.next_reg;
        let argument_array = self.reg();
        self.emit(Op::Move, argument_array, expanded_arguments, 0, 0);
        let result = self.reg();
        let parameter_eval = self.parameter_context;
        let pc = self.emit(
            Op::CallDirectEvalArray,
            result,
            callee,
            this,
            crate::bytecode::ImmediateLayout::call_immediate(base, 1, true, parameter_eval),
        );
        self.record_lexical_binding_site(pc);
        result
    }

    fn lexical_binding_projection(
        &self,
        scope: &LexicalScope,
        atom: Atom,
    ) -> Option<crate::bytecode::EvalBinding> {
        let target = scope.bindings.get(&atom)?;
        let slot = *self.local_slots.get(target)?;
        Some(crate::bytecode::EvalBinding {
            atom,
            location: crate::bytecode::EvalBindingLocation::Local(slot),
            with_depth: scope.with_depth.saturating_sub(self.inherited_with_depth),
            kind: scope
                .kinds
                .get(&atom)
                .copied()
                .unwrap_or(LexicalBindingKind::Mutable),
            declaration: if scope.catch_parameter {
                crate::bytecode::EvalBindingDeclaration::CatchParameter
            } else {
                crate::bytecode::EvalBindingDeclaration::Lexical
            },
        })
    }

    pub(super) fn record_name_binding_site(&mut self, pc: usize, atom: Atom) {
        if let Some(binding) = self
            .lexical_scopes
            .iter()
            .rev()
            .find_map(|scope| self.lexical_binding_projection(scope, atom))
        {
            self.binding_sites.push(crate::bytecode::BindingSite {
                resume_pc: (pc + 1) as u32,
                bindings: vec![binding],
            });
        }
    }

    pub(super) fn record_lexical_binding_site(&mut self, pc: usize) {
        let mut visible = rustc_hash::FxHashSet::default();
        let mut bindings = Vec::new();
        for scope in self.lexical_scopes.iter().rev() {
            for atom in scope.bindings.keys() {
                if scope.kinds.get(atom) != Some(&LexicalBindingKind::FunctionName)
                    && visible.insert(*atom)
                    && let Some(binding) = self.lexical_binding_projection(scope, *atom)
                {
                    bindings.push(binding);
                }
            }
        }
        bindings.sort_unstable_by_key(|binding| binding.atom);
        self.binding_sites.push(crate::bytecode::BindingSite {
            resume_pc: (pc + 1) as u32,
            bindings,
        });
    }

    pub(crate) fn name_bindings(&mut self) -> Vec<crate::bytecode::EvalBinding> {
        let mut visible = rustc_hash::FxHashSet::default();
        let mut bindings = Vec::new();
        for scope in self.lexical_scopes.iter().rev() {
            for (atom, target) in &scope.bindings {
                if scope.kinds.get(atom) == Some(&LexicalBindingKind::FunctionName)
                    && visible.insert(*atom)
                    && let Some(slot) = self.local_slots.get(target)
                {
                    bindings.push(crate::bytecode::EvalBinding {
                        atom: *atom,
                        location: crate::bytecode::EvalBindingLocation::Local(*slot),
                        with_depth: scope.with_depth.saturating_sub(self.inherited_with_depth),
                        kind: LexicalBindingKind::FunctionName,
                        declaration: crate::bytecode::EvalBindingDeclaration::Lexical,
                    });
                }
            }
        }
        visible.extend(self.function_scope.iter().copied());
        for (depth, scope) in self.scopes.iter().enumerate() {
            let Ok(depth) = u16::try_from(depth) else {
                self.owner.reject(
                    Span::default(),
                    "eval capture depth exceeds residual encoding",
                );
                break;
            };
            for (marker, slot) in scope.iter() {
                let text = self.owner.atoms[*marker as usize].clone();
                let Some((kind, name)) = LexicalBindingKind::ALL.into_iter().find_map(|kind| {
                    text.strip_prefix(kind.capture_prefix())
                        .map(|name| (kind, name))
                }) else {
                    continue;
                };
                let atom = self.owner.atom(name);
                if !visible.insert(atom) {
                    continue;
                }
                let catch_marker = format!("\0rqj:catch-capture:{name}");
                bindings.push(crate::bytecode::EvalBinding {
                    atom,
                    location: crate::bytecode::EvalBindingLocation::Capture { depth, slot: *slot },
                    with_depth: 0,
                    kind,
                    declaration: if self
                        .owner
                        .atom_index
                        .get(catch_marker.as_str())
                        .is_some_and(|marker| scope.contains_key(marker))
                    {
                        crate::bytecode::EvalBindingDeclaration::CatchParameter
                    } else {
                        crate::bytecode::EvalBindingDeclaration::Lexical
                    },
                });
            }
            // Lexical markers own mutability and declaration conflicts. Add
            // ordinary captures only after those source names have been resolved.
            for (atom, slot) in scope.iter() {
                if self.owner.atoms[*atom as usize].contains('\0') || !visible.insert(*atom) {
                    continue;
                }
                bindings.push(crate::bytecode::EvalBinding {
                    atom: *atom,
                    location: crate::bytecode::EvalBindingLocation::Capture { depth, slot: *slot },
                    with_depth: 0,
                    kind: LexicalBindingKind::Mutable,
                    declaration: crate::bytecode::EvalBindingDeclaration::Variable,
                });
            }
        }
        bindings.sort_unstable_by_key(|binding| binding.atom);
        bindings
    }

    fn after_super_call(&mut self, result: Register) {
        self.emit(Op::SuperCallCheck, 0, 0, 0, 0);
        if self.super_call_binds_this {
            self.emit(Op::InitializeThis, result, 0, 0, 0);
        }
    }

    pub(super) fn super_constructor(&mut self) -> Register {
        let active = self.load_name("\0rqj:super");
        let get_prototype = self.load_name("\0rqj:super-base");
        let receiver = self.literal(Constant::Undefined);
        let argument = self.reg();
        self.emit(Op::Move, argument, active, 0, 0);
        let result = self.reg();
        self.emit(
            Op::Call,
            result,
            get_prototype,
            receiver,
            crate::bytecode::ImmediateLayout::call_immediate(argument, 1, false, false),
        );
        result
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

    pub(super) fn static_member_callee(
        &mut self,
        member: &StaticMemberExpression<'_>,
    ) -> (Register, Register) {
        if matches!(&member.object, Expression::Super(_)) {
            let callee = self.static_member(member);
            return (callee, self.load_this_value());
        }
        let receiver = self.expression(&member.object);
        let (callee, end) = if member.optional {
            let (callee, end, jump) = self.emit_optional_prefix(receiver);
            self.patch_instruction(jump, self.code.len() as u32);
            (callee, end)
        } else {
            (self.reg(), None)
        };
        let atom = self.owner.atom(member.property.name.as_str());
        let cache = self.owner.cache_site();
        self.emit(
            Op::GetField,
            callee,
            FieldBase::register(receiver).0,
            cache,
            atom,
        );
        if let Some(end) = end {
            self.patch(end);
        }
        (callee, receiver)
    }

    pub(super) fn computed_member_callee(
        &mut self,
        member: &ComputedMemberExpression<'_>,
    ) -> (Register, Register) {
        if matches!(&member.object, Expression::Super(_)) {
            let callee = self.computed_member(member);
            return (callee, self.load_this_value());
        }
        let receiver = self.expression(&member.object);
        let (callee, end) = if member.optional {
            let (callee, end, jump) = self.emit_optional_prefix(receiver);
            self.patch_instruction(jump, self.code.len() as u32);
            (callee, end)
        } else {
            (self.reg(), None)
        };
        let key = self.expression_outside_optional_chain(&member.expression);
        self.emit(Op::GetIndex, callee, receiver, key, 0);
        if let Some(end) = end {
            self.patch(end);
        }
        (callee, receiver)
    }

    pub(super) fn private_field_callee(
        &mut self,
        member: &PrivateFieldExpression<'_>,
    ) -> (Register, Register) {
        let receiver = self.expression(&member.object);
        let (callee, end) = if member.optional {
            let (callee, end, jump) = self.emit_optional_prefix(receiver);
            self.patch_instruction(jump, self.code.len() as u32);
            (callee, end)
        } else {
            (self.reg(), None)
        };
        let atom = self.owner.private_name_atom(member.field.span);
        let cache = self.owner.cache_site();
        self.emit(
            Op::GetField,
            callee,
            FieldBase::register(receiver).0,
            cache,
            atom,
        );
        if let Some(end) = end {
            self.patch(end);
        }
        (callee, receiver)
    }

    pub(crate) fn callee(&mut self, value: &Expression<'_>) -> (Register, Register) {
        match value {
            Expression::Super(_) => {
                let callee = self.super_constructor();
                let this = self.literal(Constant::Undefined);
                (callee, this)
            }
            Expression::StaticMemberExpression(item) => self.static_member_callee(item),
            Expression::PrivateFieldExpression(item) => self.private_field_callee(item),
            Expression::ChainExpression(item) => self.chain_callee(&item.expression),
            Expression::ParenthesizedExpression(item) => self.callee(&item.expression),
            Expression::ComputedMemberExpression(item) => self.computed_member_callee(item),
            Expression::Identifier(identifier) => {
                let this = self.literal(Constant::Undefined);
                let atom = self.owner.atom(identifier.name.as_str());
                let callee = self.load_atom_reference(atom, Some(this));
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
}
