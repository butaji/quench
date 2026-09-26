use super::property_key::PropertyKey;
use super::*;

const FUNCTION_PROTOTYPE_LENGTH: f64 = 0.0;

pub(super) fn error_native_length(native: Native) -> Option<f64> {
    Some(match native {
        Native::ErrorIsError => 1.0,
        Native::ErrorStackSetter => 1.0,
        Native::ErrorToString | Native::ErrorStackGetter => 0.0,
        _ => return None,
    })
}
use crate::Value;
use crate::host::{CapabilityId, HostContext};
use std::fmt;

#[derive(Debug)]
pub struct JsError(pub(crate) ErrorMessage);

#[derive(Debug)]
pub(crate) struct ErrorMessage {
    payload: Box<ErrorPayload>,
}

#[derive(Debug)]
struct ErrorPayload {
    text: String,
    thrown: Option<Value>,
    eval_parser_diagnostic: bool,
}

impl From<&str> for ErrorMessage {
    fn from(value: &str) -> Self {
        Self {
            payload: Box::new(ErrorPayload {
                text: value.into(),
                thrown: None,
                eval_parser_diagnostic: false,
            }),
        }
    }
}

impl From<String> for ErrorMessage {
    fn from(value: String) -> Self {
        Self {
            payload: Box::new(ErrorPayload {
                text: value,
                thrown: None,
                eval_parser_diagnostic: false,
            }),
        }
    }
}

impl fmt::Display for JsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.payload.text)
    }
}

impl JsError {
    pub(crate) fn thrown(value: Value, message: String) -> Self {
        Self(ErrorMessage {
            payload: Box::new(ErrorPayload {
                text: message,
                thrown: Some(value),
                eval_parser_diagnostic: false,
            }),
        })
    }

    pub(crate) fn thrown_value(&self) -> Option<Value> {
        self.0.payload.thrown
    }

    pub(crate) fn is_eval_parser_diagnostic(&self) -> bool {
        self.0.payload.eval_parser_diagnostic
    }

    pub(crate) fn mark_eval_parser_diagnostic(mut self) -> Self {
        self.0.payload.eval_parser_diagnostic = true;
        self
    }

    pub(crate) fn validation(message: String) -> Self {
        Self(ErrorMessage::from(format!(
            "invalid residual program: {message}"
        )))
    }

    pub(super) fn into_message(self) -> String {
        self.0.payload.text.clone()
    }
}

impl<H: Host> Vm<H> {
    pub(super) fn error_is_error(&self, value: Value) -> bool {
        if !matches!(self.heap.get(value), Some(Cell::Object(_))) {
            return false;
        }
        self.lookup_atom("\0rqj:error-brand")
            .and_then(|atom| self.own_property(value, atom))
            .is_some_and(|brand| brand == Value::TRUE)
    }

    pub(super) fn error_stack_getter(
        &mut self,
        program: &ResidualProgram,
        receiver: Value,
    ) -> Result<Value, JsError> {
        if !self.is_object_like(receiver) {
            return Err(self.type_error(program, "Error stack getter requires an object".into()));
        }
        if !self.error_is_error(receiver) {
            return Ok(Value::UNDEFINED);
        }
        self.error_to_string(program, receiver)
    }

