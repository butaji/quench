use super::property_key::PropertyKey;
use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn install_boolean(&mut self, p: &ResidualProgram) -> Result<(), JsError> {
        let constructor = self.native_value(Native::Boolean);
        self.install_boolean_for_realm(p, self.realm.globals, self.object_proto, constructor)
    }

    pub(super) fn install_boolean_for_realm(
        &mut self,
        p: &ResidualProgram,
        global: Value,
        object_prototype: Value,
        constructor: Value,
    ) -> Result<(), JsError> {
        self.set_builtin_function_name(constructor, "Boolean")?;
        let prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
        let boolean_value = self.intern_atom("\0quench:boolean-value");
        self.set_property(prototype, boolean_value, Value::FALSE)?;
        self.set_property_attributes(
            prototype,
            PropertyKey::string(boolean_value),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: false,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        self.set_builtin_value_named(constructor, "prototype", prototype)?;
        let prototype_atom = self.prototype_atom();
        self.set_property_attributes(
            constructor,
            PropertyKey::string(prototype_atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: false,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        self.set_builtin_value_named(prototype, "constructor", constructor)?;
        for (name, native) in [
            ("toString", Native::BooleanToString),
            ("valueOf", Native::BooleanValueOf),
        ] {
            let method = self.native_with_realm(native, global, global);
            self.set_builtin_function_name(method, name)?;
            self.set_builtin_value_named(prototype, name, method)?;
        }
        if global == self.realm.globals {
            self.global(p, "Boolean", constructor)
        } else {
            let atom = self.intern_atom("Boolean");
            self.set_property(global, atom, constructor)?;
            self.set_property_attributes(
                global,
                PropertyKey::string(atom),
                PropertyAttributes {
                    writable: true,
                    enumerable: false,
                    configurable: true,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
            Ok(())
        }
    }

    pub(super) fn boolean_prototype_value(&mut self, receiver: Value) -> Option<Value> {
        if receiver.as_bool().is_some() {
            return Some(receiver);
        }
        let value_atom = self.intern_atom("\0quench:boolean-value");
        if let Some(value) = self
            .own_property(receiver, value_atom)
            .filter(|value| value.as_bool().is_some())
        {
            return Some(value);
        }
        None
    }
}
