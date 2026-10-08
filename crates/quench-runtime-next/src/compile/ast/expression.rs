use super::destructure::AssignmentReference;
use super::*;
impl FunctionCompiler<'_, '_> {
    pub(crate) fn expression(&mut self, expression: &Expression<'_>) -> Register {
        let position = self.owner.source_position(expression.span().start);
        let previous = self
            .current_source_position
            .replace((position.line, position.column));
        let result = self.expression_at_current_position(expression);
        self.current_source_position = previous;
        result
    }

    fn expression_at_current_position(&mut self, expression: &Expression<'_>) -> Register {
        match expression {
            Expression::NumericLiteral(value) => self.literal(Constant::Number(value.value)),
            Expression::StringLiteral(value) => self.string_literal(value),
            Expression::BigIntLiteral(value) => self.bigint_literal(value),
            Expression::RegExpLiteral(value) => self.regexp_literal(value),
            Expression::TemplateLiteral(value) => self.template_literal(value),
            Expression::TaggedTemplateExpression(value) => self.tagged_template(value),
            Expression::BooleanLiteral(value) => self.literal(Constant::Boolean(value.value)),
            Expression::NullLiteral(_) => self.literal(Constant::Null),
            Expression::Identifier(value) => self.load_name(value.name.as_str()),
            Expression::ThisExpression(_) => self.load_this_value(),
            Expression::NewTarget(_) => self.load_name("\0quench:new-target"),
            Expression::ImportMeta(_) => {
                if !self.owner.module_goal {
                    self.owner.reject(
                        expression.span(),
                        "SyntaxError: import.meta is only valid in modules",
                    );
                }
                let result = self.reg();
                self.emit(Op::LoadImportMeta, result, 0, 0, 0);
                result
            }
            Expression::Super(_) => self.super_base(),
            Expression::FunctionExpression(value) => self.function_expression(value),
            Expression::ArrowFunctionExpression(value) => self.arrow_function_expression(value),
            Expression::ClassExpression(value) => self.class_expression(value),
            Expression::ArrayExpression(value) => self.array_expression(value),
            Expression::ObjectExpression(value) => self.object_expression(value),
            Expression::StaticMemberExpression(value) => self.static_member(value),
            Expression::PrivateFieldExpression(value) => {
                let name = self.owner.private_name_text(value.field.span);
                self.static_get(&value.object, &name)
            }
            Expression::ComputedMemberExpression(value) => self.computed_member(value),
            Expression::ChainExpression(value) => self.chain_expression(&value.expression),
            Expression::AssignmentExpression(value) => self.assignment(value),
            Expression::UpdateExpression(value) => self.update(value),
            Expression::BinaryExpression(value) => self.binary(value),
            Expression::PrivateInExpression(value) => {
                let atom = self.owner.private_name_atom(value.left.span);
                let object = self.expression(&value.right);
                let result = self.reg();
                self.emit(Op::PrivateIn, result, object, 0, atom);
                result
            }
            Expression::UnaryExpression(value) => self.unary(value),
            Expression::LogicalExpression(value) => self.logical(value),
            Expression::ConditionalExpression(value) => self.conditional(value),
            Expression::CallExpression(value) if value.optional => self.optional_call(value),
            Expression::CallExpression(value) => self.call(value),
            Expression::ImportExpression(value) => {
                let specifier = self.expression(&value.source);
                let options = if let Some(options) = value.options.as_ref() {
                    self.expression(options)
                } else {
                    self.literal(Constant::Undefined)
                };
                let phase = match value.phase {
                    Some(oxc_ast::ast::ImportPhase::Source) => {
                        crate::bytecode::ModuleRequestPhase::Source
                    }
                    Some(oxc_ast::ast::ImportPhase::Defer) => {
                        crate::bytecode::ModuleRequestPhase::Defer
                    }
                    None => crate::bytecode::ModuleRequestPhase::Evaluation,
                };
                let phase = self.literal(Constant::Number(phase.runtime_value()));
                let callee = self.load_name("\0quench:dynamic-import");
                let this = self.literal(Constant::Undefined);
                let base = self.next_reg;
                for argument in [specifier, options, phase] {
                    let slot = self.reg();
                    self.emit(Op::Move, slot, argument, 0, 0);
                }
                let result = self.reg();
                self.emit(
                    Op::Call,
                    result,
                    callee,
                    this,
                    crate::bytecode::ImmediateLayout::call_immediate(base, 3, false, false),
                );
                result
            }
            Expression::NewExpression(value) => self.construct(value),
            Expression::SequenceExpression(value) => self.sequence_expression(value),
            Expression::ParenthesizedExpression(value) => self.expression(&value.expression),
            Expression::AwaitExpression(value) if self.async_function => {
                let source = self.expression(&value.argument);
                let destination = self.reg();
                self.emit(Op::Await, destination, source, 0, 0);
                destination
            }
            Expression::YieldExpression(value) if self.generator && value.delegate => {
                let source = if let Some(argument) = value.argument.as_ref() {
                    self.expression(argument)
                } else {
                    self.literal(Constant::Undefined)
                };
                let destination = self.reg();
                let undefined = self.literal(Constant::Undefined);
                self.emit(Op::Move, destination, undefined, 0, 0);
                let iterator = self.reg();
                self.emit(Op::Move, iterator, undefined, 0, 0);
                let awaited_result = self.reg();
                self.emit(Op::Move, awaited_result, undefined, 0, 0);
                let next_method = self.reg();
                self.emit(Op::Move, next_method, undefined, 0, 0);
                let state = crate::bytecode::ImmediateLayout::register_pair_immediate(
                    awaited_result,
                    next_method,
                );
                self.emit(Op::YieldStar, destination, source, iterator, state);
                destination
            }
            Expression::YieldExpression(value) if self.generator => {
                let source = if let Some(argument) = value.argument.as_ref() {
                    self.expression(argument)
                } else {
                    self.literal(Constant::Undefined)
                };
                let destination = self.reg();
                self.emit(Op::Yield, destination, source, 0, 0);
                destination
            }
            _ => {
                self.owner.reject(
                    expression.span(),
                    "expression is outside the supported subset",
                );
                self.literal(Constant::Undefined)
            }
        }
    }

