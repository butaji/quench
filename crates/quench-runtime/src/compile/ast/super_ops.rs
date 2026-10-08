use super::*;

impl FunctionCompiler<'_, '_> {
    pub(super) fn super_get(
        &mut self,
        base: Register,
        key: Register,
        receiver: Register,
    ) -> Register {
        self.emit(Op::RequireObjectCoercible, 0, base, 0, 0);
        let callee = self.load_name("\0rqj:super-get");
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
        let callee = self.load_name("\0rqj:super-set");
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
            self.load_name("\0rqj:super")
        };
        if !self.super_home {
            return home;
        }
        let getter = self.load_name("\0rqj:super-base");
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
}
