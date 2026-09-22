use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn install_object(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let object = self.native_value(Native::Object);
        self.set_named(program, object, "prototype", self.object_proto)?;
        self.set_named(
            program,
            self.function_proto,
            "call",
            self.native_value(Native::FunctionCall),
        )?;
        self.set_named(
            program,
            self.function_proto,
            "apply",
            self.native_value(Native::FunctionApply),
        )?;
        self.set_named(
            program,
            object,
            "keys",
            self.native_value(Native::ObjectKeys),
        )?;
        self.global(program, "Proxy", self.native_value(Native::Proxy))?;
        let proxy = self.native_value(Native::Proxy);
        self.set_named(
            program,
            proxy,
            "revocable",
            self.native_value(Native::ProxyRevocable),
        )?;
        self.install_object_extra(program, object)?;
        for (name, native) in [
            ("create", Native::ObjectCreate),
            ("assign", Native::ObjectAssign),
            ("getPrototypeOf", Native::ObjectGetPrototypeOf),
            ("setPrototypeOf", Native::ObjectSetPrototypeOf),
            ("hasOwn", Native::ObjectHasOwn),
            ("preventExtensions", Native::ObjectPreventExtensions),
            ("isExtensible", Native::ObjectIsExtensible),
            ("seal", Native::ObjectSeal),
            ("isSealed", Native::ObjectIsSealed),
            ("freeze", Native::ObjectFreeze),
            ("isFrozen", Native::ObjectIsFrozen),
        ] {
            self.set_named(program, object, name, self.native_value(native))?;
        }
        self.global(program, "Object", object)
    }
}