    pub(crate) fn load_name(&mut self, name: &str) -> Register {
        let atom = self.owner.atom(name);
        self.load_atom(atom)
    }

    fn regexp_literal(&mut self, value: &oxc_ast::ast::RegExpLiteral<'_>) -> Register {
        let callee = self.load_name(crate::bytecode::INTRINSIC_REGEXP_BINDING);
        let pattern = self.literal(Constant::String(value.regex.pattern.text.to_string()));
        let mut flags = String::new();
        for (flag, bit) in [
            ('d', RegExpFlags::D),
            ('g', RegExpFlags::G),
            ('i', RegExpFlags::I),
            ('m', RegExpFlags::M),
            ('s', RegExpFlags::S),
            ('u', RegExpFlags::U),
            ('v', RegExpFlags::V),
            ('y', RegExpFlags::Y),
        ] {
            if value.regex.flags.contains(bit) {
                flags.push(flag);
            }
        }
        let flags = self.literal(Constant::String(flags));
        let base = pattern;
        let destination = self.reg();
        self.emit(Op::Construct, destination, callee, base, 2);
        let _ = flags;
        destination
    }
    fn parameter_binding_atom(&self, atom: Atom) -> Atom {
        if self.parameter_context
            && self.owner.atoms[atom as usize].as_ref() == "arguments"
            && let Some(slot) = self.parameter_arguments_slot
        {
            self.locals[usize::from(slot)]
        } else {
            atom
        }
    }

    pub(crate) fn load_atom(&mut self, atom: Atom) -> Register {
        self.load_atom_reference(atom, None)
    }

    pub(super) fn load_atom_reference(
        &mut self,
        atom: Atom,
        receiver: Option<Register>,
    ) -> Register {
        let atom = self.parameter_binding_atom(atom);
        let compiler_binding = self.is_compiler_binding(atom);
        let lexical = if self.dynamic_eval
            && self.lexical_binding_kind(atom) == LexicalBindingKind::FunctionName
        {
            None
        } else {
            self.active_lexical_binding(atom)
        };
        let atom = lexical.unwrap_or(atom);
        let dst = self.reg();
        if (self.with_depth == self.inherited_with_depth || lexical.is_some() || compiler_binding)
            && (self.function_scope.contains(&atom) || lexical.is_some() || compiler_binding)
            && let Some(slot) = self.local_slots.get(&atom).copied()
        {
            self.emit(Op::LoadLocal, dst, 0, 0, u32::from(slot));
        } else if self.with_depth == 0
            && (!self.dynamic_eval || compiler_binding)
            && let Some((depth, slot)) = self
                .scopes
                .iter()
                .enumerate()
                .find_map(|(depth, scope)| scope.get(&atom).copied().map(|slot| (depth, slot)))
        {
            self.emit(
                Op::LoadCapture,
                dst,
                0,
                0,
                crate::bytecode::ImmediateLayout::capture_immediate(depth, slot),
            );
        } else {
            let cache = self.owner.cache_site();
            match receiver {
                Some(receiver) => {
                    self.emit(Op::LoadNameCall, dst, receiver, cache, atom);
                }
                None => {
                    self.emit(Op::LoadName, dst, 0, cache, atom);
                }
            }
        }
        dst
    }

