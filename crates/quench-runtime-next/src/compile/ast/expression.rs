use super::*;
impl FunctionCompiler<'_, '_> {
    pub(crate) fn expression(&mut self, expression: &Expression<'_>) -> Register {
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
            Expression::NewTarget(_) => self.load_name("\0rqj:new-target"),
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
                let callee = self.load_name("\0rqj:dynamic-import");
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
        let callee = self.load_name("RegExp");
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
    pub(crate) fn load_atom(&mut self, atom: Atom) -> Register {
        let compiler_binding = self.is_compiler_binding(atom);
        let lexical = self.active_lexical_binding(atom);
        let atom = lexical.unwrap_or(atom);
        let dst = self.reg();
        if self.parameter_context
            && atom == self.owner.atom("arguments")
            && let Some(slot) = self.parameter_arguments_slot
        {
            self.emit(Op::LoadLocal, dst, 0, 0, u32::from(slot));
        } else if (self.with_depth == self.inherited_with_depth
            || lexical.is_some()
            || compiler_binding)
            && (self.function_scope.contains(&atom) || lexical.is_some() || compiler_binding)
            && let Some(slot) = self.local_slots.get(&atom).copied()
        {
            self.emit(Op::LoadLocal, dst, 0, 0, u32::from(slot));
        } else if self.with_depth == 0
            && !self.dynamic_eval
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
            self.emit(Op::LoadName, dst, 0, cache, atom);
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
        let source_atom = atom;
        let immutable_lexical = !initializing
            && self
                .lexical_scopes
                .iter()
                .rev()
                .any(|scope| scope.immutable.contains(&source_atom));
        let compiler_binding = self.is_compiler_binding(source_atom);
        let lexical = self.active_lexical_binding(source_atom);
        let atom = lexical.unwrap_or(source_atom);
        if self.owner.atoms[atom as usize]
            .as_ref()
            .contains("\0rqj:class-binding:")
            || immutable_lexical
            || (self.with_depth == 0
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
            if self.function_id == 0 && !self.owner.module_goal {
                let cache = self.owner.cache_site();
                self.emit(Op::StoreName, value, 0, cache, atom);
            }
        } else if (self.with_depth == 0 || initializing)
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
            self.emit(Op::StoreName, value, 0, cache, atom);
        }
    }

