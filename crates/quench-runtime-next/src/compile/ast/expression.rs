use super::*;
impl FunctionCompiler<'_, '_> {
    pub(crate) fn expression(&mut self, expression: &Expression<'_>) -> Register {
        match expression {
            Expression::NumericLiteral(value) => self.literal(Constant::Number(value.value)),
            Expression::StringLiteral(value) => self.string_literal(value),
            Expression::BigIntLiteral(value) => self.bigint_literal(value),
            Expression::RegExpLiteral(value) => self.regexp_literal(value),
            Expression::TemplateLiteral(value) => self.template_literal(value),
            Expression::BooleanLiteral(value) => self.literal(Constant::Boolean(value.value)),
            Expression::NullLiteral(_) => self.literal(Constant::Null),
            Expression::Identifier(value) => self.load_name(value.name.as_str()),
            Expression::ThisExpression(_) => {
                let dst = self.reg();
                self.emit(Op::LoadThis, dst, 0, 0, 0);
                dst
            }
            Expression::Super(_) => self.load_name("\0rqj:super"),
            Expression::FunctionExpression(value) => self.function_expression(value),
            Expression::ArrowFunctionExpression(value) => self.arrow_function_expression(value),
            Expression::ClassExpression(value) => self.class_expression(value),
            Expression::ArrayExpression(value) => self.array_expression(value),
            Expression::ObjectExpression(value) => self.object_expression(value),
            Expression::StaticMemberExpression(value) => self.static_member(value),
            Expression::ComputedMemberExpression(value) => self.computed_member(value),
            Expression::ChainExpression(value) => self.chain_expression(&value.expression),
            Expression::AssignmentExpression(value) => self.assignment(value),
            Expression::UpdateExpression(value) => self.update(value),
            Expression::BinaryExpression(value) => self.binary(value),
            Expression::UnaryExpression(value) => self.unary(value),
            Expression::LogicalExpression(value) => self.logical(value),
            Expression::ConditionalExpression(value) => self.conditional(value),
            Expression::CallExpression(value) if value.optional => self.optional_call(value),
            Expression::CallExpression(value) => self.call(value),
            Expression::NewExpression(value) => self.construct(value),
            Expression::SequenceExpression(value) => self.sequence_expression(value),
            Expression::ParenthesizedExpression(value) => self.expression(&value.expression),
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
    pub(super) fn load_atom(&mut self, atom: Atom) -> Register {
        let dst = self.reg();
        if let Some(slot) = self.local_slots.get(&atom).copied() {
            self.emit(Op::LoadLocal, dst, 0, 0, u32::from(slot));
        } else if let Some((depth, slot)) = self
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
                ((depth as u32) << 16) | u32::from(slot),
            );
        } else {
            let cache = self.owner.cache_site();
            self.emit(Op::LoadName, dst, 0, cache, atom);
        }
        dst
    }

    pub(crate) fn store_atom(&mut self, atom: Atom, value: Register) {
        if let Some(slot) = self.local_slots.get(&atom).copied() {
            self.emit(Op::StoreLocal, value, 0, 0, u32::from(slot));
        } else if let Some((depth, slot)) = self
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
                ((depth as u32) << 16) | u32::from(slot),
            );
        } else {
            let cache = self.owner.cache_site();
            self.emit(Op::StoreName, value, 0, cache, atom);
        }
    }
    pub(super) fn function_expression(&mut self, value: &oxc_ast::ast::Function<'_>) -> Register {
        let params = Self::params(value, self.owner);
        let body = value
            .body
            .as_ref()
            .map_or(&[][..], |body| body.statements.as_slice());
        let name = value.id.as_ref().map(|id| id.name.as_str());
        let mut scopes = vec![Rc::clone(&self.local_slots)];
        scopes.extend(self.scopes.iter().cloned());
        let function = self.owner.compile_function(
            name,
            &params,
            body,
            &scopes,
            Some(self.function_id),
            FunctionOptions {
                defaults: Some(&value.params),
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
        let mut scopes = vec![Rc::clone(&self.local_slots)];
        scopes.extend(self.scopes.iter().cloned());
        let function = self
            .owner
            .compile_arrow_function(value, &scopes, Some(self.function_id));
        let dst = self.reg();
        self.emit(Op::MakeClosure, dst, 0, 0, function);
        dst
    }
    pub(super) fn static_get(&mut self, object: &Expression<'_>, key: &str) -> Register {
        if let Expression::StaticMemberExpression(inner) = object
            && matches!(&inner.object, Expression::ThisExpression(_))
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
            FieldBase::THIS
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
        let object = if matches!(object, Expression::Super(_)) && !self.super_static {
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
                value
            }
            SimpleAssignmentTarget::ComputedMemberExpression(item) => {
                let object = self.expression(&item.object);
                let key = self.expression(&item.expression);
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
                (self.load_atom(atom), UpdateTarget::Name(atom))
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
                let key = self.expression(&item.expression);
                let old = self.reg();
                self.emit(Op::GetIndex, old, object, key, 0);
                (old, UpdateTarget::Index(object, key))
            }
            _ => {
                self.owner.reject(value.span, "update target unsupported");
                return self.literal(Constant::Undefined);
            }
        };
        let next = self.reg();
        self.emit(Op::IncDec, next, old, 0, u32::from(value.operator as u8));
        match target {
            UpdateTarget::Name(atom) => self.store_atom(atom, next),
            UpdateTarget::Field(atom, object) => {
                let cache = self.owner.cache_site();
                self.emit(Op::SetField, next, object, cache, atom);
            }
            UpdateTarget::Index(object, key) => {
                self.emit(Op::SetIndex, next, object, key, 0);
            }
        }
        if value.prefix { next } else { old }
    }

    pub(super) fn binary(&mut self, value: &BinaryExpression<'_>) -> Register {
        let left = if let Some(constant) = binding_time::expression(&value.left).static_value() {
            Operand::constant(self.owner.constant(constant))
        } else if let Expression::Identifier(identifier) = &value.left
            && let Some(operand) = self.local_operand(identifier.name.as_str())
        {
            operand
        } else if let Expression::StaticMemberExpression(field) = &value.left
            && matches!(&field.object, Expression::ThisExpression(_))
        {
            let atom = self.owner.atom(field.property.name.as_str());
            let cache = self.owner.cache_site();
            Operand::field(self.field_site(FieldBase::THIS, (atom, cache), None))
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
        let atom = self.owner.atom(name);
        self.local_slots.get(&atom).copied().map(Operand::local)
    }

    pub(super) fn condition(&mut self, value: &Expression<'_>) -> usize {
        if let Expression::BinaryExpression(binary) = value {
            let left = if let Some(constant) = binding_time::expression(&binary.left).static_value()
            {
                Operand::constant(self.owner.constant(constant))
            } else if let Expression::Identifier(identifier) = &binary.left
                && let Some(operand) = self.local_operand(identifier.name.as_str())
            {
                operand
            } else if let Expression::StaticMemberExpression(field) = &binary.left
                && matches!(&field.object, Expression::ThisExpression(_))
            {
                let atom = self.owner.atom(field.property.name.as_str());
                let cache = self.owner.cache_site();
                Operand::field(self.field_site(FieldBase::THIS, (atom, cache), None))
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
}