    pub(crate) fn store_atom(&mut self, atom: Atom, value: Register) {
        self.store_atom_with_initialization(atom, value, false);
    }

    pub(crate) fn initialize_atom(&mut self, atom: Atom, value: Register) {
        self.store_atom_with_initialization(atom, value, true);
    }

    pub(super) fn store_atom_with_initialization(
        &mut self,
        atom: Atom,
        value: Register,
        initializing: bool,
    ) {
        let source_atom = self.parameter_binding_atom(atom);
        let binding_kind = self.lexical_binding_kind(source_atom);
        let immutable_lexical = !initializing
            && binding_kind == LexicalBindingKind::Immutable
            && (self.with_depth == self.inherited_with_depth
                || self.active_lexical_binding(source_atom).is_some());
        let compiler_binding = self.is_compiler_binding(source_atom);
        let lexical = if self.dynamic_eval && binding_kind == LexicalBindingKind::FunctionName {
            None
        } else {
            self.active_lexical_binding(source_atom)
        };
        let atom = lexical.unwrap_or(source_atom);
        let function_name_binding = !initializing
            && ((!self.dynamic_eval
                && binding_kind == LexicalBindingKind::FunctionName
                && (self.with_depth == self.inherited_with_depth || lexical.is_some()))
                || (self.with_depth == 0
                    && !self.dynamic_eval
                    && self.has_function_name_capture(source_atom)));
        if function_name_binding {
            if self.strict {
                self.throw_immutable_binding(source_atom);
            }
            return;
        }
        if self.owner.atoms[atom as usize]
            .as_ref()
            .contains("\0quench:class-binding:")
            || immutable_lexical
            || (self.with_depth == 0
                && !self.dynamic_eval
                && !self.local_slots.contains_key(&atom)
                && self.has_immutable_capture(atom))
        {
            self.throw_immutable_binding(atom);
            return;
        }
        if (self.with_depth == self.inherited_with_depth || lexical.is_some() || compiler_binding)
            && (self.function_scope.contains(&atom) || lexical.is_some() || compiler_binding)
            && let Some(slot) = self.local_slots.get(&atom).copied()
        {
            self.emit(
                Op::StoreLocal,
                value,
                0,
                u16::from(initializing),
                u32::from(slot),
            );
            if lexical.is_none() && self.function_id == 0 && !self.owner.module_goal {
                let cache = self.owner.cache_site();
                self.emit(
                    Op::StoreName,
                    value,
                    u16::from(initializing),
                    cache,
                    source_atom,
                );
            }
        } else if (self.with_depth == 0 || initializing)
            && (!self.dynamic_eval || compiler_binding)
            && let Some((depth, slot)) = self
                .scopes
                .iter()
                .enumerate()
                .find_map(|(depth, scope)| scope.get(&atom).copied().map(|slot| (depth, slot)))
        {
            self.emit(
                Op::StoreCapture,
                value,
                0,
                0,
                crate::bytecode::ImmediateLayout::capture_immediate(depth, slot),
            );
        } else {
            let cache = self.owner.cache_site();
            self.emit(Op::StoreName, value, u16::from(initializing), cache, atom);
        }
    }

    pub(super) fn store_annex_b_outer(
        &mut self,
        atom: Atom,
        value: Register,
        declaration_start: u32,
    ) {
        if !self.annex_b_outer_binding_allowed(atom, declaration_start) {
            return;
        }
        if let Some(slot) = self.local_slots.get(&atom).copied() {
            self.emit(Op::StoreVarBinding, value, 0, 0, u32::from(slot));
        }
    }

    pub(super) fn annex_b_outer_binding_allowed(&self, atom: Atom, declaration_start: u32) -> bool {
        let parameter_binding = self
            .local_slots
            .get(&atom)
            .is_some_and(|slot| usize::from(*slot) < self.parameter_local_count);
        !parameter_binding && !self.annex_b_collisions.contains(&declaration_start)
    }

    pub(super) fn has_immutable_capture(&mut self, atom: Atom) -> bool {
        let name = self.owner.atoms[atom as usize].clone();
        let marker = self.owner.atom(&format!(
            "{}{name}",
            LexicalBindingKind::Immutable.capture_prefix()
        ));
        self.scopes.iter().any(|scope| scope.contains_key(&marker))
    }

