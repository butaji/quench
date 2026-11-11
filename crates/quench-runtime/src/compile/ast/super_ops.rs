use super::*;

impl FunctionCompiler<'_, '_> {
    /// Stores the class's instance home, its `prototype`, then declares each private field
    /// name under its home, the class for a static field, before any code of the class can
    /// name it (`Vm::declare_private_field`).
    pub(in crate::compile) fn store_class_home(&mut self, class: &Class<'_>, home_atom: Atom, class_value: Register, prototype: Register) {
        self.store_atom(home_atom, prototype);
        // A field's name, and an instance method's or accessor's, whose brand is its key.
        for element in &class.body.body {
            let (key, home, method) = match element {
                ClassElement::PropertyDefinition(field) => (&field.key, if field.r#static { class_value } else { prototype }, 0),
                ClassElement::MethodDefinition(method) if !method.r#static => (&method.key, prototype, 1),
                _ => continue,
            };
            if let PropertyKey::PrivateIdentifier(identifier) = key {
                let atom = self.owner.private_name_atom(identifier.span);
                self.emit(Op::DeclarePrivateName, 0, home, method, atom);
            }
        }
    }

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
}
