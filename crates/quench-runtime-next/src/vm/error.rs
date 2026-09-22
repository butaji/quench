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
    pub(super) fn install_host_globals(
        &mut self,
        program: &ResidualProgram,
    ) -> Result<(), JsError> {
        self.global(program, "globalThis", self.globals)?;
        self.global(program, "Function", self.native_value(Native::Function))?;
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
        let source = args.last().copied().unwrap_or(Value::UNDEFINED);
        let source = self.to_string(program, source)?;
        if source.trim() != "return this;" {
            return Err(JsError("dynamic Function source is unsupported".into()));
        }
        Ok(self.native_with_env(Native::FunctionReturnThis, Value::NULL))
    }

    pub(super) fn call_function_dispatch(
        &mut self,
        program: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if native == Native::FunctionReturnThis {
            Ok(self.globals)
        } else {
            self.function_native(program, args)
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
