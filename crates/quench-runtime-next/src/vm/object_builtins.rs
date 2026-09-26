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
        self.set_builtin_named(program, self.object_proto, "constructor", Native::Object)?;
        self.set_builtin_named(program, self.function_proto, "call", Native::FunctionCall)?;
        self.set_builtin_named(program, self.function_proto, "apply", Native::FunctionApply)?;
        self.set_builtin_named(program, self.function_proto, "bind", Native::FunctionBind)?;
        self.set_builtin_named(
            program,
            self.function_proto,
            "toString",
            Native::FunctionToString,
        )?;
        let throw_type_error = self.native_value(Native::ThrowTypeError);
        for name in ["caller", "arguments"] {
            let atom = self.intern_atom(name);
            self.set_named(program, self.function_proto, name, Value::UNDEFINED)?;
            let getter = if name == "caller" {
                self.native_value(Native::FunctionCaller)
            } else {
                throw_type_error
            };
            self.set_property_attributes(
                self.function_proto,
                property_key::PropertyKey::string(atom),
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: true,
                    accessor: true,
                    getter: Some(getter),
                    setter: Some(throw_type_error),
                },
            );
        }
        self.set_builtin_named(program, object, "keys", Native::ObjectKeys)?;
        self.global(
            program,
            "\0rqj:for-in-keys",
            self.native_value(Native::ForInKeys),
        )?;
        self.global(
            program,
            "\0rqj:for-in-key-is-enumerable",
            self.native_value(Native::ForInKeyIsEnumerable),
        )?;
        self.global(program, "Proxy", self.native_value(Native::Proxy))?;
        let proxy = self.native_value(Native::Proxy);
        self.set_builtin_named(program, proxy, "revocable", Native::ProxyRevocable)?;
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
            self.set_builtin_named(program, object, name, native)?;
        }
        self.global(program, "Object", object)
    }
}
