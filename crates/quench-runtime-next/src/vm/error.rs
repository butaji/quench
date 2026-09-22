use super::*;
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
}

impl From<&str> for ErrorMessage {
    fn from(value: &str) -> Self {
        Self {
            payload: Box::new(ErrorPayload {
                text: value.into(),
                thrown: None,
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
            }),
        })
    }

    pub(crate) fn thrown_value(&self) -> Option<Value> {
        self.0.payload.thrown
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
        JsError::thrown(object, text)
    }

    pub(super) fn thrown_value_for(&mut self, program: &ResidualProgram, error: JsError) -> Value {
        if let Some(value) = error.thrown_value() {
            return value;
        }
        let text = error.into_message();
        let native = if text.starts_with("cannot ")
            || text.contains("not callable")
            || text.contains("must be ")
            || text.contains("requires ")
        {
            Native::TypeError
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
            }
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
        self.global(program, "globalThis", self.globals)?;
        let function = self.native_value(Native::Function);
        self.set_named(program, function, "prototype", self.function_proto)?;
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
                    self.global(program, global.name, realm)?;
                    continue;
                }
                CapabilityId::WriteLine | CapabilityId::ClockMillis => continue,
            };
            self.global(program, global.name, self.native_value(native))?;
        }
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
        for argument in args {
            let text = self.to_string(program, *argument)?;
            if text.trim().starts_with("#!") {
                let message = self
                    .heap
                    .alloc(Cell::String("hashbang is not allowed in Function source".into()));
                let error = self.construct_error_native(program, Native::SyntaxError, &[message])?;
                return Err(JsError::thrown(
                    error,
                    "SyntaxError: hashbang is not allowed in Function source".into(),
                ));
            }
        }
        let source = args.last().copied().unwrap_or(Value::UNDEFINED);
        let source = self.to_string(program, source)?;
        let source = source.trim();
        if source == "return this;" {
            return Ok(self.native_with_env(Native::FunctionReturnThis, Value::NULL));
        }
        let Some(name) = source
            .strip_prefix("return ")
            .map(|name| name.trim().trim_end_matches(';').trim())
            .filter(|name| {
                !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
            })
        else {
            return Err(JsError("dynamic Function source is unsupported".into()));
        };
        let name = self.heap.alloc(Cell::String(JsString::from_str(name)));
        Ok(self.native_with_env(Native::FunctionReturnName, name))
    }

    pub(super) fn call_function_dispatch(
        &mut self,
        program: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::FunctionReturnThis => Ok(self.globals),
            Native::FunctionReturnName => {
                let name = self
                    .active_native_env()
                    .and_then(|value| match self.heap.get(value) {
                        Some(Cell::String(name)) => Some(name.clone()),
                        _ => None,
                    })
                    .ok_or_else(|| JsError("invalid dynamic Function environment".into()))?;
                let atom = self.intern_js_atom(&name);
                self.get_property(program, self.globals, atom)
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
        self.set_named(
            program,
            global,
            "TypeError",
            self.native_value(Native::RealmTypeError),
        )?;
        let realm = self.object();
        self.set_named(program, realm, "global", global)?;
        Ok(realm)
    }

    pub(super) fn install_errors(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let constructors = [
            ("Error", Native::Error),
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
        let constructor = self.native_value(native);
        let prototype_atom = self.intern_atom("prototype");
        let prototype = self
            .own_property(constructor, prototype_atom)
            .unwrap_or(self.object_proto);
        let object = self.heap.alloc(Cell::Object(Self::empty_object(prototype)));
        if let Some(value) = args.first().copied().filter(|value| !value.is_undefined()) {
            let message = self.to_string(program, value)?;
            let message_value = self.heap.alloc(Cell::String(JsString::from_str(&message)));
            self.set_named(program, object, "message", message_value)?;
        }
        Ok(object)
    }
}
