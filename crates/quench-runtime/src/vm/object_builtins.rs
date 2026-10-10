use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn object_define_properties(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let target = self
            .heap
            .root(args.first().copied().unwrap_or(Value::UNDEFINED));
        let input = self
            .heap
            .root(args.get(1).copied().unwrap_or(Value::UNDEFINED));
        let mut descriptors_root = None;
        let mut keys = Vec::new();
        let mut definitions = Vec::new();
        let outcome = (|| {
            if !self.is_object_like(self.heap.root_value(target).unwrap()) {
                return Err(self.type_error(p, "defineProperties target is not an object".into()));
            }
            let descriptors = self.heap.root_value(input).unwrap();
            let descriptors = self.box_object_or_type_error(p, descriptors)?;
            let descriptors = self.heap.root(descriptors);
            descriptors_root = Some(descriptors);
            let object = self.heap.root_value(descriptors).unwrap();
            keys = self
                .object_own_key_values(p, object)?
                .into_iter()
                .map(|key| self.heap.root(key))
                .collect();
            for &key in &keys {
                let object = self.heap.root_value(descriptors).unwrap();
                let property = self.heap.root_value(key).unwrap();
                let current = self.object_get_own_property_descriptor(p, &[object, property])?;
                if current.is_undefined() || !self.descriptor_flag(current, "enumerable") {
                    continue;
                }
                let object = self.heap.root_value(descriptors).unwrap();
                let property = self.heap.root_value(key).unwrap();
                let view = self.get_index(p, object, property)?;
                let record = self.to_property_descriptor(p, view)?;
                definitions.push((
                    key,
                    super::property_definition::RootedPropertyDescriptor::new(
                        &mut self.heap,
                        record,
                    ),
                ));
            }
            for (key, descriptor) in &definitions {
                let object = self.heap.root_value(target).unwrap();
                let property = self.heap.root_value(*key).unwrap();
                self.define_property_or_throw(p, object, property, descriptor.resolve(&self.heap))?;
            }
            Ok(self.heap.root_value(target).unwrap())
        })();
        for (_, descriptor) in definitions {
            descriptor.release(&mut self.heap);
        }
        for key in keys {
            self.heap.release_root(key);
        }
        for root in [Some(target), Some(input), descriptors_root]
            .into_iter()
            .flatten()
        {
            self.heap.release_root(root);
        }
        outcome
    }

    pub(super) fn install_object(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let object = self.native_value(Native::Object);
        self.set_builtin_function_name(object, "Object")?;
        self.set_named(program, object, "prototype", self.object_proto)?;
        let prototype = self.prototype_atom();
        self.set_property_attributes(
            object,
            super::property_key::PropertyKey::string(prototype),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: false,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
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
        let throw_type_error = self.throw_type_error_for_current_realm();
        for name in ["caller", "arguments"] {
            let atom = self.intern_atom(name);
            self.set_named(program, self.function_proto, name, Value::UNDEFINED)?;
            self.set_property_attributes(
                self.function_proto,
                property_key::PropertyKey::string(atom),
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: true,
                    accessor: true,
                    getter: Some(throw_type_error),
                    setter: Some(throw_type_error),
                },
            );
        }
        self.set_builtin_named(program, object, "keys", Native::ObjectKeys)?;
        self.global(
            program,
            "\0quench:for-in-keys",
            self.native_value(Native::ForInKeys),
        )?;
        self.global(
            program,
            "\0quench:for-in-key-is-enumerable",
            self.native_value(Native::ForInKeyIsEnumerable),
        )?;
        let proxy = self.native_value(Native::Proxy);
        self.set_builtin_function_name(proxy, "Proxy")?;
        self.set_builtin_named(program, proxy, "revocable", Native::ProxyRevocable)?;
        self.global(program, "Proxy", proxy)?;
        self.install_object_extra(program, object)?;
        for (name, native) in [
            ("create", Native::ObjectCreate),
            ("groupBy", Native::ObjectGroupBy),
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