    fn has_function_name_capture(&mut self, atom: Atom) -> bool {
        let name = self.owner.atoms[atom as usize].clone();
        let marker = self.owner.atom(&format!(
            "{}{name}",
            LexicalBindingKind::FunctionName.capture_prefix()
        ));
        self.scopes.iter().any(|scope| scope.contains_key(&marker))
    }

    fn throw_immutable_binding(&mut self, atom: Atom) {
        let _ = self.load_atom(atom);
        let constructor = self.load_name("TypeError");
        let error = self.reg();
        self.emit(Op::Construct, error, constructor, 0, 0);
        self.emit(Op::Throw, error, 0, 0, 0);
    }

    pub(super) fn function_expression(&mut self, value: &oxc_ast::ast::Function<'_>) -> Register {
        let params = Self::params(value, self.owner);
        let body = value
            .body
            .as_ref()
            .map_or(&[][..], |body| body.statements.as_slice());
        let name = value.id.as_ref().map(|id| id.name.as_str());
        let name_binding = value
            .id
            .as_ref()
            .map(|id| self.owner.atom(id.name.as_str()));
        let scopes = self.capture_scopes();
        let function = self.owner.compile_function(
            name,
            &params,
            FunctionBody::Statements(body),
            &scopes,
            Some(self.function_id),
            FunctionOptions {
                source_text: self.owner.source_text(value.span),
                defaults: Some(&value.params),
                name_binding,
                async_function: value.r#async,
                generator: value.generator,
                with_depth: self.with_depth,
                strict: self.strict
                    || value.body.as_ref().is_some_and(|body| {
                        body.directives
                            .iter()
                            .any(|directive| directive.directive == "use strict")
                    }),
                ..FunctionOptions::default()
            },
        );
        let dst = self.reg();
        self.emit(Op::MakeClosure, dst, 0, 0, function);
        dst
    }
    pub(super) fn arrow_function_expression(
        &mut self,
        value: &oxc_ast::ast::ArrowFunctionExpression<'_>,
    ) -> Register {
        let lexical_this_atom = self.this_override.map(|this| {
            let atom = self.hidden_local("\0quench:lexical-this-override");
            self.store_atom(atom, this);
            atom
        });
        let scopes = self.capture_scopes();
        let function = self.owner.compile_arrow_function(
            value,
            &scopes,
            Some(self.function_id),
            self.super_static,
            self.super_home,
            self.super_home_atom,
            self.strict,
            self.class_field_initializer,
            lexical_this_atom,
            self.super_call_binds_this,
            self.with_depth,
        );
        let dst = self.reg();
        self.emit(Op::MakeClosure, dst, 0, 0, function);
        dst
    }
    pub(super) fn static_get(&mut self, object: &Expression<'_>, key: &str) -> Register {
        if matches!(object, Expression::Super(_)) {
            let receiver = self.reg();
            self.emit(Op::LoadThis, receiver, 0, 0, 0);
        }
        if let Expression::StaticMemberExpression(inner) = object
            && matches!(&inner.object, Expression::ThisExpression(_))
            && self.this_override.is_none()
        {
            let dst = self.reg();
            let first = self.owner.atom(inner.property.name.as_str());
            let second = self.owner.atom(key);
            let site = self.owner.cache_site();
            let second_site = self.owner.cache_site();
            let access =
                self.field_site(FieldBase::THIS, (first, site), Some((second, second_site)));
            self.emit(Op::GetField, dst, FieldBase::NESTED, 0, access);
            return dst;
        }
        let dst = self.reg();
        let atom = self.owner.atom(key);
        let cache = self.owner.cache_site();
        let base = if matches!(object, Expression::ThisExpression(_)) {
            self.this_override
                .map(FieldBase::register)
                .unwrap_or(FieldBase::THIS)
        } else {
            FieldBase::register(self.expression(object))
        };
        self.emit(Op::GetField, dst, base.0, cache, atom);
        dst
    }
    pub(super) fn field_site(
        &mut self,
        base: FieldBase,
        first: (Atom, u16),
        second: Option<(Atom, u16)>,
    ) -> u32 {
        let site = self.owner.field_sites.len() as u32;
        self.owner.field_sites.push(FieldSite {
            base,
            first,
            second,
            sink: None,
        });
        site
    }

    pub(crate) fn emit_binary(&mut self, op: u32, left: Operand, right: Operand) -> Register {
        let dst = self.reg();
        self.emit(Op::Binary, dst, left.0, right.0, op);
        dst
    }