    pub(super) fn has_immutable_capture(&mut self, atom: Atom) -> bool {
        let name = self.owner.atoms[atom as usize].clone();
        let marker = self.owner.atom(&format!("\0rqj:immutable-capture:{name}"));
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
            body,
            &scopes,
            Some(self.function_id),
            FunctionOptions {
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
            let atom = self.hidden_local("\0rqj:lexical-this-override");
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

    pub(super) fn assign_target(
        &mut self,
        target: &SimpleAssignmentTarget<'_>,
        right: Register,
        operator: u8,
    ) -> Register {
        match target {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => {
                let atom = self.owner.atom(id.name.as_str());
                let value = self.compound_name(atom, right, operator);
                self.store_atom(atom, value);
                value
            }
            SimpleAssignmentTarget::StaticMemberExpression(item) => {
                let atom = self.owner.atom(item.property.name.as_str());
                if operator == 0 && matches!(&item.object, Expression::ThisExpression(_)) {
                    let site = self.owner.cache_site();
                    self.emit(Op::SetThisField, right, 0, site, atom);
                    return right;
                }
                let object = self.expression(&item.object);
                let value = self.compound_field(object, atom, right, operator);
                let site = self.owner.cache_site();
                self.emit(Op::SetField, value, object, site, atom);
                // A top-level global declaration is represented by the root
                // frame while `globalThis` is its object view. Keep both
                // views coherent at this explicit mutation boundary.
                if matches!(&item.object, Expression::Identifier(id) if id.name == "globalThis") {
                    self.store_atom(atom, value);
                }
                value
            }
            SimpleAssignmentTarget::PrivateFieldExpression(item) => {
                let atom = self.owner.private_name_atom(item.field.span);
                let object = self.expression(&item.object);
                self.emit(Op::CheckPrivate, object, 0, 0, atom);
                let value = self.compound_field(object, atom, right, operator);
                let site = self.owner.cache_site();
                self.emit(Op::SetField, value, object, site, atom);
                value
            }
            SimpleAssignmentTarget::ComputedMemberExpression(item) => {
                let object = self.expression(&item.object);
                if matches!(&item.object, Expression::Super(_)) {
                    let receiver = self.reg();
                    self.emit(Op::LoadThis, receiver, 0, 0, 0);
                }
                let raw_key = self.expression(&item.expression);
                self.emit(Op::RequireObjectCoercible, 0, object, 0, 0);
                let key = self.reg();
                self.emit(Op::ToPropertyKey, key, raw_key, 0, 0);
                let value = self.compound_index(object, key, right, operator);
                self.emit(Op::SetIndex, value, object, key, 0);
                value
            }
            _ => {
                self.owner
                    .reject(target.span(), "assignment target unsupported");
                right
            }
        }
    }
    pub(super) fn compound_name(&mut self, atom: Atom, right: Register, op: u8) -> Register {
        if op == 0 {
            return right;
        }
        let left = self.load_atom(atom);
        self.apply_compound(left, right, op)
    }
    pub(super) fn compound_field(
        &mut self,
        object: Register,
        atom: Atom,
        right: Register,
        op: u8,
    ) -> Register {
        if op == 0 {
            return right;
        }
        let left = self.reg();
        let cache = self.owner.cache_site();
        self.emit(
            Op::GetField,
            left,
            FieldBase::register(object).0,
            cache,
            atom,
        );
        self.apply_compound(left, right, op)
    }
    pub(super) fn compound_index(
        &mut self,
        object: Register,
        key: Register,
        right: Register,
        op: u8,
    ) -> Register {
        if op == 0 {
            return right;
        }
        let left = self.reg();
        self.emit(Op::GetIndex, left, object, key, 0);
        self.apply_compound(left, right, op)
    }
    pub(super) fn apply_compound(&mut self, left: Register, right: Register, op: u8) -> Register {
        self.emit_binary(
            u32::from(op + 7),
            Operand::register(left),
            Operand::register(right),
        )
    }
    pub(super) fn update(&mut self, value: &UpdateExpression<'_>) -> Register {
        let (old, target) = match &value.argument {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => {
                let atom = self.owner.atom(id.name.as_str());
                let capture_global_reference = self.needs_strict_global_reference_capture(atom);
                let environment = if self.with_depth != 0 || capture_global_reference {
                    let environment = self.reg();
                    let cache = self.owner.cache_site();
                    self.emit(
                        Op::ResolveName,
                        environment,
                        u16::from(capture_global_reference),
                        cache,
                        atom,
                    );
                    Some(environment)
                } else {
                    None
                };
                (self.load_atom(atom), UpdateTarget::Name(atom, environment))
            }
            SimpleAssignmentTarget::StaticMemberExpression(item) => {
                let object = self.expression(&item.object);
                let atom = self.owner.atom(item.property.name.as_str());
                let old = self.reg();
                let cache = self.owner.cache_site();
                self.emit(
                    Op::GetField,
                    old,
                    FieldBase::register(object).0,
                    cache,
                    atom,
                );
                (old, UpdateTarget::Field(atom, object))
            }
            SimpleAssignmentTarget::ComputedMemberExpression(item) => {
                let object = self.expression(&item.object);
                if matches!(&item.object, Expression::Super(_)) {
                    let receiver = self.reg();
                    self.emit(Op::LoadThis, receiver, 0, 0, 0);
                }
                let raw_key = self.expression(&item.expression);
                self.emit(Op::RequireObjectCoercible, 0, object, 0, 0);
                let key = self.reg();
                self.emit(Op::ToPropertyKey, key, raw_key, 0, 0);
                let old = self.reg();
                self.emit(Op::GetIndex, old, object, key, 0);
                (old, UpdateTarget::Index(object, key))
            }
            _ => {
                self.owner.reject(value.span, "update target unsupported");
                return self.literal(Constant::Undefined);
            }
        };
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
        match target {
            UpdateTarget::Name(atom, Some(environment)) => {
                self.emit(
                    Op::StoreResolvedName,
                    next,
                    environment,
                    u16::from(self.strict),
                    atom,
                );
            }
            UpdateTarget::Name(atom, None) => self.store_atom(atom, next),
            UpdateTarget::Field(atom, object) => {
                let cache = self.owner.cache_site();
                self.emit(Op::SetField, next, object, cache, atom);
            }
            UpdateTarget::Index(object, key) => {
                self.emit(Op::SetIndex, next, object, key, 0);
            }
        }
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
