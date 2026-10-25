use super::array_literal::AccumulatedElement;
use super::*;

impl FunctionCompiler<'_, '_> {
    pub(super) fn with_source_offset<T>(&mut self, offset: u32, emit: impl FnOnce(&mut Self) -> T) -> T {
        let position = self.owner.source_position(offset);
        let previous = self.current_source_position.replace((position.line, position.column));
        let result = emit(self);
        self.current_source_position = previous;
        result
    }

    pub(super) fn static_access_source_offset(&self, member: &StaticMemberExpression<'_>) -> u32 {
        if member.optional {
            self.owner.source_delimiter(member.object.span().end, member.span.end, '?')
        } else { member.property.span.start }
    }

    pub(super) fn computed_access_source_offset(&self, member: &ComputedMemberExpression<'_>) -> u32 {
        self.owner.source_delimiter(member.object.span().end, member.span.end, '[')
    }

    pub(super) fn call_source_offset(&self, call: &CallExpression<'_>) -> u32 {
        if !call.optional {
            match &call.callee {
                Expression::Identifier(_) | Expression::Super(_) => return call.callee.span().start,
                Expression::StaticMemberExpression(member) => return member.property.span.start,
                _ => {}
            }
        }
        let start = call.type_arguments.as_ref().map_or(call.callee.span().end, |arguments| arguments.span.end);
        self.owner.source_delimiter(start, call.span.end, '(')
    }

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

    /// Whether this function binds `atom` in its own scope or an active block, so a reference
    /// to it does not reach an outer scope.
    pub(in crate::compile) fn binds_name(&self, atom: Atom) -> bool {
        self.binds_own_name(atom) || self.scopes.contains(atom)
    }

    /// Whether this function itself binds `atom`, apart from the scopes it is nested in.
    pub(in crate::compile) fn binds_own_name(&self, atom: Atom) -> bool {
        self.function_scope.contains(&atom) || self.active_lexical_binding(atom).is_some()
    }

    pub(super) fn unary(&mut self, value: &UnaryExpression<'_>) -> Register {
        if value.operator == UnaryOperator::Delete {
            return self.delete_expression(&value.argument);
        }
        if let Some(constant) = binding_time::unary(value).static_value() {
            return self.literal(constant);
        }
        let input = if value.operator == UnaryOperator::Typeof {
            self.typeof_operand(&value.argument)
        } else {
            self.expression(&value.argument)
        };
        let dst = self.reg();
        self.emit(Op::Unary, dst, input, 0, value.operator as u32);
        dst
    }

    /// The value `typeof` examines: an unresolvable name reads as undefined rather than
    /// throwing (ECMA-262 13.5.3.1 step 2), which `LoadNameTypeof` does.
    pub(super) fn typeof_operand(&mut self, argument: &Expression<'_>) -> Register {
        let mut operand = argument;
        while let Expression::ParenthesizedExpression(parenthesized) = operand {
            operand = &parenthesized.expression;
        }
        if let Expression::Identifier(identifier) = operand {
            let atom = self.owner.atom(identifier.name.as_str());
            if !self.binds_name(atom) {
                let input = self.reg();
                let cache = self.owner.cache_site();
                self.emit(Op::LoadNameTypeof, input, 0, cache, atom);
                return input;
            }
        }
        self.expression(operand)
    }