    pub(super) fn computed_get(
        &mut self,
        object: &Expression<'_>,
        key: &Expression<'_>,
    ) -> Register {
        if matches!(object, Expression::Super(_)) {
            let receiver = self.reg();
            self.emit(Op::LoadThis, receiver, 0, 0, 0);
        }
        let object =
            if matches!(object, Expression::Super(_)) && !self.super_static && !self.super_home {
                let base = self.expression(object);
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
                self.expression(object)
            };
        if let Expression::StringLiteral(value) = key
            && !matches!(
                super::super::string::constant(value),
                Constant::StringUnits(_)
            )
        {
            let (dst, atom, cache) = (
                self.reg(),
                self.owner.atom(value.value.as_str()),
                self.owner.cache_site(),
            );
            self.emit(
                Op::GetField,
                dst,
                FieldBase::register(object).0,
                cache,
                atom,
            );
            return dst;
        }
        let key = self.expression(key);
        let dst = self.reg();
        self.emit(Op::GetIndex, dst, object, key, 0);
        dst
    }

    pub(super) fn emit_set_field(
        &mut self,
        value: Register,
        object: Register,
        site: u16,
        atom: Atom,
    ) {
        let op = if self.strict {
            Op::SetFieldStrict
        } else {
            Op::SetField
        };
        self.emit(op, value, object, site, atom);
    }

    pub(super) fn emit_set_this_field(&mut self, value: Register, site: u16, atom: Atom) {
        let op = if self.strict {
            Op::SetThisFieldStrict
        } else {
            Op::SetThisField
        };
        self.emit(op, value, 0, site, atom);
    }
    pub(super) fn update(&mut self, value: &UpdateExpression<'_>) -> Register {
        let reference = self.prepare_assignment_reference(&value.argument);
        if let AssignmentReference::Abrupt(result) = reference {
            return result;
        }
        let reference = self.canonicalize_assignment_reference(reference, true);
        let old = self.load_assignment_reference(reference);
        let numeric_old = self.reg();
        self.emit(Op::ToNumeric, numeric_old, old, 0, 0);
        let next = self.reg();
        self.emit(
            Op::IncDec,
            next,
            numeric_old,
            0,
            u32::from(value.operator as u8),
        );
        self.store_assignment_reference(reference, next);
        if value.prefix { next } else { numeric_old }
    }

    pub(super) fn binary(&mut self, value: &BinaryExpression<'_>) -> Register {
        let left = if let Some(constant) = binding_time::expression(&value.left).static_value() {
            Operand::constant(self.owner.constant(constant))
        } else {
            Operand::register(self.expression(&value.left))
        };
        let right = if let Some(constant) = binding_time::expression(&value.right).static_value() {
            Operand::constant(self.owner.constant(constant))
        } else if let Expression::Identifier(identifier) = &value.right
            && let Some(operand) = self.local_operand(identifier.name.as_str())
        {
            operand
        } else {
            Operand::register(self.expression(&value.right))
        };
        self.emit_binary(value.operator as u32, left, right)
    }
    fn local_operand(&mut self, name: &str) -> Option<Operand> {
        let source = self.owner.atom(name);
        let compiler_binding = self.is_compiler_binding(source);
        let lexical = self.active_lexical_binding(source);
        if lexical.is_none() && !compiler_binding && self.with_depth != self.inherited_with_depth {
            return None;
        }
        let atom = lexical.unwrap_or(source);
        self.local_slots.get(&atom).copied().map(Operand::local)
    }

    pub(super) fn condition(&mut self, value: &Expression<'_>) -> usize {
        if let Expression::BinaryExpression(binary) = value {
            let left = if let Some(constant) = binding_time::expression(&binary.left).static_value()
            {
                Operand::constant(self.owner.constant(constant))
            } else {
                Operand::register(self.expression(&binary.left))
            };
            let right =
                if let Some(constant) = binding_time::expression(&binary.right).static_value() {
                    Operand::constant(self.owner.constant(constant))
                } else if let Expression::Identifier(identifier) = &binary.right
                    && let Some(operand) = self.local_operand(identifier.name.as_str())
                {
                    operand
                } else {
                    Operand::register(self.expression(&binary.right))
                };
            return self.emit(
                Op::JumpBinaryFalse,
                binary.operator as u16,
                left.0,
                right.0,
                0,
            );
        }
        let test = self.expression(value);
        self.emit(Op::JumpFalse, test, 0, 0, 0)
    }

    pub(crate) fn load_this_value(&mut self) -> Register {
        let dst = self.reg();
        if let Some(this) = self.this_override {
            self.emit(Op::Move, dst, this, 0, 0);
        } else {
            self.emit(Op::LoadThis, dst, 0, 0, 0);
        }
        dst
    }
}