    pub(super) fn error_stack_setter(
        &mut self,
        program: &ResidualProgram,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if !self.is_object_like(receiver) {
            return Err(self.type_error(program, "Error stack setter requires an object".into()));
        }
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        if !matches!(self.heap.get(value), Some(Cell::String(_))) {
            return Err(self.type_error(program, "Error stack must be a string".into()));
        }
        let error_atom = self.intern_atom("Error");
        let error_constructor = self.get_property(program, self.realm.globals, error_atom)?;
        let prototype_atom = self.intern_atom("prototype");
        let error_prototype = self.get_property(program, error_constructor, prototype_atom)?;
        if receiver == error_prototype {
            return Err(self.type_error(program, "cannot set Error.prototype.stack".into()));
        }
        let key = self.heap.alloc(Cell::String("stack".into()));
        let descriptor = self.object_get_own_property_descriptor(program, &[receiver, key])?;
        if !descriptor.is_undefined() {
            if matches!(self.heap.get(receiver), Some(Cell::Proxy { .. })) {
                let atom = self.intern_atom("stack");
                self.set_property_with_receiver(program, receiver, atom, value, receiver)?;
                return Ok(Value::UNDEFINED);
            }
            let get = self.intern_atom("get");
            let set = self.intern_atom("set");
            let descriptor_getter = self.get_property(program, descriptor, get)?;
            let descriptor_setter = self.get_property(program, descriptor, set)?;
            if !descriptor_getter.is_undefined() || !descriptor_setter.is_undefined() {
                if descriptor_setter.is_undefined() {
                    return Err(self.type_error(program, "cannot set Error stack property".into()));
                }
                self.call_value(program, descriptor_setter, receiver, &[value])?;
                return Ok(Value::UNDEFINED);
            }
            let writable = self.intern_atom("writable");
            let writable = self.get_property(program, descriptor, writable)?;
            if !self.truthy(writable) {
                return Err(self.type_error(program, "cannot set Error stack property".into()));
            }
            let atom = self.intern_atom("stack");
            if !self.set_property_with_receiver(program, receiver, atom, value, receiver)? {
                return Err(self.type_error(program, "cannot set Error stack property".into()));
            }
            return Ok(Value::UNDEFINED);
        }
        let descriptor = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.object_proto)));
        for (name, field_value) in [
            ("value", value),
            ("writable", Value::TRUE),
            ("enumerable", Value::TRUE),
            ("configurable", Value::TRUE),
        ] {
            let atom = self.intern_atom(name);
            self.set_property(descriptor, atom, field_value)?;
        }
        self.object_define_property(program, &[receiver, key, descriptor])?;
        Ok(Value::UNDEFINED)
    }

    pub(super) fn error_to_string(
        &mut self,
        program: &ResidualProgram,
        receiver: Value,
    ) -> Result<Value, JsError> {
        if receiver.is_null() || receiver.is_undefined() {
            return Err(self.type_error(
                program,
                "Error.prototype.toString called on nullish value".into(),
            ));
        }
        if !self.is_object_like(receiver) {
            return Err(self.type_error(
                program,
                "Error.prototype.toString called on non-object value".into(),
            ));
        }
        let name_atom = self.intern_atom("name");
        let message_atom = self.intern_atom("message");
        let name = self.get_property(program, receiver, name_atom)?;
        let name = if name.is_undefined() {
            "Error".to_owned()
        } else {
            self.to_string(program, name)?
        };
        let message = self.get_property(program, receiver, message_atom)?;
        let message = if message.is_undefined() {
            String::new()
        } else {
            self.to_string(program, message)?
        };
        let text = match (name.is_empty(), message.is_empty()) {
            (true, _) => message,
            (false, true) => name,
            (false, false) => format!("{name}: {message}"),
        };
        Ok(self.heap.alloc(Cell::String(JsString::from_str(&text))))
    }

    pub(crate) fn format_error(&mut self, program: &ResidualProgram, error: &JsError) -> String {
        let Some(value) = error.thrown_value() else {
            return error.to_string();
        };
        let Ok(display) = self.to_string(program, value) else {
            return error.to_string();
        };
        if display != "[object Object]" {
            return display;
        }
        let name_atom = self.intern_atom("name");
        let message_atom = self.intern_atom("message");
        let name = self
            .get_property(program, value, name_atom)
            .ok()
            .and_then(|value| match self.heap.get(value) {
                Some(Cell::String(name)) => Some(name.to_string()),
                _ => None,
            })
            .unwrap_or_default();
        let message = self
            .get_property(program, value, message_atom)
            .ok()
            .and_then(|value| match self.heap.get(value) {
                Some(Cell::String(message)) => Some(message.to_string()),
                _ => None,
            })
            .unwrap_or_default();
        match (name.is_empty(), message.is_empty()) {
            (true, true) => display,
            (true, false) => message,
            (false, true) => name,
            (false, false) => format!("{name}: {message}"),
        }
    }

    pub(super) fn type_error(&mut self, program: &ResidualProgram, text: String) -> JsError {
        let message = self.heap.alloc(Cell::String(JsString::from_str(&text)));
        let object = self
            .construct_error_native(program, Native::TypeError, &[message])
            .unwrap_or(Value::UNDEFINED);
        JsError::thrown(object, text)
    }

    pub(super) fn reference_error(&mut self, program: &ResidualProgram, text: String) -> JsError {
        let message = self.heap.alloc(Cell::String(JsString::from_str(&text)));
        let object = self
            .construct_error_native(program, Native::ReferenceError, &[message])
            .unwrap_or(Value::UNDEFINED);
        JsError::thrown(object, format!("ReferenceError: {text}"))
    }

    pub(super) fn range_error(&mut self, program: &ResidualProgram, text: String) -> JsError {
        let message = self.heap.alloc(Cell::String(JsString::from_str(&text)));
        let object = self
            .construct_error_native(program, Native::RangeError, &[message])
            .unwrap_or(Value::UNDEFINED);
        JsError::thrown(object, text)
    }

    pub(super) fn thrown_value_for(&mut self, program: &ResidualProgram, error: JsError) -> Value {
        if let Some(value) = error.thrown_value() {
            return value;
        }
        let text = error.into_message();
        let normalized = text.to_ascii_lowercase();
        let native = if normalized.contains("typeerror")
            || normalized.contains("cannot ")
            || normalized.contains("not callable")
            || normalized.contains("not a constructor")
            || normalized.contains("not an object")
            || normalized.contains("must be ")
            || normalized.contains("requires ")
            || normalized.contains("not iterable")
        {
            Native::TypeError
        } else if normalized.contains("referenceerror")
            || normalized.contains("not defined")
            || normalized.contains("before initialization")
        {
            Native::ReferenceError
        } else if normalized.contains("rangeerror")
            || normalized.contains("out of range")
            || normalized.contains("invalid array length")
        {
            Native::RangeError
        } else if normalized.contains("syntaxerror") {
            Native::SyntaxError
        } else {
            Native::Error
        };
        let message = self.heap.alloc(Cell::String(JsString::from_str(&text)));
        self.construct_error_native(program, native, &[message])
            .unwrap_or(Value::UNDEFINED)
    }

    pub(super) fn box_primitive_object(&mut self, value: Value) -> Result<Value, JsError> {
        let marker = match self.heap.get(value) {
            Some(Cell::String(_)) => "\0rqj:string-value",
            Some(Cell::Symbol(_)) => "\0rqj:symbol-value",
            Some(Cell::BigInt(_)) => "\0rqj:bigint-value",
            _ if value.as_number().is_some() => "\0rqj:number-value",
            _ if value.as_bool().is_some() => "\0rqj:boolean-value",
            _ => return Ok(self.object()),
        };
        let prototype = self.primitive_prototype(value).unwrap_or(self.object_proto);
        let object = self.heap.alloc(Cell::Object(Self::empty_object(prototype)));
        if let Some(Cell::String(text)) = self.heap.get(value).cloned() {
            for (index, unit) in text.units().iter().copied().enumerate() {
                let key = self.intern_atom(&index.to_string());
                let character = self
                    .heap
                    .alloc(Cell::String(super::wtf16::JsString::from_units(&[unit])));
                self.set_property(object, key, character)?;
                self.set_property_attributes(
                    object,
                    PropertyKey::string(key),
                    PropertyAttributes {
                        writable: false,
                        enumerable: true,
                        configurable: false,
                        accessor: false,
                        getter: None,
                        setter: None,
                    },
                );
            }
            let length = self.intern_atom("length");
            self.set_property(object, length, Value::number(text.units().len() as f64))?;
            self.set_property_attributes(
                object,
                PropertyKey::string(length),
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: false,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
        }
        let marker_atom = self.intern_atom(marker);
        self.set_property(object, marker_atom, value)?;
        Ok(object)
    }

    pub(super) fn box_bigint_object(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<Value, JsError> {
        let prototype_atom = self.intern_atom("prototype");
        let constructor_atom = self.intern_atom("BigInt");
        let constructor = self
            .active_native_env()
            .and_then(|global| self.own_property(global, constructor_atom))
            .unwrap_or_else(|| self.native_value(Native::BigInt));
        let prototype = self.get_property(p, constructor, prototype_atom)?;
        let object = self.heap.alloc(Cell::Object(Self::empty_object(prototype)));
        let value_atom = self.intern_atom("\0rqj:bigint-value");
        self.set_property(object, value_atom, value)?;
        Ok(object)
    }

    pub(super) fn install_host_globals(
        &mut self,
        program: &ResidualProgram,
    ) -> Result<(), JsError> {
        self.global(program, "globalThis", self.realm.globals)?;
        let function = self.native_value(Native::Function);
        self.set_builtin_value_named(function, "prototype", self.function_proto)?;
        let prototype = self.intern_atom("prototype");
        self.set_property_attributes(
            function,
            PropertyKey::string(prototype),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: false,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        self.set_builtin_value_named(self.function_proto, "constructor", function)?;
        self.set_builtin_value_named(
            self.function_proto,
            "length",
            Value::number(FUNCTION_PROTOTYPE_LENGTH),
        )?;
        let prototype_length = self.intern_atom("length");
        self.set_property_attributes(
            self.function_proto,
            PropertyKey::string(prototype_length),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        self.set_builtin_function_name(self.function_proto, "")?;
        let length = self.intern_atom("length");
        self.set_builtin_value_named(function, "length", Value::number(1.0))?;
        self.set_property_attributes(
            function,
            PropertyKey::string(length),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        self.set_builtin_function_name(function, "Function")?;
        self.global(program, "Function", function)?;
        let symbol = self.native_value(Native::Symbol);
        let symbol_prototype = self.object();
        self.set_builtin_value_named(symbol, "prototype", symbol_prototype)?;
        self.set_builtin_value_named(symbol_prototype, "constructor", symbol)?;
        self.set_named(
            program,
            symbol_prototype,
            "valueOf",
            self.native_value(Native::SymbolValueOf),
        )?;
        self.set_builtin_named(
            program,
            symbol_prototype,
            "toString",
            Native::SymbolToString,
        )?;
        for native in [Native::String, Native::Number] {
            let constructor = self.native_value(native);
            let prototype = self.object();
            self.set_builtin_value_named(constructor, "prototype", prototype)?;
            self.set_builtin_value_named(prototype, "constructor", constructor)?;
            let value_of = match native {
                Native::String => Native::StringValueOf,
                Native::Number => Native::NumberValueOf,
                _ => unreachable!(),
            };
            self.set_named(program, prototype, "valueOf", self.native_value(value_of))?;
        }
        self.install_boolean(program)?;
        self.install_bigint(program)?;
        for global in self.host.globals() {
            let native = match global.capability {
                CapabilityId::Done => Native::HostDone,
                CapabilityId::CreateRealm => {
                    let realm = self.object();
                    self.set_named(
                        program,
                        realm,
                        "createRealm",
                        self.native_value(Native::CreateRealm),
                    )?;
                    self.set_named(
                        program,
                        realm,
                        "evalScript",
                        self.native_value(Native::EvalScript),
                    )?;
                    self.set_named(
                        program,
                        realm,
                        "detachArrayBuffer",
                        self.native_value(Native::DetachArrayBuffer),
                    )?;
                    let detach_name = self.heap.alloc(Cell::String("detachArrayBuffer".into()));
                    self.set_named(
                        program,
                        self.native_value(Native::DetachArrayBuffer),
                        "name",
                        detach_name,
                    )?;
                    self.set_named(
                        program,
                        realm,
                        "AbstractModuleSource",
                        self.native_value(Native::AbstractModuleSource),
                    )?;
                    self.install_test262_agent(program, realm)?;
                    self.global(program, global.name, realm)?;
                    continue;
                }
                CapabilityId::WriteLine | CapabilityId::ClockMillis => continue,
            };
            self.global(program, global.name, self.native_value(native))?;
        }
        Ok(())
    }

    pub(super) fn install_abstract_module_source(
        &mut self,
        program: &ResidualProgram,
    ) -> Result<(), JsError> {
        let constructor = self.native_value(Native::AbstractModuleSource);
        let prototype = self.object();
        self.set_named(program, constructor, "prototype", prototype)?;
        self.set_named(program, prototype, "constructor", constructor)?;
        let name = self.heap.alloc(Cell::String("AbstractModuleSource".into()));
        self.set_named(program, constructor, "name", name)?;
        let name_atom = self.intern_atom("name");
        let prototype_atom = self.intern_atom("prototype");
        let constructor_atom = self.intern_atom("constructor");
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
        self.set_property_attributes(
            constructor,
            PropertyKey::string(name_atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        let to_string_tag = self
            .well_known_symbols
            .get("toStringTag")
            .copied()
            .expect("well-known toStringTag symbol installed before module source intrinsics");
        self.set_symbol_property(prototype, to_string_tag, Value::UNDEFINED)?;
        self.set_property_attributes(
            prototype,
            PropertyKey::symbol(to_string_tag),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: true,
                getter: Some(self.native_value(Native::AbstractModuleSourceToStringTag)),
                setter: None,
            },
        );
        Ok(())
    }

    pub(super) fn call_host_done(
        &mut self,
        program: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let text = args
            .first()
            .copied()
            .filter(|value| !value.is_undefined())
            .map(|value| self.to_string(program, value))
            .transpose()?;
        HostContext::new(&mut self.host).invoke(CapabilityId::Done, text.as_deref());
        Ok(Value::UNDEFINED)
    }

    pub(super) fn function_native(
        &mut self,
        program: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let function_realm = self.realm.globals;
        let mut argument_strings = Vec::with_capacity(args.len());
        for argument in args {
            let text = self.to_string(program, *argument)?;
            if text.trim().starts_with("#!") {
                let message = self.heap.alloc(Cell::String(
                    "hashbang is not allowed in Function source".into(),
                ));
                let error =
                    self.construct_error_native(program, Native::SyntaxError, &[message])?;
                return Err(JsError::thrown(
                    error,
                    "SyntaxError: hashbang is not allowed in Function source".into(),
                ));
            }
            argument_strings.push(text);
        }
        let source = argument_strings.pop().unwrap_or_default();
        let source = source.trim();
        if let Some(base_name) = dynamic_class_base(source) {
            let base_atom = self.intern_atom(base_name);
            let base_key = self.heap.alloc(Cell::String(JsString::from_str(base_name)));
            if !self.has_property(program, function_realm, base_key)? {
                return Err(self.reference_error(program, format!("{base_name} is not defined")));
            }
            let base = self.get_property(program, function_realm, base_atom)?;
            if !self.is_constructable(program, base) {
                return Err(JsError("dynamic class base is not a constructor".into()));
            }
            return Ok(self.native_with_env(Native::FunctionReturnClass, base));
        }
        let parameters = argument_strings.join(",");
        let parser_parameters = format!("{parameters}\n");
        let source_name = format!("<Function:{}>", self.programs.len());
        let atom_prefix = (0..self.atom_text.len() + self.dynamic_atoms.len())
            .map(|atom| self.atom_name(atom as u32).to_owned())
            .collect::<Vec<_>>();
        let kind = match native {
            Native::AsyncFunction => crate::compile::DynamicFunctionKind::Async,
            Native::GeneratorFunction => crate::compile::DynamicFunctionKind::Generator,
            Native::AsyncGeneratorFunction => crate::compile::DynamicFunctionKind::AsyncGenerator,
            _ => crate::compile::DynamicFunctionKind::Ordinary,
        };
        let residual = crate::Engine::specialize_dynamic_function_with_kind(
            &parser_parameters,
            source,
            &source_name,
            &atom_prefix,
            kind,
        )
        .map_err(|diagnostics| {
            let message = diagnostics
                .first()
                .map_or("invalid Function source".to_owned(), ToString::to_string);
            self.syntax_error_result(program, &message)
                .expect_err("dynamic Function syntax errors must throw")
        })?;
        let Some(program_id) = self.store_dynamic_program(residual) else {
            return Err(self.type_error(program, "dynamic program store is full".into()));
        };
        let residual = self
            .programs
            .get(program_id)
            .ok_or_else(|| self.type_error(program, "dynamic program is unavailable".into()))?;
        let active_program = std::mem::replace(&mut self.active_program, program_id);
        let result = (|| {
            let function = residual
                .functions
                .iter()
                .enumerate()
                .find(|(_, function)| {
                    function.parent == Some(0)
                        && function
                            .name
                            .is_some_and(|name| &residual.atoms[name as usize] == "anonymous")
                })
                .map(|(id, _)| id as u32)
                .ok_or_else(|| {
                    self.type_error(program, "dynamic Function body is unavailable".into())
                })?;
            self.closure_in_realm(&residual, function, Value::NULL, function_realm)
        })();
        self.active_program = active_program;
        let function = result?;
        let function_source = match kind {
            crate::compile::DynamicFunctionKind::Ordinary => {
                format!("function anonymous({parameters}\n) {{\n{source}\n}}")
            }
            crate::compile::DynamicFunctionKind::Async => {
                format!("async function anonymous({parameters}\n) {{\n{source}\n}}")
            }
            crate::compile::DynamicFunctionKind::Generator => {
                format!("function* anonymous({parameters}\n) {{\n{source}\n}}")
            }
            crate::compile::DynamicFunctionKind::AsyncGenerator => {
                format!("async function* anonymous({parameters}\n) {{\n{source}\n}}")
            }
        };
        self.set_function_source(function, &function_source)?;
        Ok(function)
    }

    pub(super) fn dynamic_class_native(&mut self, base: Value) -> Result<Value, JsError> {
        let function = self.native_with_env(Native::DynamicDerivedClass, base);
        let prototype = self.object();
        if let Some(base_prototype_atom) = self.lookup_atom("prototype")
            && let Some(base_prototype) = self.own_property(base, base_prototype_atom)
            && let Some(object) = self.object_data_mut(prototype)
        {
            object.proto = base_prototype;
        }
        let prototype_atom = self.intern_atom("prototype");
        self.set_property(function, prototype_atom, prototype)?;
        self.set_builtin_value_named(prototype, "constructor", function)?;
        if let Some(object) = self.object_data_mut(function) {
            object.proto = base;
        }
        Ok(function)
    }

    pub(super) fn call_function_dispatch(
        &mut self,
        program: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::FunctionReturnThis => {
                Ok(self.active_native_env().unwrap_or(self.realm.globals))
            }
            Native::FunctionReturnClass => {
                let base = self
                    .active_native_env()
                    .ok_or_else(|| JsError("invalid dynamic class environment".into()))?;
                self.dynamic_class_native(base)
            }
            Native::FunctionReturnName => {
                let name = self
                    .active_native_env()
                    .and_then(|value| match self.heap.get(value) {
                        Some(Cell::String(name)) => Some(name.clone()),
                        _ => None,
                    })
                    .ok_or_else(|| JsError("invalid dynamic Function environment".into()))?;
                let atom = self.intern_js_atom(&name);
                self.get_property(program, self.realm.globals, atom)
            }
            Native::DynamicFunction => {
                let environment = self.active_native_env().unwrap_or(Value::NULL);
                if let Some(result) = self.call_eval_super_arrow(program, environment)? {
                    return Ok(result);
                }
                let source = match self.heap.get(environment) {
                    Some(Cell::String(source)) => Some(source.host_string().to_owned()),
                    _ => None,
                }
                .ok_or_else(|| JsError("invalid dynamic Function environment".into()))?;
                let source = source.trim();
                let body_strict =
                    source.starts_with("'use strict';") || source.starts_with("\"use strict\";");
                let source = source
                    .strip_prefix("'use strict';")
                    .or_else(|| source.strip_prefix("\"use strict\";"))
                    .map(str::trim)
                    .unwrap_or(source);
                if let Some(expression) = source
                    .strip_prefix("return ")
                    .map(|expression| expression.trim().trim_end_matches(';').trim())
                {
                    return self.eval_simple_expression(program, expression, body_strict);
                }
                self.eval_source_simple(program, source, body_strict)
            }
            _ => self.function_native(program, native, args),
        }
    }

    pub(super) fn call_host(
        &mut self,
        program: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if native == Native::HostDone {
            self.call_host_done(program, args)
        } else if native == Native::CreateRealm {
            self.create_realm(program)
        } else if native == Native::Boolean {
            Ok(
                if args
                    .first()
                    .copied()
                    .is_some_and(|value| self.truthy(value))
                {
                    Value::TRUE
                } else {
                    Value::FALSE
                },
            )
        } else if native == Native::BigInt {
            self.bigint_constructor(program, args.first().copied())
        } else {
            self.call_function_dispatch(program, native, args)
        }
    }

    fn bigint_constructor(
        &mut self,
        program: &ResidualProgram,
        value: Option<Value>,
    ) -> Result<Value, JsError> {
        let Some(value) = value else {
            return Err(self.type_error(program, "cannot convert value to BigInt".into()));
        };
        let primitive = self.to_primitive(program, value, "number")?;
        if matches!(self.heap.get(primitive), Some(Cell::BigInt(_))) {
            return Ok(primitive);
        }
        if matches!(self.heap.get(primitive), Some(Cell::Symbol(_))) {
            return Err(self.type_error(program, "cannot convert Symbol to BigInt".into()));
        }
        if let Some(number) = primitive.as_number() {
            let Some(integer) = crate::bigint::number_as_bigint(number) else {
                return Err(self.range_error(
                    program,
                    "cannot convert non-integral Number to BigInt".into(),
                ));
            };
            return Ok(self.heap.alloc(Cell::BigInt(integer.to_string())));
        }
        if let Some(value) = primitive.as_bool() {
            return Ok(self.heap.alloc(Cell::BigInt(i32::from(value).to_string())));
        }
        if let Some(Cell::String(text)) = self.heap.get(primitive) {
            let parsed = crate::bigint::parse_string(&text.host_string());
            return match parsed {
                Some(value) => Ok(self.heap.alloc(Cell::BigInt(value.to_string()))),
                None => self.syntax_error_result(program, "invalid BigInt value"),
            };
        }
        Err(self.type_error(program, "cannot convert value to BigInt".into()))
    }

    fn create_realm(&mut self, program: &ResidualProgram) -> Result<Value, JsError> {
        let global = self.object();
        let type_error = self.native_with_realm(Native::RealmTypeError, global, global);
        let error_prototype = self
            .lookup_atom("prototype")
            .and_then(|atom| self.own_property(self.native_value(Native::TypeError), atom))
            .unwrap_or(Value::NULL);
        let type_error_prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(error_prototype)));
        self.set_named(program, type_error, "prototype", type_error_prototype)?;
        self.set_named(program, type_error_prototype, "constructor", type_error)?;
        let type_error_name = self.heap.alloc(Cell::String("TypeError".into()));
        self.set_named(program, type_error_prototype, "name", type_error_name)?;
        self.set_named(program, global, "TypeError", type_error)?;
        let eval = self.native_with_realm(Native::Eval, global, global);
        self.set_named(program, global, "eval", eval)?;
        let function = self.native_with_realm(Native::Function, global, global);
        self.set_named(program, global, "Function", function)?;
        let object_prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(Value::NULL)));
        self.install_number_for_realm(program, global, object_prototype)?;
        let object_constructor = self.native_with_realm(Native::Object, global, global);
        self.set_builtin_function_name(object_constructor, "Object")?;
        self.set_builtin_value_named(object_constructor, "prototype", object_prototype)?;
        self.set_builtin_value_named(object_prototype, "constructor", object_constructor)?;
        self.install_function_prototype_for_realm(global, object_prototype, function)?;
        let object_name = self.intern_atom("Object");
        self.set_property(global, object_name, object_constructor)?;
        self.set_property_attributes(
            global,
            PropertyKey::string(object_name),
            PropertyAttributes {
                writable: true,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        let proxy = self.native_with_realm(Native::Proxy, global, global);
        self.set_builtin_function_name(proxy, "Proxy")?;
        let revocable = self.native_with_realm(Native::ProxyRevocable, global, global);
        self.set_builtin_function_name(revocable, "revocable")?;
        self.set_builtin_value_named(proxy, "revocable", revocable)?;
        self.set_builtin_value_named(global, "Proxy", proxy)?;
        let boolean = self.native_with_realm(Native::Boolean, global, global);
        self.install_boolean_for_realm(program, global, object_prototype, boolean)?;
        let bigint = self.native_with_realm(Native::BigInt, global, global);
        self.install_bigint_for_realm(program, global, object_prototype, bigint)?;
        let data_view = self.native_with_realm(Native::DataView, global, global);
        let data_view_prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
        self.install_data_view_for_realm(program, global, data_view, data_view_prototype)?;
        let date = self.native_with_realm(Native::Date, global, global);
        let date_prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
        self.install_date_for_realm(program, global, date, date_prototype)?;
        let promise = self.native_with_realm(Native::Promise, global, global);
        self.set_builtin_function_name(promise, "Promise")?;
        let promise_prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
        self.set_builtin_value_named(promise, "prototype", promise_prototype)?;
        self.set_builtin_value_named(promise_prototype, "constructor", promise)?;
        self.set_builtin_value_named(global, "Promise", promise)?;
        self.install_disposal_for_realm(program, global, object_prototype)?;
        self.install_finalization_registry_for_realm(global, object_prototype)?;
        let realm_iterator_proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
        self.install_iterator_constructor(global, realm_iterator_proto, Some(global))?;
        self.install_iterator_prototype(realm_iterator_proto, Some(global))?;
        let realm_iterator_helper_proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(realm_iterator_proto)));
        let realm_wrap_for_valid_iterator_proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(realm_iterator_proto)));
        self.install_iterator_helper_prototype(realm_iterator_helper_proto, Some(global))?;
        self.install_wrap_for_valid_iterator_prototype(
            realm_wrap_for_valid_iterator_proto,
            Some(global),
        )?;
        let realm_generator_proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(realm_iterator_proto)));
        let realm_async_iterator_proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(realm_iterator_proto)));
        self.install_async_iterator_prototype(
            realm_async_iterator_proto,
            realm_iterator_proto,
            Some(global),
        )?;
        let realm_async_generator_proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(realm_async_iterator_proto)));
        self.set_builtin_value_named(
            realm_async_generator_proto,
            "next",
            self.native_value(Native::IteratorNext),
        )?;
        self.iterator_realm_prototypes.insert(
            global,
            IteratorRealmPrototypes {
                helper: realm_iterator_helper_proto,
                wrapper: realm_wrap_for_valid_iterator_proto,
                generator: realm_generator_proto,
                async_generator: realm_async_generator_proto,
            },
        );
        for (name, native, prototype) in [
            (
                "AsyncFunction",
                Native::AsyncFunction,
                self.heap
                    .alloc(Cell::Object(Self::empty_object(self.function_proto))),
            ),
            (
                "GeneratorFunction",
                Native::GeneratorFunction,
                self.heap
                    .alloc(Cell::Object(Self::empty_object(self.function_proto))),
            ),
            (
                "AsyncGeneratorFunction",
                Native::AsyncGeneratorFunction,
                self.heap
                    .alloc(Cell::Object(Self::empty_object(self.function_proto))),
            ),
        ] {
            let constructor = self.native_with_realm(native, global, global);
            self.object_data_mut(constructor)
                .expect("realm dynamic function")
                .proto = function;
            self.set_builtin_value_named(constructor, "prototype", prototype)?;
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
            self.set_builtin_value_named(prototype, "constructor", constructor)?;
            if native == Native::GeneratorFunction {
                self.install_generator_prototype(realm_generator_proto, prototype, Some(global))?;
                self.set_builtin_value_named(prototype, "prototype", realm_generator_proto)?;
                let prototype_atom = self.intern_atom("prototype");
                self.set_property_attributes(
                    prototype,
                    PropertyKey::string(prototype_atom),
                    PropertyAttributes {
                        writable: false,
                        enumerable: false,
                        configurable: true,
                        accessor: false,
                        getter: None,
                        setter: None,
                    },
                );
            }
            if matches!(
                native,
                Native::GeneratorFunction | Native::AsyncGeneratorFunction
            ) {
                let constructor_atom = self.intern_atom("constructor");
                self.set_property_attributes(
                    prototype,
                    PropertyKey::string(constructor_atom),
                    PropertyAttributes {
                        writable: false,
                        enumerable: false,
                        configurable: true,
                        accessor: false,
                        getter: None,
                        setter: None,
                    },
                );
            }
            if native == Native::AsyncGeneratorFunction {
                self.set_builtin_value_named(prototype, "prototype", realm_async_generator_proto)?;
                let prototype_atom = self.intern_atom("prototype");
                self.set_property_attributes(
                    prototype,
                    PropertyKey::string(prototype_atom),
                    PropertyAttributes {
                        writable: false,
                        enumerable: false,
                        configurable: true,
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
                        writable: false,
                        enumerable: false,
                        configurable: true,
                        accessor: false,
                        getter: None,
                        setter: None,
                    },
                );
            }
            self.set_builtin_value_named(global, name, constructor)?;
            self.install_builtin_to_string_tag(prototype, name)?;
        }
        let async_generator_function_atom = self.intern_atom("AsyncGeneratorFunction");
        let prototype_atom = self.intern_atom("prototype");
        let async_generator_function = self
            .own_property(global, async_generator_function_atom)
            .unwrap_or(self.native_value(Native::AsyncGeneratorFunction));
        let async_generator_function_prototype = self
            .own_property(async_generator_function, prototype_atom)
            .unwrap_or(self.function_proto);
        self.install_async_generator_prototype(
            realm_async_generator_proto,
            async_generator_function_prototype,
            Some(global),
        )?;
        let object = self.native_with_realm(Native::Object, global, global);
        self.set_named(program, object, "prototype", object_prototype)?;
        self.set_named(program, object_prototype, "constructor", object)?;
        for (name, native) in [
            ("defineProperty", Native::ObjectDefineProperty),
            ("setPrototypeOf", Native::ObjectSetPrototypeOf),
            ("create", Native::ObjectCreate),
            (
                "getOwnPropertyDescriptor",
                Native::ObjectGetOwnPropertyDescriptor,
            ),
        ] {
            let method = self.native_with_realm(native, global, global);
            self.set_named(program, object, name, method)?;
        }
        self.set_named(program, global, "Object", object)?;
        let array = self.native_with_realm(Native::Array, global, global);
        let array_prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.array_proto)));
        self.set_builtin_value_named(array, "prototype", array_prototype)?;
        self.set_builtin_value_named(array_prototype, "constructor", array)?;
        if let Some(species) = self.well_known_symbols.get("species").copied() {
            let getter = self.native_with_realm(Native::ArraySpecies, global, global);
            self.set_symbol_property(array, species, Value::UNDEFINED)?;
            self.set_property_attributes(
                array,
                PropertyKey::symbol(species),
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: true,
                    accessor: true,
                    getter: Some(getter),
                    setter: None,
                },
            );
        }
        self.set_named(program, global, "Array", array)?;
        let map = self.native_with_realm(Native::Map, global, global);
        let map_prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
        self.set_builtin_function_name(map, "Map")?;
        self.set_builtin_value_named(map, "prototype", map_prototype)?;
        self.set_builtin_value_named(map_prototype, "constructor", map)?;
        let prototype_atom = self.intern_atom("prototype");
        self.set_property_attributes(
            map,
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
        self.set_builtin_value_named(global, "Map", map)?;
        let regexp = self.native_with_realm(Native::RegExp, global, global);
        let regexp_escape = self.native_with_realm(Native::RegExpEscape, global, global);
        self.set_builtin_function_name(regexp_escape, "escape")?;
        self.set_builtin_value_named(regexp, "escape", regexp_escape)?;
        let regexp_prototype = self.heap.alloc(Cell::RegExp {
            object: Self::empty_object(object_prototype),
            source: JsString::from_str("(?:)"),
            flags: String::new(),
        });
        self.set_builtin_value_named(regexp, "prototype", regexp_prototype)?;
        self.set_builtin_value_named(regexp_prototype, "constructor", regexp)?;
        self.install_regexp_symbol_properties(regexp, regexp_prototype, global)?;
        let regexp_name = self.heap.alloc(Cell::String("RegExp".into()));
        self.set_builtin_value_named(regexp, "name", regexp_name)?;
        self.set_builtin_value_named(global, "RegExp", regexp)?;
        let async_disposable_stack =
            self.native_with_realm(Native::AsyncDisposableStack, global, global);
        let async_disposable_stack_prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
        self.set_builtin_value_named(
            async_disposable_stack,
            "prototype",
            async_disposable_stack_prototype,
        )?;
        self.set_builtin_value_named(
            async_disposable_stack_prototype,
            "constructor",
            async_disposable_stack,
        )?;
        self.set_builtin_value_named(global, "AsyncDisposableStack", async_disposable_stack)?;
        let array_buffer = self.native_with_realm(Native::ArrayBuffer, global, global);
        let array_buffer_prototype = self.object();
        self.object_data_mut(array_buffer_prototype)
            .expect("realm ArrayBuffer prototype")
            .proto = self.array_buffer_proto;
        self.set_builtin_value_named(array_buffer, "prototype", array_buffer_prototype)?;
        self.set_builtin_value_named(array_buffer_prototype, "constructor", array_buffer)?;
        let array_buffer_name = self.heap.alloc(Cell::String("ArrayBuffer".into()));
        self.set_builtin_value_named(array_buffer, "name", array_buffer_name)?;
        self.set_named(program, global, "ArrayBuffer", array_buffer)?;
        for (name, native) in [
            ("Number", Native::Number),
            ("String", Native::String),
            ("Boolean", Native::Boolean),
            ("Symbol", Native::Symbol),
        ] {
            let constructor = self.native_with_realm(native, global, global);
            let prototype = self.object();
            self.set_builtin_value_named(constructor, "prototype", prototype)?;
            self.set_builtin_value_named(prototype, "constructor", constructor)?;
            self.set_builtin_value_named(global, name, constructor)?;
        }
        let realm_error_prototype = self.object();
        let realm_error_constructor = self.native_with_realm(Native::Error, global, global);
        self.set_builtin_value_named(realm_error_constructor, "prototype", realm_error_prototype)?;
        self.set_builtin_value_named(
            realm_error_prototype,
            "constructor",
            realm_error_constructor,
        )?;
        let realm_error_name = self.heap.alloc(Cell::String("Error".into()));
        self.set_builtin_value_named(realm_error_prototype, "name", realm_error_name)?;
        let realm_empty_message = self.heap.alloc(Cell::String("".into()));
        self.set_builtin_value_named(realm_error_prototype, "message", realm_empty_message)?;
        let realm_is_error = self.native_with_realm(Native::ErrorIsError, global, global);
        self.set_builtin_function_name(realm_is_error, "isError")?;
        self.set_builtin_value_named(realm_error_constructor, "isError", realm_is_error)?;
        let stack_getter = self.native_with_realm(Native::ErrorStackGetter, global, global);
        let stack_setter = self.native_with_realm(Native::ErrorStackSetter, global, global);
        self.set_builtin_function_name(stack_getter, "get stack")?;
        self.set_builtin_function_name(stack_setter, "set stack")?;
        let stack_atom = self.intern_atom("stack");
        self.set_property(realm_error_prototype, stack_atom, Value::UNDEFINED)?;
        self.set_property_attributes(
            realm_error_prototype,
            PropertyKey::string(stack_atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: true,
                getter: Some(stack_getter),
                setter: Some(stack_setter),
            },
        );
        self.set_builtin_value_named(global, "Error", realm_error_constructor)?;
        for (name, native) in [
            ("AggregateError", Native::AggregateError),
            ("SuppressedError", Native::SuppressedError),
            ("EvalError", Native::EvalError),
            ("RangeError", Native::RangeError),
            ("ReferenceError", Native::ReferenceError),
            ("SyntaxError", Native::SyntaxError),
            ("URIError", Native::URIError),
        ] {
            let constructor = self.native_with_realm(native, global, global);
            let prototype = self
                .heap
                .alloc(Cell::Object(Self::empty_object(realm_error_prototype)));
            self.set_builtin_value_named(constructor, "prototype", prototype)?;
            self.set_builtin_value_named(prototype, "constructor", constructor)?;
            let name_value = self.heap.alloc(Cell::String(name.into()));
            self.set_builtin_value_named(prototype, "name", name_value)?;
            let empty_message = self.heap.alloc(Cell::String("".into()));
            self.set_builtin_value_named(prototype, "message", empty_message)?;
            self.set_builtin_value_named(global, name, constructor)?;
            self.object_data_mut(constructor).unwrap().proto = realm_error_constructor;
        }
        for (name, native) in [
            ("parseFloat", Native::NumberParseFloat),
            ("parseInt", Native::ParseInt),
        ] {
            let intrinsic = self.native_with_realm(native, global, global);
            self.set_named(program, global, name, intrinsic)?;
        }
        let realm = self.object();
        self.set_named(program, realm, "global", global)?;
        let eval_script = self.native_with_realm(Native::EvalScript, global, global);
        self.set_builtin_function_name(eval_script, "evalScript")?;
        self.set_builtin_value_named(realm, "evalScript", eval_script)?;
        Ok(realm)
    }

    fn install_function_prototype_for_realm(
        &mut self,
        global: Value,
        object_prototype: Value,
        constructor: Value,
    ) -> Result<(), JsError> {
        self.set_builtin_function_name(constructor, "Function")?;
        self.set_builtin_value_named(constructor, "length", Value::number(1.0))?;
        let constructor_length = self.intern_atom("length");
        self.set_property_attributes(
            constructor,
            PropertyKey::string(constructor_length),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        let prototype = self.heap.alloc(Cell::Function {
            object: Box::new(Self::empty_object(object_prototype)),
            kind: FunctionKind::Native(Native::FunctionPrototype),
            env: Value::NULL,
            realm: global,
        });
        self.set_builtin_value_named(constructor, "prototype", prototype)?;
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
        self.set_builtin_value_named(prototype, "constructor", constructor)?;
        self.set_builtin_value_named(prototype, "length", Value::number(0.0))?;
        let length = self.intern_atom("length");
        self.set_property_attributes(
            prototype,
            PropertyKey::string(length),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        self.set_builtin_function_name(prototype, "")?;
        for (name, native) in [
            ("call", Native::FunctionCall),
            ("apply", Native::FunctionApply),
            ("bind", Native::FunctionBind),
            ("toString", Native::FunctionToString),
        ] {
            let method = self.native_with_realm(native, global, global);
            self.set_builtin_function_name(method, name)?;
            self.set_builtin_value_named(prototype, name, method)?;
        }
        if let Some(symbol) = self.well_known_symbols.get("hasInstance").copied() {
            let method =
                self.native_with_realm(Native::FunctionPrototypeHasInstance, global, global);
            self.set_builtin_function_name(method, "[Symbol.hasInstance]")?;
            self.set_symbol_property(prototype, symbol, method)?;
            self.set_property_attributes(
                prototype,
                PropertyKey::symbol(symbol),
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: false,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
        }
        let throw_type_error = self.native_value(Native::ThrowTypeError);
        for key in ["caller", "arguments"] {
            let atom = self.intern_atom(key);
            self.set_property(prototype, atom, Value::UNDEFINED)?;
            self.set_property_attributes(
                prototype,
                PropertyKey::string(atom),
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
        let name = self.intern_atom("Function");
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

    pub(super) fn install_errors(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let constructors = [
            ("Error", Native::Error),
            ("AggregateError", Native::AggregateError),
            ("SuppressedError", Native::SuppressedError),
            ("EvalError", Native::EvalError),
            ("RangeError", Native::RangeError),
            ("ReferenceError", Native::ReferenceError),
            ("SyntaxError", Native::SyntaxError),
            ("TypeError", Native::TypeError),
            ("URIError", Native::URIError),
        ];
        let error_prototype = self.object();
        for (index, (name, native)) in constructors.iter().enumerate() {
            let constructor = self.native_value(*native);
            let prototype = if index == 0 {
                error_prototype
            } else {
                self.heap
                    .alloc(Cell::Object(Self::empty_object(error_prototype)))
            };
            self.set_named(program, constructor, "prototype", prototype)?;
            self.set_builtin_value_named(prototype, "constructor", constructor)?;
            let constructor_name = self.heap.alloc(Cell::String(JsString::from_str(name)));
            self.set_named(program, constructor, "name", constructor_name)?;
            let name_atom = self.intern_atom("name");
            self.set_property_attributes(
                constructor,
                PropertyKey::string(name_atom),
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: true,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
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
            let name_value = self.heap.alloc(Cell::String(JsString::from_str(name)));
            self.set_builtin_value_named(prototype, "name", name_value)?;
            let empty_message = self.heap.alloc(Cell::String(JsString::from_str("")));
            self.set_builtin_value_named(prototype, "message", empty_message)?;
            self.global(program, name, constructor)?;
        }
        let error_constructor = self.native_value(Native::Error);
        for (_, native) in constructors.iter().skip(1) {
            let constructor = self.native_value(*native);
            if let Some(function) = self.object_data_mut(constructor) {
                function.proto = error_constructor;
            }
        }
        if let Some(error) = self
            .lookup_atom("Error")
            .and_then(|atom| self.own_property(self.realm.globals, atom))
            && let Some(aggregate) = self
                .lookup_atom("AggregateError")
                .and_then(|atom| self.own_property(self.realm.globals, atom))
        {
            self.object_data_mut(aggregate).unwrap().proto = error;
        }
        self.set_builtin_named(program, error_prototype, "toString", Native::ErrorToString)?;
        let error_constructor = self.native_value(Native::Error);
        self.set_builtin_named(program, error_constructor, "isError", Native::ErrorIsError)?;
        let stack_getter = self.native_value(Native::ErrorStackGetter);
        let stack_setter = self.native_value(Native::ErrorStackSetter);
        self.set_builtin_function_name(stack_getter, "get stack")?;
        self.set_builtin_function_name(stack_setter, "set stack")?;
        let stack_atom = self.intern_atom("stack");
        self.set_property(error_prototype, stack_atom, Value::UNDEFINED)?;
        self.set_property_attributes(
            error_prototype,
            PropertyKey::string(stack_atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: true,
                getter: Some(stack_getter),
                setter: Some(stack_setter),
            },
        );
        let realm_constructor = self.native_value(Native::RealmTypeError);
        let realm_prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(error_prototype)));
        self.set_named(program, realm_constructor, "prototype", realm_prototype)?;
        self.set_named(program, realm_prototype, "constructor", realm_constructor)?;
        let realm_name = self
            .heap
            .alloc(Cell::String(JsString::from_str("TypeError")));
        self.set_named(program, realm_prototype, "name", realm_name)?;
        Ok(())
    }

    pub(super) fn construct_error_native(
        &mut self,
        program: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let prototype_atom = self.intern_atom("prototype");
        let constructor = if matches!(native, Native::TypeError | Native::RealmTypeError) {
            self.lookup_atom("TypeError")
                .and_then(|atom| self.own_property(self.realm.globals, atom))
                .unwrap_or_else(|| self.native_value(native))
        } else {
            self.native_value(native)
        };
        let prototype = self
            .own_property(constructor, prototype_atom)
            .unwrap_or(self.object_proto);
        let object = self.heap.alloc(Cell::Object(Self::empty_object(prototype)));
        self.set_builtin_value_named(object, "\0rqj:error-brand", Value::TRUE)?;
        if native == Native::SuppressedError {
            for (name, value) in [
                ("error", args.first().copied().unwrap_or(Value::UNDEFINED)),
                (
                    "suppressed",
                    args.get(1).copied().unwrap_or(Value::UNDEFINED),
                ),
            ] {
                self.set_builtin_value_named(object, name, value)?;
            }
            if let Some(message) = args.get(2).copied().filter(|value| !value.is_undefined()) {
                let message = self.to_string(program, message)?;
                let message = self.heap.alloc(Cell::String(JsString::from_str(&message)));
                self.set_builtin_value_named(object, "message", message)?;
            }
            return Ok(object);
        }
        if let Some(value) = args.first().copied().filter(|value| !value.is_undefined()) {
            let message = self.to_string(program, value)?;
            let message_value = self.heap.alloc(Cell::String(JsString::from_str(&message)));
            self.set_named(program, object, "message", message_value)?;
            let message_atom = self.intern_atom("message");
            self.set_property_attributes(
                object,
                PropertyKey::string(message_atom),
                PropertyAttributes {
                    writable: true,
                    enumerable: false,
                    configurable: true,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
        }
        if let Some(options) = args
            .get(1)
            .copied()
            .filter(|value| self.is_object_like(*value))
        {
            let cause_atom = self.intern_atom("cause");
            let cause_key = self.heap.alloc(Cell::String("cause".into()));
            if self.has_property(program, options, cause_key)? {
                let cause = self.get_property(program, options, cause_atom)?;
                self.set_builtin_value_named(object, "cause", cause)?;
            }
        }
        Ok(object)
    }

    pub(super) fn construct_aggregate_error(
        &mut self,
        program: &ResidualProgram,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        let message = args.get(1).copied().filter(|value| !value.is_undefined());
        let message = message
            .map(|message| self.to_string(program, message))
            .transpose()?;
        let errors =
            self.spread_to_array(program, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let prototype_atom = self.intern_atom("prototype");
        let realm = match self.heap.get(new_target) {
            Some(Cell::Function { realm, .. }) => *realm,
            _ => self.realm.globals,
        };
        let constructor_atom = self.intern_atom("AggregateError");
        let constructor = self
            .own_property(realm, constructor_atom)
            .unwrap_or_else(|| self.native_value(Native::AggregateError));
        let prototype = self
            .own_property(constructor, prototype_atom)
            .unwrap_or(self.object_proto);
        let object = self.heap.alloc(Cell::Object(Self::empty_object(prototype)));
        self.set_builtin_value_named(object, "\0rqj:error-brand", Value::TRUE)?;
        self.set_builtin_value_named(object, "errors", errors)?;
        if let Some(message) = message {
            let message = self.heap.alloc(Cell::String(JsString::from_str(&message)));
            self.set_builtin_value_named(object, "message", message)?;
        }
        if let Some(options) = args
            .get(2)
            .copied()
            .filter(|value| self.is_object_like(*value))
        {
            let cause_atom = self.intern_atom("cause");
            let cause_key = self.heap.alloc(Cell::String("cause".into()));
            if self.has_property(program, options, cause_key)? {
                let cause = self.get_property(program, options, cause_atom)?;
                self.set_builtin_value_named(object, "cause", cause)?;
            }
        }
        Ok(object)
    }
}

fn dynamic_class_base(source: &str) -> Option<&str> {
    let rest = source.strip_prefix("return class ")?;
    let (_, rest) = rest.split_once(" extends ")?;
    let end = rest.find([' ', '{', '('])?;
    let name = &rest[..end];
    (!name.is_empty()).then_some(name)
}
