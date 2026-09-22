use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn object_define_properties(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let target = args.first().copied().unwrap_or(Value::UNDEFINED);
        if self.object_data(target).is_none() {
            return Err(JsError("defineProperties target is not an object".into()));
        }
        let descriptors = self.box_object(args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
        let keys = self.object_own_key_values(p, descriptors)?;
        for key in keys {
            let descriptor = self.get_index(p, descriptors, key)?;
            self.object_define_property(p, &[target, key, descriptor])?;
        }
        Ok(target)
    }

    pub(super) fn install_object(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let object = self.native_value(Native::Object);
        self.set_named(program, object, "prototype", self.object_proto)?;
        self.set_named(program, self.object_proto, "constructor", object)?;
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
            self.function_proto,
            "bind",
            self.native_value(Native::FunctionBind),
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
