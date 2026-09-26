use super::*;
use crate::heap::{FinalizationEntries, FinalizationEntry};

pub(super) fn finalization_native_length(native: Native) -> Option<f64> {
    Some(match native {
        Native::FinalizationRegistry => 1.0,
        Native::FinalizationRegistryRegister => 2.0,
        Native::FinalizationRegistryUnregister => 1.0,
        _ => return None,
    })
}

impl<H: Host> Vm<H> {
    pub(super) fn is_finalization_native(native: Native) -> bool {
        matches!(
            native,
            Native::FinalizationRegistryRegister | Native::FinalizationRegistryUnregister
        )
    }

    pub(super) fn install_finalization_registry(
        &mut self,
        program: &ResidualProgram,
    ) -> Result<(), JsError> {
        let registry = self.native_value(Native::FinalizationRegistry);
        self.set_builtin_function_name(registry, "FinalizationRegistry")?;
        let prototype = self.object();
        self.finalization_registry_proto = prototype;
        self.set_builtin_value_named(registry, "prototype", prototype)?;
        let prototype_atom = self.intern_atom("prototype");
        self.set_property_attributes(
            registry,
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
        self.set_builtin_value_named(prototype, "constructor", registry)?;
        self.install_builtin_to_string_tag(prototype, "FinalizationRegistry")?;
        for (name, native) in [
            ("register", Native::FinalizationRegistryRegister),
            ("unregister", Native::FinalizationRegistryUnregister),
        ] {
            self.set_builtin_named(program, prototype, name, native)?;
        }
        self.global(program, "FinalizationRegistry", registry)
    }

    pub(super) fn install_finalization_registry_for_realm(
        &mut self,
        global: Value,
        object_prototype: Value,
    ) -> Result<(), JsError> {
        let constructor = self.native_with_realm(Native::FinalizationRegistry, global, global);
        let prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
        self.set_builtin_function_name(constructor, "FinalizationRegistry")?;
        self.set_builtin_value_named(constructor, "prototype", prototype)?;
        self.set_builtin_value_named(prototype, "constructor", constructor)?;
        let prototype_atom = self.intern_atom("prototype");
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
        self.install_builtin_to_string_tag(prototype, "FinalizationRegistry")?;
        for (name, native) in [
            ("register", Native::FinalizationRegistryRegister),
            ("unregister", Native::FinalizationRegistryUnregister),
        ] {
            let method = self.native_with_realm(native, global, global);
            self.set_builtin_function_name(method, name)?;
            self.set_builtin_value_named(prototype, name, method)?;
        }
        let name = self.intern_atom("FinalizationRegistry");
        self.set_property(global, name, constructor)?;
        self.set_property_attributes(
            global,
            PropertyKey::string(name),
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

    pub(super) fn construct_finalization_registry_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        let callback = args.first().copied().unwrap_or(Value::UNDEFINED);
        if !matches!(
            self.heap.get(callback),
            Some(Cell::Function { .. } | Cell::Proxy { .. })
        ) {
            return Err(self.type_error(p, "FinalizationRegistry callback is not callable".into()));
        }
        let prototype_atom = self.intern_atom("prototype");
        let candidate = self.get_property(p, new_target, prototype_atom)?;
        let prototype = if self.object_data(candidate).is_some() {
            candidate
        } else {
            let realm = match self.heap.get(new_target) {
                Some(Cell::Function { realm, .. }) => *realm,
                _ => self.realm.globals,
            };
            let constructor_atom = self.intern_atom("FinalizationRegistry");
            let realm_constructor = self.get_property(p, realm, constructor_atom)?;
            let realm_prototype = self.get_property(p, realm_constructor, prototype_atom)?;
            if self.object_data(realm_prototype).is_some() {
                realm_prototype
            } else {
                self.finalization_registry_proto
            }
        };
        Ok(self.heap.alloc(Cell::FinalizationRegistry {
            object: Self::empty_object(prototype),
            callback,
            entries: Box::new(FinalizationEntries::default()),
        }))
    }

    pub(super) fn call_finalization_registry_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::FinalizationRegistryRegister => {
                let target_value =
                    self.weak_key(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let target = self
                    .heap
                    .weak_handle(target_value)
                    .ok_or_else(|| JsError("FinalizationRegistry target is not live".into()))?;
                let held = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                if self.same_value(target_value, held) {
                    return Err(self.type_error(
                        p,
                        "FinalizationRegistry target and held value must differ".into(),
                    ));
                }
                let token = match args.get(2).copied().filter(|value| !value.is_undefined()) {
                    None => None,
                    Some(value) => {
                        let value = self.weak_key(p, value)?;
                        Some(self.heap.weak_handle(value).ok_or_else(|| {
                            JsError("FinalizationRegistry unregister token is not live".into())
                        })?)
                    }
                };
                let Some(Cell::FinalizationRegistry { entries, .. }) = self.heap.get_mut(this)
                else {
                    return Err(self.type_error(p, "invalid FinalizationRegistry receiver".into()));
                };
                entries.push(FinalizationEntry {
                    target,
                    held,
                    token,
                });
                Ok(Value::UNDEFINED)
            }
            Native::FinalizationRegistryUnregister => {
                let token = self.weak_key(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let Some(token) = self.heap.weak_handle(token) else {
                    return Ok(Value::FALSE);
                };
                let Some(Cell::FinalizationRegistry { entries, .. }) = self.heap.get_mut(this)
                else {
                    return Err(self.type_error(p, "invalid FinalizationRegistry receiver".into()));
                };
                let before = entries.len();
                entries.retain(|entry| entry.token != Some(token));
                Ok(if entries.len() != before {
                    Value::TRUE
                } else {
                    Value::FALSE
                })
            }
            _ => Err(self.type_error(p, "invalid finalization native".into())),
        }
    }
}

include!("disposal.rs");
include!("relational.rs");