    /// `typeof x === "name"` and its `!==`, `==` and `!=` forms, either way round, for a name
    /// some value's `typeof` yields: the boolean of a `UNARY_TYPEOF_IS` test of `x`. Both
    /// operands are strings, so loose and strict equality agree, and the literal side has
    /// no effects to order. `None` for any other comparison.
    pub(super) fn typeof_test(&mut self, value: &BinaryExpression<'_>) -> Option<Register> {
        let negated = match value.operator {
            BinaryOperator::StrictEquality | BinaryOperator::Equality => false,
            BinaryOperator::StrictInequality | BinaryOperator::Inequality => true,
            _ => return None,
        };
        fn unparenthesized<'e, 'a>(mut expression: &'e Expression<'a>) -> &'e Expression<'a> {
            while let Expression::ParenthesizedExpression(parenthesized) = expression {
                expression = &parenthesized.expression;
            }
            expression
        }
        // `typeof a === typeof b`: each name is one kind's (`TYPEOF_NAMES`), so the names are
        // equal exactly when the kinds' indices are, compared as integers.
        if let (Expression::UnaryExpression(left), Expression::UnaryExpression(right)) =
            (unparenthesized(&value.left), unparenthesized(&value.right))
            && left.operator == UnaryOperator::Typeof
            && right.operator == UnaryOperator::Typeof
        {
            let left = self.typeof_index(&left.argument);
            let right = self.typeof_index(&right.argument);
            let operator = if negated { BinaryOperator::StrictInequality } else { BinaryOperator::StrictEquality };
            return Some(self.emit_binary(operator as u32, Operand::register(left), Operand::register(right)));
        }
        let (typeof_argument, operator) = match (unparenthesized(&value.left), unparenthesized(&value.right)) {
            (Expression::UnaryExpression(unary), Expression::StringLiteral(name))
            | (Expression::StringLiteral(name), Expression::UnaryExpression(unary))
                if unary.operator == UnaryOperator::Typeof =>
            {
                (&unary.argument, crate::bytecode::typeof_test_operator(name.value.as_str(), negated)?)
            }
            _ => return None,
        };
        let input = self.typeof_operand(typeof_argument);
        let dst = self.reg();
        self.emit(Op::Unary, dst, input, 0, operator);
        Some(dst)
    }

