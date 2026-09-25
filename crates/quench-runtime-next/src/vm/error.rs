use super::property_key::PropertyKey;
use super::*;

const FUNCTION_PROTOTYPE_LENGTH: f64 = 0.0;
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
        let (constructor, marker) = match self.heap.get(value) {
            Some(Cell::String(_)) => (Native::String, "\0rqj:string-value"),
            Some(Cell::Symbol(_)) => (Native::Symbol, "\0rqj:symbol-value"),
            Some(Cell::BigInt(_)) => (Native::BigInt, "\0rqj:bigint-value"),
            _ if value.as_number().is_some() => (Native::Number, "\0rqj:number-value"),
            _ if value.as_bool().is_some() => (Native::Boolean, "\0rqj:boolean-value"),
            _ => return Ok(self.object()),
        };
        let prototype_atom = self.intern_atom("prototype");
        let prototype = self
            .own_property(self.native_value(constructor), prototype_atom)
            .unwrap_or(self.object_proto);
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
        let prototype = self.get_property(p, self.native_value(Native::BigInt), prototype_atom)?;
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
        self.set_named(program, function, "prototype", self.function_proto)?;
        self.set_named(program, self.function_proto, "constructor", function)?;
        self.set_named(
            program,
            self.function_proto,
            "length",
            Value::number(FUNCTION_PROTOTYPE_LENGTH),
        )?;
        let length = self.intern_atom("length");
        self.set_named(program, function, "length", Value::number(1.0))?;
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
        self.global(program, "Function", function)?;
        let symbol = self.native_value(Native::Symbol);
        let symbol_prototype = self.object();
        self.set_named(program, symbol, "prototype", symbol_prototype)?;
        self.set_named(program, symbol_prototype, "constructor", symbol)?;
        self.set_named(
            program,
            symbol_prototype,
            "valueOf",
            self.native_value(Native::SymbolValueOf),
        )?;
        for native in [Native::String, Native::Number] {
            let constructor = self.native_value(native);
            let prototype = self.object();
            self.set_named(program, constructor, "prototype", prototype)?;
            self.set_named(program, prototype, "constructor", constructor)?;
            let value_of = match native {
                Native::String => Native::StringValueOf,
                Native::Number => Native::NumberValueOf,
                _ => unreachable!(),
            };
            self.set_named(program, prototype, "valueOf", self.native_value(value_of))?;
        }
        let boolean = self.native_value(Native::Boolean);
        let boolean_prototype = self.object();
        self.set_named(program, boolean, "prototype", boolean_prototype)?;
        self.set_named(program, boolean_prototype, "constructor", boolean)?;
        self.set_named(
            program,
            boolean_prototype,
            "toString",
            self.native_value(Native::BooleanToString),
        )?;
        self.set_named(
            program,
            boolean_prototype,
            "valueOf",
            self.native_value(Native::BooleanValueOf),
        )?;
        self.global(program, "Boolean", boolean)?;
        let bigint = self.native_value(Native::BigInt);
        let bigint_prototype = self.object();
        self.set_named(program, bigint, "prototype", bigint_prototype)?;
        self.set_named(program, bigint_prototype, "constructor", bigint)?;
        self.set_named(
            program,
            bigint_prototype,
            "valueOf",
            self.native_value(Native::BigIntValueOf),
        )?;
        self.global(program, "BigInt", bigint)?;
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
                        "AbstractModuleSource",
                        self.native_value(Native::AbstractModuleSource),
                    )?;
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
        args: &[Value],
    ) -> Result<Value, JsError> {
        let function_realm = self
            .active_native_env()
            .filter(|global| self.object_data(*global).is_some())
            .unwrap_or(self.realm.globals);
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
        if source == "return this;" {
            return Ok(self.native_with_env(Native::FunctionReturnThis, function_realm));
        }
        let parameters = argument_strings.join(",");
        let source_name = format!("<Function:{}>", self.programs.len());
        let atom_prefix = (0..self.atom_text.len() + self.dynamic_atoms.len())
            .map(|atom| self.atom_name(atom as u32).to_owned())
            .collect::<Vec<_>>();
        let residual = crate::Engine::specialize_dynamic_function(
            &parameters,
            source,
            &source_name,
            &atom_prefix,
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
        result
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
        let constructor_atom = self.intern_atom("constructor");
        self.set_property(prototype, constructor_atom, function)?;
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
            _ => self.function_native(program, args),
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
            let value = args.first().copied().unwrap_or(Value::UNDEFINED);
            if let Some(Cell::BigInt(_)) = self.heap.get(value) {
                Ok(value)
            } else {
                let text = self.to_string(program, value)?;
                if text.trim().parse::<i128>().is_err() {
                    return Err(JsError("invalid BigInt value".into()));
                }
                Ok(self.heap.alloc(Cell::BigInt(text)))
            }
        } else {
            self.call_function_dispatch(program, native, args)
        }
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
        let object = self.native_with_realm(Native::Object, global, global);
        let object_prototype = self.object();
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
        for (name, native) in [
            ("parseFloat", Native::NumberParseFloat),
            ("parseInt", Native::ParseInt),
        ] {
            let intrinsic = self.native_with_realm(native, global, global);
            self.set_named(program, global, name, intrinsic)?;
        }
        let realm = self.object();
        self.set_named(program, realm, "global", global)?;
        Ok(realm)
    }

    pub(super) fn install_errors(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let constructors = [
            ("Error", Native::Error),
            ("AggregateError", Native::AggregateError),
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
            self.set_named(program, prototype, "constructor", constructor)?;
            let name_value = self.heap.alloc(Cell::String(JsString::from_str(name)));
            self.set_named(program, prototype, "name", name_value)?;
            self.global(program, name, constructor)?;
        }
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
