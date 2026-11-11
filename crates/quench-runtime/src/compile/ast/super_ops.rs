use super::*;

impl FunctionCompiler<'_, '_> {
    pub(super) fn super_get(
        &mut self,
        base: Register,
        key: Register,
        receiver: Register,
    ) -> Register {
        self.emit(Op::RequireObjectCoercible, 0, base, 0, 0);
        let callee = self.load_name("\0quench:super-get");
        let this = self.literal(Constant::Undefined);
        let start = self.next_reg;
        for argument in [base, key, receiver] {
            let slot = self.reg();
            self.emit(Op::Move, slot, argument, 0, 0);
        }
        let result = self.reg();
        self.emit(
            Op::Call,
            result,
            callee,
            this,
            crate::bytecode::ImmediateLayout::call_immediate(start, 3, false, false),
        );
        result
    }

    pub(super) fn super_set(
        &mut self,
        base: Register,
        key: Register,
        value: Register,
        receiver: Register,
    ) {
        let callee = self.load_name("\0quench:super-set");
        let this = self.literal(Constant::Undefined);
        let strict = self.literal(Constant::Boolean(self.strict));
        let start = self.next_reg;
        for argument in [base, key, value, receiver, strict] {
            let slot = self.reg();
            self.emit(Op::Move, slot, argument, 0, 0);
        }
        let result = self.reg();
        self.emit(
            Op::Call,
            result,
            callee,
            this,
            crate::bytecode::ImmediateLayout::call_immediate(start, 5, false, false),
        );
    }

    pub(super) fn super_base(&mut self) -> Register {
        if self.super_home {
            let receiver = self.reg();
            self.emit(Op::LoadThis, receiver, 0, 0, 0);
        }
        let home = if let Some(atom) = self.super_home_atom {
            self.load_atom(atom)
        } else {
            self.load_name("\0quench:super")
        };
        if !self.super_home {
            return home;
        }
        let getter = self.load_name(crate::bytecode::SUPER_BASE_BINDING);
        let this = self.literal(Constant::Undefined);
        let start = self.next_reg;
        let arg = self.reg();
        self.emit(Op::Move, arg, home, 0, 0);
        let result = self.reg();
        self.emit(
            Op::Call,
            result,
            getter,
            this,
            crate::bytecode::ImmediateLayout::call_immediate(start, 1, false, false),
        );
        result
    }

        /// `inner.key` (`a.b.c`, `this.b.c`) as one nested site: both fields read in order
        /// (getters, throws and their order included), and the intermediate value never made.
        pub(super) fn field_chain(&mut self, inner: &StaticMemberExpression<'_>, key: &str) -> Register {
            let base = if matches!(&inner.object, Expression::ThisExpression(_)) {
                self.this_override
                    .map(FieldBase::register)
                    .unwrap_or(FieldBase::THIS)
            } else {
                FieldBase::register(self.expression(&inner.object))
            };
            let dst = self.reg();
            let first = self.owner.atom(inner.property.name.as_str());
            let second = self.owner.atom(key);
            let site = self.owner.cache_site();
            let second_site = self.owner.cache_site();
            let access = self.field_site(base, (first, site), Some((second, second_site)));
            let first_position = self.owner.source_position(inner.property.span.start);
            self.owner.field_sites[access as usize].sources[0] = Some((first_position.line, first_position.column));
            self.emit(Op::GetField, dst, FieldBase::NESTED, 0, access);
            dst
        }
}