    /// The index in `TYPEOF_NAMES` of `typeof argument`'s name (`UNARY_TYPEOF_INDEX`).
    pub(super) fn typeof_index(&mut self, argument: &Expression<'_>) -> Register {
        let input = self.typeof_operand(argument);
        let index = self.reg();
        self.emit(Op::Unary, index, input, 0, crate::bytecode::UNARY_TYPEOF_INDEX);
        index
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
                let key = self.literal(Constant::String(member.property.name.as_str().into()));
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

    pub(super) fn logical(&mut self, value: &LogicalExpression<'_>, debug_name: Option<Atom>) -> Register {
        if value.operator.is_coalesce() {
            return self.coalesce(value, debug_name);
        }
        let left = self.expression_with_debug_name(&value.left, debug_name);
        let dst = self.reg();
        self.emit(Op::Move, dst, left, 0, 0);
        let jump = self.emit(Op::JumpFalse, left, 0, 0, 0);
        if value.operator.is_or() {
            let skip = self.emit(Op::Jump, 0, 0, 0, 0);
            self.patch(jump);
            let right = self.expression_with_debug_name(&value.right, debug_name);
            self.emit(Op::Move, dst, right, 0, 0);
            self.patch(skip);
        } else {
            let right = self.expression_with_debug_name(&value.right, debug_name);
            self.emit(Op::Move, dst, right, 0, 0);
            self.patch(jump);
        }
        dst
    }

    pub(super) fn conditional(&mut self, value: &ConditionalExpression<'_>, debug_name: Option<Atom>) -> Register {
        let dst = self.reg();
        let test = self.expression(&value.test);
        let other = self.emit(Op::JumpFalse, test, 0, 0, 0);
        let yes = self.expression_with_debug_name(&value.consequent, debug_name);
        self.emit(Op::Move, dst, yes, 0, 0);
        let end = self.emit(Op::Jump, 0, 0, 0, 0);
        self.patch(other);
        let no = self.expression_with_debug_name(&value.alternate, debug_name);
        self.emit(Op::Move, dst, no, 0, 0);
        self.patch(end);
        dst
    }

    pub(super) fn call(&mut self, value: &CallExpression<'_>) -> Register {
        self.owner.note_invoked_callee(&value.callee);
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
            let op = if Self::forwards_arguments(value) {
                Op::CallForwardingArguments
            } else {
                Op::Call
            };
            let pc = self.emit(
                op,
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

    /// `f.apply(receiver, arguments)`: the call passes the activation's `arguments` binding
    /// on, which the activation may then never materialize (`CallPlan::forwards_arguments`).
    fn forwards_arguments(value: &CallExpression<'_>) -> bool {
        matches!(&value.callee, Expression::StaticMemberExpression(member)
            if !member.optional && member.property.name == "apply")
            && matches!(
                value.arguments.as_slice(),
                [_, Argument::Identifier(arguments)] if arguments.name == "arguments"
            )
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
            Op::CallArray,
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
        let mut private_labels = FxHashSet::default();
        for scope in self.lexical_scopes.iter().rev() {
            for atom in scope.bindings.keys() {
                if scope.kinds.get(atom) == Some(&LexicalBindingKind::PrivateName)
                    && let Some(label) = crate::bytecode::private_identity_label(&self.owner.atoms[*atom as usize])
                    && !private_labels.insert(label)
                { continue; }
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

    /// The bindings a dynamic name lookup in this function resolves statically: its function-name
    /// bindings, then the captured bindings its own declarations do not hide.
    pub(crate) fn name_bindings(&mut self) -> crate::bytecode::NameBindings {
        let mut direct = Vec::new();
        for scope in self.lexical_scopes.iter().rev() {
            for (atom, target) in &scope.bindings {
                if scope.kinds.get(atom) == Some(&LexicalBindingKind::FunctionName)
                    && !direct
                        .iter()
                        .any(|binding: &crate::bytecode::EvalBinding| binding.atom == *atom)
                    && let Some(slot) = self.local_slots.get(target)
                {
                    direct.push(crate::bytecode::EvalBinding {
                        atom: *atom,
                        location: crate::bytecode::EvalBindingLocation::Local(*slot),
                        with_depth: scope.with_depth.saturating_sub(self.inherited_with_depth),
                        kind: LexicalBindingKind::FunctionName,
                        declaration: crate::bytecode::EvalBindingDeclaration::Lexical,
                    });
                }
            }
        }
        // Eval may mention private names only in its string. Its class home
        // still needs the same lexical capture that compiled super access uses.
        if self.dynamic_eval && let Some(atom) = self.super_home_atom {
            let location = self.local_slots.get(&atom).copied()
                .map(crate::bytecode::EvalBindingLocation::Local)
                .or_else(|| {
                    let (depth, slot) = self.scopes.find(atom)?;
                    let depth = u16::try_from(depth).ok()?;
                    Some(crate::bytecode::EvalBindingLocation::Capture { depth, slot })
                });
            if let Some(location) = location
                && !direct.iter().any(|binding| binding.atom == atom)
            {
                direct.push(crate::bytecode::EvalBinding {
                    atom, location, with_depth: 0,
                    kind: LexicalBindingKind::Mutable,
                    declaration: crate::bytecode::EvalBindingDeclaration::Variable,
                });
            }
        }
        direct.sort_unstable_by_key(|binding| binding.atom);
        // The captured and hidden names are consulted only by lookups through dynamic name
        // resolution, which the whole program decides; `Compiler::finish` resolves them for
        // the functions that may see such a lookup (`capture_layout::dynamic_name_scopes`).
        self.owner.deferred_name_bindings.insert(
            self.function_id,
            super::super::DeferredNameBindings {
                names_private_members: self.owner.private_name_references
                    != self.private_name_references,
                direct: direct.clone(),
                scopes: self.scopes.clone(),
                function_scope: self.function_scope.iter().copied().collect(),
            },
        );
        crate::bytecode::NameBindings::new(direct, Vec::new(), None)
    }

    fn after_super_call(&mut self, result: Register) {
        self.emit(Op::SuperCallCheck, 0, 0, 0, 0);
        if self.super_call_binds_this {
            self.emit(Op::InitializeThis, result, 0, 0, 0);
        }
    }

    pub(super) fn super_constructor(&mut self) -> Register {
        let active = self.load_name("\0quench:super");
        let get_prototype = self.load_name(crate::bytecode::SUPER_BASE_BINDING);
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
            let dst = self.reg();
            self.emit(
                Op::Construct,
                dst,
                callee,
                arguments,
                crate::bytecode::ImmediateLayout::construct_immediate(1, false, true),
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
        let source = self.static_access_source_offset(member);
        self.with_source_offset(source, |compiler| compiler.emit(
            Op::GetField, callee, FieldBase::register(receiver).0, cache, atom,
        ));
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
        let source = self.computed_access_source_offset(member);
        self.with_source_offset(source, |compiler| compiler.emit(Op::GetIndex, callee, receiver, key, 0));
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
        self.with_source_offset(member.field.span.start, |compiler| compiler.emit(
            Op::GetField, callee, FieldBase::register(receiver).0, cache, atom,
        ));
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
                let atom = self.owner.atom(identifier.name.as_str());
                if let Some(callee) = self.static_atom_reference(atom) {
                    return (callee, self.undefined_register());
                }
                // A by-name callee may resolve through a with object, which becomes `this`.
                let this = self.literal(Constant::Undefined);
                let callee = self.load_atom_reference(atom, Some(this));
                (callee, this)
            }
            _ => {
                let callee = self.expression(value);
                (callee, self.undefined_register())
            }
        }
    }

    pub(super) fn arguments(&mut self, values: &[Argument<'_>]) -> (Register, u16) {
        // Evaluate first, then reserve the contiguous ABI argument window.
        // Reserving targets before expression lowering lets expression
        // temporaries occupy later argument registers. A primitive literal has no
        // evaluation effect, so it loads straight into its window slot instead.
        let arguments = values
            .iter()
            .filter_map(|argument| {
                let Some(expression) = argument.as_expression() else {
                    self.owner
                        .reject(argument.span(), "spread arguments unsupported");
                    return None;
                };
                Some(match Self::primitive_literal(expression) {
                    Some(constant) => Err(constant),
                    None => Ok(self.expression(expression)),
                })
            })
            .collect::<Vec<_>>();
        let base = self.next_reg;
        let targets: Vec<_> = (0..arguments.len()).map(|_| self.reg()).collect();
        let count = targets.len() as u16;
        for (argument, target) in arguments.into_iter().zip(targets) {
            match argument {
                Ok(value) => {
                    self.emit(Op::Move, target, value, 0, 0);
                }
                Err(constant) => {
                    let id = self.owner.constant(constant);
                    self.emit(Op::LoadConst, target, 0, 0, id);
                }
            }
        }
        (base, count)
    }

    /// The constant of a literal whose load has no observable effect.
    fn primitive_literal(expression: &Expression<'_>) -> Option<Constant> {
        match expression {
            Expression::NumericLiteral(value) => Some(Constant::Number(value.value)),
            Expression::BooleanLiteral(value) => Some(Constant::Boolean(value.value)),
            Expression::NullLiteral(_) => Some(Constant::Null),
            _ => None,
        }
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
        // `f(...iterable)`: the call iterates its one spread itself (`Op::CallSpread`), so
        // an intact array passes its elements without an argument array.
        let (op, arguments) = match values {
            [Argument::SpreadElement(spread)] => (Op::CallSpread, self.expression(&spread.argument)),
            _ => (Op::CallArray, self.spread_arguments(values)),
        };
        let base = self.next_reg;
        let argument_array = self.reg();
        self.emit(Op::Move, argument_array, arguments, 0, 0);
        let result = self.reg();
        self.emit(
            op,
            result,
            callee,
            this,
            crate::bytecode::ImmediateLayout::call_immediate(base, 1, false, false),
        );
        result
    }

    pub(super) fn spread_arguments(&mut self, values: &[Argument<'_>]) -> Register {
        let elements = values
            .iter()
            .map(|argument| match argument {
                Argument::SpreadElement(spread) => AccumulatedElement::Spread(&spread.argument),
                argument => {
                    AccumulatedElement::Value(argument.as_expression().expect("expression argument"))
                }
            })
            .collect::<Vec<_>>();
        self.accumulated_array(&elements)
    }
}
