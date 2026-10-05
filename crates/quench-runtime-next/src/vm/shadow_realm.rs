use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn install_shadow_realm(
        &mut self,
        program: &ResidualProgram,
    ) -> Result<(), JsError> {
        let constructor = self.native_value(Native::ShadowRealm);
        self.shadow_realm_proto = self.install_shadow_realm_for_realm(
            program,
            self.realm.globals,
            constructor,
            self.object_proto,
        )?;
        self.install_shadow_realm_tag(self.shadow_realm_proto)?;
        self.global(program, "ShadowRealm", constructor)
    }

    pub(super) fn install_shadow_realm_for_realm(
        &mut self,
        _program: &ResidualProgram,
        global: Value,
        constructor: Value,
        object_prototype: Value,
    ) -> Result<Value, JsError> {
        let prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
        for (name, native, length) in [
            ("evaluate", Native::ShadowRealmEvaluate, 1.0),
            ("importValue", Native::ShadowRealmImportValue, 2.0),
        ] {
            let method = self.native_with_realm(native, Value::NULL, global);
            self.set_builtin_function_name(method, name)?;
            self.set_builtin_function_length(method, length)?;
            self.set_builtin_value_named(prototype, name, method)?;
        }
        self.set_builtin_value_named(prototype, "constructor", constructor)?;
        self.set_builtin_value_named(constructor, "prototype", prototype)?;
        self.set_builtin_function_name(constructor, "ShadowRealm")?;
        self.set_builtin_function_length(constructor, 0.0)?;
        self.set_builtin_value_named(global, "ShadowRealm", constructor)?;
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
        let constructor_atom = self.intern_atom("constructor");
        self.set_property_attributes(
            prototype,
            PropertyKey::string(constructor_atom),
            PropertyAttributes {
                writable: true,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        self.install_shadow_realm_tag(prototype)?;
        Ok(prototype)
    }

    pub(super) fn install_shadow_realm_tag(&mut self, prototype: Value) -> Result<(), JsError> {
        let Some(tag) = self.well_known_symbols.get("toStringTag").copied() else {
            return Ok(());
        };
        let value = self.heap.alloc(Cell::String("ShadowRealm".into()));
        self.set_symbol_property(prototype, tag, value)?;
        self.set_property_attributes(
            prototype,
            PropertyKey::symbol(tag),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        Ok(())
    }

    fn set_builtin_function_length(&mut self, function: Value, length: f64) -> Result<(), JsError> {
        let atom = self.intern_atom("length");
        if self
            .own_property(function, atom)
            .and_then(|value| value.as_number())
            == Some(length)
        {
            return Ok(());
        }
        let key = PropertyKey::string(atom);
        self.set_property_attributes(
            function,
            key,
            PropertyAttributes {
                writable: true,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        self.set_property(function, atom, Value::number(length))?;
        self.set_property_attributes(
            function,
            key,
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        Ok(())
    }

    pub(super) fn shadow_realm_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::ShadowRealmEvaluate => self.shadow_realm_evaluate(p, this, args),
            Native::ShadowRealmImportValue => self.shadow_realm_import_value(p, this, args),
            Native::ShadowRealmImportValueFulfilled => self.shadow_realm_import_fulfilled(p, args),
            Native::ShadowRealmWrappedFunction => self.call_shadow_wrapped_function(p, args),
            _ => Err(JsError("invalid ShadowRealm native".into())),
        }
    }

    fn shadow_realm_evaluate(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let Some(Cell::ShadowRealm {
            caller_global,
            realm_global,
            ..
        }) = self.heap.get(this)
        else {
            return Err(self.type_error(
                p,
                "ShadowRealm.prototype.evaluate called on incompatible receiver".into(),
            ));
        };
        let (caller_global, realm_global) = (*caller_global, *realm_global);
        let evaluation_caller = self.realm.globals;
        let source = args.first().copied().unwrap_or(Value::UNDEFINED);
        let Some(Cell::String(source)) = self.heap.get(source) else {
            return self.with_realm_type_error(
                p,
                caller_global,
                "ShadowRealm.prototype.evaluate requires a string",
            );
        };
        let source = source.host_string().to_owned();
        let atom_prefix = (0..self.atom_text.len() + self.dynamic_atoms.len())
            .map(|atom| self.atom_name(atom as u32).to_owned())
            .collect::<Vec<_>>();
        let source_is_invalid = crate::Engine::specialize_eval_with_context(
            &source,
            "<ShadowRealm>",
            &atom_prefix,
            false,
            crate::compile::EvalContext::default(),
            &[],
            &[],
        )
        .is_err();
        let prior_global = self.switch_realm_global(realm_global);
        let result = self.eval_global_script(p, &source, false);
        self.switch_realm_global(prior_global);
        match result {
            Ok(value) if self.is_function(value) => {
                self.wrap_shadow_callable(p, value, evaluation_caller)
            }
            Ok(value) if !self.is_object_like(value) => Ok(value),
            Ok(_) => {
                let prior_global = self.switch_realm_global(caller_global);
                let error =
                    self.type_error(p, "ShadowRealm evaluation must return a primitive".into());
                self.switch_realm_global(prior_global);
                Err(error)
            }
            Err(error) => {
                if source_is_invalid {
                    return self.with_realm_syntax_error(p, caller_global, error.to_string());
                }
                let prior_global = self.switch_realm_global(caller_global);
                let _ = error;
                let error = self.type_error(p, "ShadowRealm evaluation threw an exception".into());
                self.switch_realm_global(prior_global);
                Err(error)
            }
        }
    }

    fn shadow_realm_import_value(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let Some(Cell::ShadowRealm { realm_global, .. }) = self.heap.get(this) else {
            return Err(self.type_error(
                p,
                "ShadowRealm.prototype.importValue called on incompatible receiver".into(),
            ));
        };
        let realm_global = *realm_global;
        let specifier = args.first().copied().unwrap_or(Value::UNDEFINED);
        let specifier = self.to_string(p, specifier)?;
        let Some(export_name) = args.get(1).copied() else {
            return Err(self.type_error(
                p,
                "ShadowRealm.prototype.importValue export name must be a string".into(),
            ));
        };
        if !matches!(self.heap.get(export_name), Some(Cell::String(_))) {
            return Err(self.type_error(
                p,
                "ShadowRealm.prototype.importValue export name must be a string".into(),
            ));
        }
        let caller_global = self.realm.globals;
        self.start_shadow_realm_import(
            p,
            realm_global,
            caller_global,
            specifier.into(),
            export_name,
        )
    }

    fn start_shadow_realm_import(
        &mut self,
        p: &ResidualProgram,
        realm_global: Value,
        caller_global: Value,
        specifier: JsString,
        export_name: Value,
    ) -> Result<Value, JsError> {
        let specifier = self.heap.alloc(Cell::String(specifier));
        let prior_global = self.switch_realm_global(realm_global);
        let import = self.call_native(p, Native::DynamicImport, Value::UNDEFINED, &[specifier]);
        self.switch_realm_global(prior_global);
        let import = import?;
        let import_root = self.heap.root(import);
        let result = (|| {
            let on_fulfilled =
                self.shadow_realm_import_handler(export_name, caller_global, false)?;
            let on_rejected = self.shadow_realm_import_handler(export_name, caller_global, true)?;
            let then_atom = self.intern_atom("then");
            let then = self.get_property(p, import, then_atom)?;
            self.call_value(p, then, import, &[on_fulfilled, on_rejected])
        })();
        self.heap.release_root(import_root);
        result
    }

    fn shadow_realm_import_handler(
        &mut self,
        export_name: Value,
        caller_global: Value,
        rejected: bool,
    ) -> Result<Value, JsError> {
        let env = self.object();
        let export_atom = self.intern_atom("\0rqj:shadow-export-name");
        let caller_atom = self.intern_atom("\0rqj:shadow-import-caller");
        let rejected_atom = self.intern_atom("\0rqj:shadow-import-rejected");
        self.set_property(env, export_atom, export_name)?;
        self.set_property(env, caller_atom, caller_global)?;
        self.set_property(
            env,
            rejected_atom,
            if rejected { Value::TRUE } else { Value::FALSE },
        )?;
        Ok(self.native_with_realm(Native::ShadowRealmImportValueFulfilled, env, caller_global))
    }

    fn shadow_realm_import_fulfilled(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let env = self.active_native_env().unwrap_or(Value::NULL);
        let export_atom = self.intern_atom("\0rqj:shadow-export-name");
        let caller_atom = self.intern_atom("\0rqj:shadow-import-caller");
        let rejected_atom = self.intern_atom("\0rqj:shadow-import-rejected");
        let export_name = self
            .own_property(env, export_atom)
            .unwrap_or(Value::UNDEFINED);
        let caller = self
            .own_property(env, caller_atom)
            .unwrap_or(self.realm.globals);
        if self
            .own_property(env, rejected_atom)
            .is_some_and(|value| self.truthy(value))
        {
            return self.with_realm_type_error(p, caller, "ShadowRealm import failed");
        }
        let namespace = args.first().copied().unwrap_or(Value::UNDEFINED);
        let descriptor = self.object_get_own_property_descriptor(p, &[namespace, export_name])?;
        if descriptor.is_undefined() {
            return self.with_realm_type_error(p, caller, "ShadowRealm export not found");
        }
        let Some(Cell::String(export_name)) = self.heap.get(export_name).cloned() else {
            return self.with_realm_type_error(p, caller, "ShadowRealm export not found");
        };
        let export_atom = self.intern_js_atom(&export_name);
        let value = self.get_property(p, namespace, export_atom)?;
        if self.is_function(value) {
            return self.wrap_shadow_callable(p, value, caller);
        }
        if self.is_object_like(value) {
            return self.with_realm_type_error(
                p,
                caller,
                "ShadowRealm imported value must be primitive or callable",
            );
        }
        Ok(value)
    }

    pub(super) fn construct_shadow_realm(
        &mut self,
        p: &ResidualProgram,
        new_target: Value,
    ) -> Result<Value, JsError> {
        let prototype_atom = self.intern_atom("prototype");
        let prototype = self.get_property(p, new_target, prototype_atom)?;
        let prototype = if self.object_data(prototype).is_some() {
            prototype
        } else {
            let realm = self.function_realm(p, new_target)?;
            let constructor_atom = self.intern_atom("ShadowRealm");
            let constructor = self.get_property(p, realm, constructor_atom)?;
            self.get_property(p, constructor, prototype_atom)?
        };
        let caller_global = self.realm.globals;
        let realm_record = self.create_realm(p)?;
        let realm_record_root = self.heap.root(realm_record);
        let global_atom = self.intern_atom("global");
        let realm_global = self.get_property(p, realm_record, global_atom)?;
        let realm_global_root = self.heap.root(realm_global);
        let object = Self::empty_object(prototype);
        let shadow_realm = self.heap.alloc(Cell::ShadowRealm {
            object,
            caller_global,
            realm_global: self
                .heap
                .root_value(realm_global_root)
                .unwrap_or(realm_global),
        });
        self.heap.release_root(realm_record_root);
        self.heap.release_root(realm_global_root);
        Ok(shadow_realm)
    }

    fn call_shadow_wrapped_function(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let env = self.active_native_env().unwrap_or(Value::NULL);
        let target_atom = self.intern_atom("\0rqj:shadow-target");
        let caller_atom = self.intern_atom("\0rqj:shadow-caller");
        let target_realm_atom = self.intern_atom("\0rqj:shadow-target-realm");
        let target = self
            .own_property(env, target_atom)
            .unwrap_or(Value::UNDEFINED);
        let caller = self
            .own_property(env, caller_atom)
            .unwrap_or(self.realm.globals);
        let target_realm = self
            .own_property(env, target_realm_atom)
            .unwrap_or(self.realm.globals);
        if args
            .iter()
            .any(|value| self.is_object_like(*value) && !self.is_function(*value))
        {
            return self.with_realm_type_error(
                p,
                caller,
                "ShadowRealm wrapped function argument must be primitive or callable",
            );
        }
        let mut wrapped_args = Vec::with_capacity(args.len());
        for argument in args.iter().copied() {
            wrapped_args.push(if self.is_function(argument) {
                self.wrap_shadow_callable(p, argument, target_realm)?
            } else {
                argument
            });
        }
        let result = match self.call_value(p, target, Value::UNDEFINED, &wrapped_args) {
            Ok(result) => result,
            Err(_) => {
                return self.with_realm_type_error(
                    p,
                    caller,
                    "ShadowRealm wrapped function threw an exception",
                );
            }
        };
        if self.is_function(result) {
            return self.wrap_shadow_callable(p, result, caller);
        }
        if self.is_object_like(result) {
            return self.with_realm_type_error(
                p,
                caller,
                "ShadowRealm wrapped function must return a primitive",
            );
        }
        Ok(result)
    }

    fn wrap_shadow_callable(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        caller: Value,
    ) -> Result<Value, JsError> {
        let target_root = self.heap.root(target);
        let name_atom = self.intern_atom("name");
        let length_atom = self.intern_atom("length");
        let target_realm = match self.function_realm(p, target) {
            Ok(realm) => realm,
            Err(_) => {
                self.heap.release_root(target_root);
                return self.with_realm_type_error(
                    p,
                    caller,
                    "ShadowRealm wrapped function metadata failed",
                );
            }
        };
        let metadata = (|| {
            let length_key = self.heap.alloc(Cell::String(JsString::from_str("length")));
            let length_descriptor =
                self.object_get_own_property_descriptor(p, &[target, length_key])?;
            let length = if length_descriptor.is_undefined() {
                Value::number(0.0)
            } else {
                let target_length = self.get_property(p, target, length_atom)?;
                match target_length.as_number() {
                    Some(length) if length == f64::INFINITY => target_length,
                    Some(length) if length.is_finite() => Value::number(length.trunc().max(0.0)),
                    _ => Value::number(0.0),
                }
            };
            let name = self.get_property(p, target, name_atom)?;
            let name = if matches!(self.heap.get(name), Some(Cell::String(_))) {
                name
            } else {
                self.heap.alloc(Cell::String(JsString::from_str("")))
            };
            Ok::<_, JsError>((name, length))
        })();
        let (name, length) = match metadata {
            Ok(metadata) => metadata,
            Err(_) => {
                self.heap.release_root(target_root);
                return self.with_realm_type_error(
                    p,
                    caller,
                    "ShadowRealm wrapped function metadata failed",
                );
            }
        };
        let env = self.object();
        let target_atom = self.intern_atom("\0rqj:shadow-target");
        let caller_atom = self.intern_atom("\0rqj:shadow-caller");
        let target_realm_atom = self.intern_atom("\0rqj:shadow-target-realm");
        self.set_property(
            env,
            target_atom,
            self.heap.root_value(target_root).unwrap_or(target),
        )?;
        self.set_property(env, caller_atom, caller)?;
        self.set_property(env, target_realm_atom, target_realm)?;
        let wrapper = self.native_with_realm(Native::ShadowRealmWrappedFunction, env, caller);
        let function_atom = self.intern_atom("Function");
        let constructor = self.get_property(p, caller, function_atom)?;
        let prototype_atom = self.intern_atom("prototype");
        let function_prototype = self.get_property(p, constructor, prototype_atom)?;
        if let Some(object) = self.object_data_mut(wrapper) {
            object.proto = function_prototype;
        }
        self.set_function_metadata_value(wrapper, name_atom, name)?;
        self.set_function_metadata_value(wrapper, length_atom, length)?;
        self.heap.release_root(target_root);
        Ok(wrapper)
    }

    fn set_function_metadata_value(
        &mut self,
        function: Value,
        atom: Atom,
        value: Value,
    ) -> Result<(), JsError> {
        let key = PropertyKey::string(atom);
        self.set_property_attributes(
            function,
            key,
            PropertyAttributes {
                writable: true,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        self.set_property(function, atom, value)?;
        self.set_property_attributes(
            function,
            key,
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        Ok(())
    }

    fn with_realm_type_error(
        &mut self,
        p: &ResidualProgram,
        realm: Value,
        message: &str,
    ) -> Result<Value, JsError> {
        let prior = self.switch_realm_global(realm);
        let error = self.realm_error_value(p, realm, "TypeError", message);
        self.switch_realm_global(prior);
        Err(error)
    }

    fn with_realm_syntax_error(
        &mut self,
        p: &ResidualProgram,
        realm: Value,
        message: String,
    ) -> Result<Value, JsError> {
        let prior = self.switch_realm_global(realm);
        let result = self.realm_error_value(p, realm, "SyntaxError", &message);
        self.switch_realm_global(prior);
        Err(result)
    }

    fn realm_error_value(
        &mut self,
        p: &ResidualProgram,
        realm: Value,
        name: &str,
        message: &str,
    ) -> JsError {
        let constructor_atom = self.intern_atom(name);
        let constructor = self.get_property(p, realm, constructor_atom);
        let message_value = self.heap.alloc(Cell::String(JsString::from_str(message)));
        let error = match constructor {
            Ok(constructor) if self.is_constructable(p, constructor) => self
                .construct_value_with_new_target(p, constructor, constructor, &[message_value])
                .map(|value| JsError::thrown(value, format!("{name}: {message}"))),
            _ => Err(self.type_error(p, message.into())),
        };
        error.unwrap_or_else(|error| error)
    }
}
