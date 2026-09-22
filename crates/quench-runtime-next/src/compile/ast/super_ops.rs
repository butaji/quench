use super::*;

impl FunctionCompiler<'_, '_> {
    pub(super) fn super_base(&mut self) -> Register {
        let home = if let Some(atom) = self.super_home_atom {
            self.load_atom(atom)
        } else {
            self.load_name("\0rqj:super")
        };
        if !self.super_home {
            return home;
        }
        let object = self.load_name("Object");
        let atom = self.owner.atom("getPrototypeOf");
        let getter = self.reg();
        let cache = self.owner.cache_site();
        self.emit(Op::GetField, getter, FieldBase::register(object).0, cache, atom);
        let arg = self.reg();
        self.emit(Op::Move, arg, home, 0, 0);
        let result = self.reg();
        self.emit(Op::Call, result, getter, object, (u32::from(arg) << 16) | 1);
        result
    }
}
