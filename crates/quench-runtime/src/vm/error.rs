use super::object_descriptors::PropertyDescriptorRecord;
use super::property_key::PropertyKey;
use super::*;

const FUNCTION_PROTOTYPE_LENGTH: f64 = 0.0;
const ERROR_OPTIONS_ARGUMENT: usize = 1;
const SUPPRESSED_MESSAGE_ARGUMENT: usize = 2;
const DEFAULT_STACK_TRACE_LIMIT: usize = 10;

const CALL_SITE_METHODS: &[(&str, Native)] = &[
    ("getFileName", Native::CallSiteGetFileName),
    ("getThis", Native::CallSiteGetThis),
    ("getFunctionName", Native::CallSiteGetFunctionName),
    ("getLineNumber", Native::CallSiteGetLineNumber),
    ("getColumnNumber", Native::CallSiteGetColumnNumber),
    ("getTypeName", Native::CallSiteGetTypeName),
    ("getMethodName", Native::CallSiteGetMethodName),
    ("isEval", Native::CallSiteIsEval),
    ("getEvalOrigin", Native::CallSiteGetEvalOrigin),
    ("isConstructor", Native::CallSiteIsConstructor),
    ("isNative", Native::CallSiteIsNative),
    ("toString", Native::CallSiteToString),
];

const ERROR_CONSTRUCTORS: &[(&str, Native)] = &[
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

pub(super) fn error_native_length(native: Native) -> Option<f64> {
    Some(match native {
        Native::ErrorIsError => 1.0,
        Native::ErrorCaptureStackTrace => 2.0,
        Native::ErrorStackSetter => 1.0,
        Native::ErrorToString
        | Native::ErrorStackGetter
        | Native::CallSiteGetFileName
        | Native::CallSiteGetThis
        | Native::CallSiteGetFunctionName
        | Native::CallSiteGetLineNumber
        | Native::CallSiteGetColumnNumber
        | Native::CallSiteGetTypeName
        | Native::CallSiteGetMethodName
        | Native::CallSiteIsEval
        | Native::CallSiteGetEvalOrigin
        | Native::CallSiteIsConstructor
        | Native::CallSiteIsNative
        | Native::CallSiteToString => 0.0,
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
    description: ErrorDescription,
    thrown: Option<Value>,
}

#[derive(Debug)]
enum ErrorDescription {
    Text {
        message: String,
        eval_parser_diagnostic: bool,
    },
    WasmTrap(crate::WasmTrap),
    WasmException,
    WasmLink(String),
}

impl From<&str> for ErrorMessage {
    fn from(value: &str) -> Self {
        Self::from(value.to_owned())
    }
}

impl From<String> for ErrorMessage {
    fn from(message: String) -> Self {
        Self {
            payload: Box::new(ErrorPayload {
                description: ErrorDescription::Text {
                    message,
                    eval_parser_diagnostic: false,
                },
                thrown: None,
            }),
        }
    }
}

impl fmt::Display for JsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0.payload.description {
            ErrorDescription::Text { message, .. } | ErrorDescription::WasmLink(message) => {
                f.write_str(message)
            }
            ErrorDescription::WasmTrap(trap) => fmt::Display::fmt(trap, f),
            ErrorDescription::WasmException => f.write_str("uncaught Wasm exception"),
        }
    }
}

impl JsError {
    pub(crate) fn thrown(value: Value, message: String) -> Self {
        Self(ErrorMessage {
            payload: Box::new(ErrorPayload {
                description: ErrorDescription::Text {
                    message,
                    eval_parser_diagnostic: false,
                },
                thrown: Some(value),
            }),
        })
    }

    pub fn thrown_value(&self) -> Option<Value> {
        self.0.payload.thrown
    }

    pub(crate) fn replace_thrown_value(&mut self, value: Value) {
        self.0.payload.thrown = Some(value);
    }

    pub(crate) fn is_eval_parser_diagnostic(&self) -> bool {
        matches!(
            self.0.payload.description,
            ErrorDescription::Text {
                eval_parser_diagnostic: true,
                ..
            }
        )
    }

    pub(crate) fn mark_eval_parser_diagnostic(mut self) -> Self {
        if let ErrorDescription::Text {
            eval_parser_diagnostic,
            ..
        } = &mut self.0.payload.description
        {
            *eval_parser_diagnostic = true;
        }
        self
    }

    /// Inspect a thrown Wasm exception independently of guest traps and JS throws.
    pub fn wasm_exception(&self) -> Option<Value> {
        matches!(self.0.payload.description, ErrorDescription::WasmException)
            .then_some(self.0.payload.thrown)
            .flatten()
    }

    pub(crate) fn thrown_wasm_exception(value: Value) -> Self {
        Self(ErrorMessage {
            payload: Box::new(ErrorPayload {
                description: ErrorDescription::WasmException,
                thrown: Some(value),
            }),
        })
    }

    /// Inspect a typed WebAssembly trap without treating it as a JS throw.
    pub fn wasm_trap(&self) -> Option<crate::WasmTrap> {
        match self.0.payload.description {
            ErrorDescription::WasmTrap(trap) => Some(trap),
            ErrorDescription::Text { .. }
            | ErrorDescription::WasmLink(_)
            | ErrorDescription::WasmException => None,
        }
    }

    pub(crate) fn wasm_trap_error(trap: crate::WasmTrap) -> Self {
        Self(ErrorMessage {
            payload: Box::new(ErrorPayload {
                description: ErrorDescription::WasmTrap(trap),
                thrown: None,
            }),
        })
    }

    /// Link failures are neither guest traps nor unsupported residual operations.
    pub fn wasm_link_error(&self) -> Option<&str> {
        match &self.0.payload.description {
            ErrorDescription::WasmLink(message) => Some(message),
            _ => None,
        }
    }

    pub(crate) fn wasm_link(message: impl Into<String>) -> Self {
        Self(ErrorMessage {
            payload: Box::new(ErrorPayload {
                description: ErrorDescription::WasmLink(message.into()),
                thrown: None,
            }),
        })
    }

    pub(crate) fn validation(message: String) -> Self {
        Self(ErrorMessage::from(format!(
            "invalid residual program: {message}"
        )))
    }

    pub(super) fn into_message(self) -> String {
        match self.0.payload.description {
            ErrorDescription::Text { message, .. } | ErrorDescription::WasmLink(message) => message,
            ErrorDescription::WasmTrap(trap) => trap.to_string(),
            ErrorDescription::WasmException => "uncaught Wasm exception".into(),
        }
    }
}

impl<H: Host> Vm<H> {
    pub(super) fn error_is_error(&self, value: Value) -> bool {
        matches!(self.heap.get(value), Some(Cell::Object(object)) if object.has_error_data())
    }

    pub(super) fn error_stack_getter(
        &mut self,
        program: &ResidualProgram,
        receiver: Value,
    ) -> Result<Value, JsError> {
        if !self.is_object_like(receiver) {
            return Err(self.type_error(program, "Error stack getter requires an object".into()));
        }
        let Some(StackData::Captured(records)) = self
            .object_data(receiver)
            .and_then(Object::stack_data)
            .cloned()
        else {
            return if self.error_is_error(receiver) {
                self.error_to_string(program, receiver)
            } else {
                Ok(Value::UNDEFINED)
            };
        };

        let receiver_root = self.heap.root(receiver);
        let mut roots = Vec::with_capacity(2);
        let outcome = (|| {
            let global = self.realm.globals;
            let error_atom = self.intern_atom("Error");
            let constructor = self.get_property(program, global, error_atom)?;
            roots.push(self.heap.root(constructor));
            let prepare_atom = self.intern_atom("prepareStackTrace");
            let prepare = self.get_property(program, constructor, prepare_atom)?;
            roots.push(self.heap.root(prepare));
            let result = (|| {
                if self.is_function(prepare) {
                    let sites = self.error_call_site_array(program, &records)?;
                    let sites_root = self.heap.root(sites);
                    let target = self.heap.root_value(receiver_root).unwrap();
                    let constructor = self.heap.root_value(roots[0]).unwrap();
                    let prepare = self.heap.root_value(roots[1]).unwrap();
                    let sites = self.heap.root_value(sites_root).unwrap();
                    let result = self.call_value(program, prepare, constructor, &[target, sites]);
                    self.heap.release_root(sites_root);
                    result
                } else {
                    self.error_default_stack(program, receiver_root, &records)
                }
            })();
            result
        })();
        for root in roots {
            self.heap.release_root(root);
        }
        self.heap.release_root(receiver_root);
        outcome
    }

    fn error_default_stack(
        &mut self,
        program: &ResidualProgram,
        receiver: RootId,
        records: &[CallSiteRecord],
    ) -> Result<Value, JsError> {
        let target = self.heap.root_value(receiver).unwrap();
        let mut stack = if self.error_is_error(target) {
            let header = self.error_to_string(program, target)?;
            self.to_string(program, header)?
        } else {
            "Error".to_owned()
        };
        for record in records {
            stack.push_str("\n    at ");
            stack.push_str(&Self::format_call_site(record));
        }
        Ok(self.heap.alloc(Cell::String(JsString::from_str(&stack))))
    }

    fn error_call_site_array(
        &mut self,
        program: &ResidualProgram,
        records: &[CallSiteRecord],
    ) -> Result<Value, JsError> {
        let mut roots = Vec::with_capacity(records.len());
        let result = (|| {
            let mut sites = Vec::with_capacity(records.len());
            for record in records {
                let site = self
                    .heap
                    .alloc(Cell::Object(Self::empty_object(self.object_proto)));
                let root = self.heap.root(site);
                roots.push(root);
                self.object_data_mut(site)
                    .expect("CallSite is an ordinary object")
                    .set_stack_data(StackData::CallSite(record.clone()));
                for &(name, native) in CALL_SITE_METHODS {
                    self.set_builtin_named(program, site, name, native)?;
                }
                sites.push(self.heap.root_value(root).unwrap());
            }
            Ok(self.heap.alloc(Cell::array(
                self.array_proto,
                Rc::new(sites),
            )))
        })();
        for root in roots {
            self.heap.release_root(root);
        }
        result
    }

    pub(super) fn error_capture_stack_trace(
        &mut self,
        program: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let target = args.first().copied().unwrap_or(Value::UNDEFINED);
        if !self.is_object_like(target) {
            return Err(
                self.type_error(program, "Error.captureStackTrace requires an object".into())
            );
        }
        let constructor_opt = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let trim_constructor =
            constructor_opt != Value::UNDEFINED && self.is_function(constructor_opt);
        let limit = self.error_stack_trace_limit(program)?;
        let mut records = Vec::with_capacity(self.frames.len().min(limit));
        for frame in self.frames.iter().rev() {
            if records.len() >= limit {
                break;
            }
            if frame.context == CallContext::Internal {
                continue;
            }
            if trim_constructor && frame.context.callable() == Some(constructor_opt) {
                break;
            }
            let Some(residual) = self.programs.get(frame.program) else {
                continue;
            };
            let Some(function) = residual.functions.get(frame.function as usize) else {
                continue;
            };
            // A PC without compiler-owned source metadata has no truthful JS location.
            let pc = u32::try_from(frame.pc).unwrap_or(u32::MAX);
            let position = function
                .source_positions
                .partition_point(|position| position.pc <= pc)
                .checked_sub(1)
                .and_then(|index| function.source_positions.get(index));
            let Some(position) = position else {
                continue;
            };
            let function_name = function
                .name
                .map(|atom| self.atom_name(atom).to_owned())
                .filter(|name| !name.starts_with('\0'));
            records.push(CallSiteRecord {
                file_name: residual.source_name.clone(),
                function_name,
                this_value: frame.this,
                line: position.line,
                column: position.column,
            });
        }

        let target_root = self.heap.root(target);
        let result = (|| {
            let target = self.heap.root_value(target_root).unwrap();
            if self.object_data(target).is_none() {
                return Err(self.type_error(
                    program,
                    "Error.captureStackTrace target must be an object".into(),
                ));
            }
            let getter = self.native_with_realm(
                Native::ErrorStackGetter,
                self.realm.globals,
                self.realm.globals,
            );
            let setter = self.native_with_realm(
                Native::ErrorStackSetter,
                self.realm.globals,
                self.realm.globals,
            );
            let target = self.heap.root_value(target_root).unwrap();
            let key = self.heap.alloc(Cell::String("stack".into()));
            self.define_property_or_throw(
                program,
                target,
                key,
                PropertyDescriptorRecord {
                    value: None,
                    writable: None,
                    enumerable: Some(false),
                    configurable: Some(true),
                    getter: Some(getter),
                    setter: Some(setter),
                },
            )?;
            self.object_data_mut(target)
                .expect("captureStackTrace target remains object-like")
                .set_stack_data(StackData::Captured(records));
            Ok(Value::UNDEFINED)
        })();
        self.heap.release_root(target_root);
        result
    }

    fn error_stack_trace_limit(&mut self, program: &ResidualProgram) -> Result<usize, JsError> {
        let global = self.realm.globals;
        let error_atom = self.intern_atom("Error");
        let constructor = self.get_property(program, global, error_atom)?;
        let limit_atom = self.intern_atom("stackTraceLimit");
        let value = self.get_property(program, constructor, limit_atom)?;
        let number = self.to_number(program, value)?;
        Ok(if number.is_nan() || number <= 0.0 {
            0
        } else if !number.is_finite() {
            usize::MAX
        } else {
            number.floor().min(usize::MAX as f64) as usize
        })
    }

    fn format_call_site(record: &CallSiteRecord) -> String {
        let location = format!("{}:{}:{}", record.file_name, record.line, record.column);
        record
            .function_name
            .as_ref()
            .map_or(location.clone(), |name| format!("{name} ({location})"))
    }

    pub(super) fn call_site_native(
        &mut self,
        program: &ResidualProgram,
        native: Native,
        receiver: Value,
    ) -> Result<Value, JsError> {
        let record = match self.object_data(receiver).and_then(Object::stack_data) {
            Some(StackData::CallSite(record)) => record.clone(),
            _ => {
                return Err(self.type_error(
                    program,
                    "CallSite method called on incompatible receiver".into(),
                ));
            }
        };
        match native {
            Native::CallSiteGetFileName => {
                Ok(self.heap.alloc(Cell::String(record.file_name.into())))
            }
            Native::CallSiteGetThis => Ok(record.this_value),
            Native::CallSiteGetFunctionName | Native::CallSiteGetMethodName => Ok(record
                .function_name
                .map(|name| self.heap.alloc(Cell::String(name.into())))
                .unwrap_or(Value::UNDEFINED)),
            Native::CallSiteGetLineNumber => Ok(Value::number(f64::from(record.line))),
            Native::CallSiteGetColumnNumber => Ok(Value::number(f64::from(record.column))),
            Native::CallSiteIsEval => Ok(Value::FALSE),
            Native::CallSiteIsConstructor | Native::CallSiteIsNative => Ok(Value::FALSE),
            Native::CallSiteGetTypeName | Native::CallSiteGetEvalOrigin => Ok(Value::UNDEFINED),
            Native::CallSiteToString => Ok(self
                .heap
                .alloc(Cell::String(Self::format_call_site(&record).into()))),
            _ => Err(JsError::validation(
                "invalid CallSite native dispatch".into(),
            )),
        }
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
        let receiver = self.heap.root(receiver);
        let value_root = self.heap.root(value);
        let mut key_root = None;
        let result = (|| {
            let target = self.heap.root_value(receiver).unwrap();
            let home =
                self.realm.intrinsics.builtin_prototypes[&(self.realm.globals, Native::Error)];
            if target == home {
                return Err(self.type_error(program, "cannot set Error.prototype.stack".into()));
            }
            let key = self.heap.alloc(Cell::String("stack".into()));
            let key = self.heap.root(key);
            key_root = Some(key);
            let property = self.heap.root_value(key).unwrap();
            let target = self.heap.root_value(receiver).unwrap();
            let descriptor =
                self.object_get_own_property_descriptor(program, &[target, property])?;
            let target = self.heap.root_value(receiver).unwrap();
            let value = self.heap.root_value(value_root).unwrap();
            if descriptor.is_undefined() {
                let key = self.heap.root_value(key).unwrap();
                self.define_property_or_throw(
                    program,
                    target,
                    key,
                    PropertyDescriptorRecord::data(value),
                )?;
            } else {
                let atom = self.intern_atom("stack");
                if !self.set_property_with_receiver(program, target, atom, value, target)? {
                    return Err(self.type_error(program, "cannot set Error stack property".into()));
                }
            }
            if let Some(object) = self.object_data_mut(target) {
                object.clear_stack_data();
            }
            Ok(Value::UNDEFINED)
        })();
        if let Some(root) = key_root {
            self.heap.release_root(root);
        }
        self.heap.release_root(value_root);
        self.heap.release_root(receiver);
        result
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

    pub(super) fn enter_stack(&mut self) -> Result<crate::stack::StackGuard, JsError> {
        crate::stack::StackGuard::enter().map_err(|()| self.stack_exhaustion_error())
    }

    pub(super) fn stack_exhaustion_error(&mut self) -> JsError {
        // Exhaustion must not invoke guest accessors or constructors.
        let prototype =
            self.realm.intrinsics.builtin_prototypes[&(self.realm.globals, Native::RangeError)];
        let object = self.heap.alloc(Cell::Object(Object::error(prototype)));
        let message = self
            .heap
            .alloc(Cell::String(crate::stack::STACK_EXHAUSTED_MESSAGE.into()));
        self.set_builtin_value_named(object, "message", message)
            .expect("fresh error object accepts message");
        JsError::thrown(object, crate::stack::STACK_EXHAUSTED_MESSAGE.into())
    }

    pub(super) fn range_error(&mut self, program: &ResidualProgram, text: String) -> JsError {
        let message = self.heap.alloc(Cell::String(JsString::from_str(&text)));
        let object = self
            .construct_error_native(program, Native::RangeError, &[message])
            .unwrap_or(Value::UNDEFINED);
        JsError::thrown(object, text)
    }

    pub(super) fn uri_error(&mut self, program: &ResidualProgram, text: String) -> JsError {
        let message = self.heap.alloc(Cell::String(JsString::from_str(&text)));
        let object = self
            .construct_error_native(program, Native::URIError, &[message])
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
            Some(Cell::String(_)) => "\0quench:string-value",
            Some(Cell::Symbol(_)) => "\0quench:symbol-value",
            Some(Cell::BigInt(_)) => "\0quench:bigint-value",
            _ if value.as_number().is_some() => "\0quench:number-value",
            _ if value.as_bool().is_some() => "\0quench:boolean-value",
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
        let prototype_atom = self.prototype_atom();
        let constructor_atom = self.intern_atom("BigInt");
        let constructor = self
            .active_native_env()
            .and_then(|global| self.own_property(global, constructor_atom))
            .unwrap_or_else(|| self.native_value(Native::BigInt));
        let prototype = self.get_property(p, constructor, prototype_atom)?;
        let object = self.heap.alloc(Cell::Object(Self::empty_object(prototype)));
        let value_atom = self.intern_atom("\0quench:bigint-value");
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
        let prototype = self.prototype_atom();
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
        self.set_named_constant(program, symbol, "prototype", symbol_prototype)?;
        self.set_builtin_value_named(symbol_prototype, "constructor", symbol)?;
        self.set_builtin_named(program, symbol_prototype, "valueOf", Native::SymbolValueOf)?;
        self.set_builtin_named(
            program,
            symbol_prototype,
            "toString",
            Native::SymbolToString,
        )?;
        let description = self.native_value(Native::SymbolDescriptionGetter);
        self.set_builtin_function_name(description, "get description")?;
        let description_atom = self.intern_atom("description");
        self.set_property(symbol_prototype, description_atom, Value::UNDEFINED)?;
        self.set_property_attributes(
            symbol_prototype,
            PropertyKey::string(description_atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: true,
                getter: Some(description),
                setter: None,
            },
        );
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
                    let realm = self.install_realm_host_api(program, self.realm.globals)?;
                    self.install_test262_agent(program, realm)?;
                    self.global(program, global.name, realm)?;
                    continue;
                }
                CapabilityId::IsHTMLDDA => {
                    let realm_atom = self.intern_atom(global.name);
                    let realm = self
                        .own_property(self.realm.globals, realm_atom)
                        .ok_or_else(|| JsError("IsHTMLDDA capability requires $262".into()))?;
                    self.set_named(
                        program,
                        realm,
                        "IsHTMLDDA",
                        self.native_value(Native::IsHTMLDDA),
                    )?;
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
        let prototype_atom = self.prototype_atom();
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
        let residual = crate::Engine::specialize_function_constructor(
            &parser_parameters,
            source,
            &source_name,
            &atom_prefix,
            kind,
        )
        .map_err(|diagnostics| {
            if diagnostics
                .iter()
                .any(crate::compile::Diagnostic::is_stack_exhausted)
            {
                return self.stack_exhaustion_error();
            }
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
                .find(|(_, function)| function.parent == Some(super::ROOT_FUNCTION_ID))
                .map(|(id, _)| id as u32)
                .ok_or_else(|| {
                    self.type_error(program, "dynamic Function body is unavailable".into())
                })?;
            self.closure_in_realm(&residual, function, Value::NULL, function_realm)
        })();
        self.active_program = active_program;
        let function = result?;
        self.set_builtin_function_name(function, "anonymous")?;
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
            self.function_native(program, native, args)
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
            let parsed = crate::bigint::parse_string(text.host_string());
            return match parsed {
                Some(value) => Ok(self.heap.alloc(Cell::BigInt(value.to_string()))),
                None => self.syntax_error_result(program, "invalid BigInt value"),
            };
        }
        Err(self.type_error(program, "cannot convert value to BigInt".into()))
    }

    pub(super) fn create_realm(&mut self, program: &ResidualProgram) -> Result<Value, JsError> {
        let global = self.object();
        self.set_builtin_value_named(global, "globalThis", global)?;
        self.install_throw_type_error_for_realm(global)?;
        for (name, value) in [
            ("undefined", Value::UNDEFINED),
            ("NaN", Value::number(f64::NAN)),
            ("Infinity", Value::number(f64::INFINITY)),
        ] {
            self.set_builtin_value_named(global, name, value)?;
            let atom = self.intern_atom(name);
            self.set_property_attributes(
                global,
                PropertyKey::string(atom),
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
        let type_error = self.native_with_realm(Native::RealmTypeError, global, global);
        let type_error_prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(Value::NULL)));
        self.realm
            .intrinsics
            .builtin_prototypes
            .insert((global, Native::TypeError), type_error_prototype);
        self.realm
            .intrinsics
            .builtin_prototypes
            .insert((global, Native::RealmTypeError), type_error_prototype);
        self.set_named_constant(program, type_error, "prototype", type_error_prototype)?;
        self.set_named(program, type_error_prototype, "constructor", type_error)?;
        let type_error_name = self.heap.alloc(Cell::String("TypeError".into()));
        self.set_named(program, type_error_prototype, "name", type_error_name)?;
        self.set_named(program, global, "TypeError", type_error)?;
        let eval = self.native_with_realm(Native::Eval, global, global);
        self.set_builtin_function_value_named(global, "eval", eval)?;
        let function = self.native_with_realm(Native::Function, global, global);
        self.set_named(program, global, "Function", function)?;
        let object_prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.object_proto)));
        self.realm
            .intrinsics
            .builtin_prototypes
            .insert((global, Native::Object), object_prototype);
        self.install_object_prototype_methods(program, object_prototype, Some(global))?;
        let (shared_array_buffer, _) =
            self.install_shared_array_buffer_for_realm(program, global, object_prototype)?;
        self.set_builtin_value_named(global, "SharedArrayBuffer", shared_array_buffer)?;
        self.object_data_mut(global)
            .expect("realm global is an object")
            .proto = object_prototype;
        self.install_string_for_realm(program, global, object_prototype)?;
        self.install_number_for_realm(program, global, object_prototype)?;
        self.install_function_prototype_for_realm(global, object_prototype, function)?;
        self.install_typed_array_constructors_for_realm(program, global)?;
        self.install_weak_collections_for_realm(program, global, object_prototype)?;
        let set = self.native_with_realm(Native::Set, global, global);
        self.install_set_prototype(program, set, object_prototype)?;
        self.install_set_species(set)?;
        self.set_builtin_value_named(global, "Set", set)?;
        let shadow_realm = self.native_with_realm(Native::ShadowRealm, global, global);
        self.install_shadow_realm_for_realm(program, global, shadow_realm, object_prototype)?;
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
        let promise_prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
        self.install_promise_for_realm(program, promise, promise_prototype, Some(global))?;
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
        self.realm.intrinsics.iterator_prototypes.insert(
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
            self.realm
                .intrinsics
                .builtin_prototypes
                .insert((global, native), prototype);
            let constructor = self.native_with_realm(native, global, global);
            self.object_data_mut(constructor)
                .expect("realm dynamic function")
                .proto = function;
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
            if native == Native::GeneratorFunction {
                self.install_generator_prototype(realm_generator_proto, prototype, Some(global))?;
                self.set_builtin_value_named(prototype, "prototype", realm_generator_proto)?;
                let prototype_atom = self.prototype_atom();
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
                let prototype_atom = self.prototype_atom();
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
        let prototype_atom = self.prototype_atom();
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
        self.set_builtin_function_name(object, "Object")?;
        self.set_named(program, object, "prototype", object_prototype)?;
        self.set_named(program, object_prototype, "constructor", object)?;
        self.set_property_attributes(
            object,
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
        for (name, native) in [
            ("defineProperty", Native::ObjectDefineProperty),
            ("defineProperties", Native::ObjectDefineProperties),
            ("setPrototypeOf", Native::ObjectSetPrototypeOf),
            ("getPrototypeOf", Native::ObjectGetPrototypeOf),
            ("create", Native::ObjectCreate),
            ("keys", Native::ObjectKeys),
            ("values", Native::ObjectValues),
            ("entries", Native::ObjectEntries),
            ("getOwnPropertyNames", Native::ObjectGetOwnPropertyNames),
            ("getOwnPropertySymbols", Native::ObjectGetOwnPropertySymbols),
            (
                "getOwnPropertyDescriptor",
                Native::ObjectGetOwnPropertyDescriptor,
            ),
            (
                "getOwnPropertyDescriptors",
                Native::ObjectGetOwnPropertyDescriptors,
            ),
            ("fromEntries", Native::ObjectFromEntries),
            ("assign", Native::ObjectAssign),
            ("is", Native::ObjectIs),
            ("hasOwn", Native::ObjectHasOwn),
            ("preventExtensions", Native::ObjectPreventExtensions),
            ("isExtensible", Native::ObjectIsExtensible),
        ] {
            self.set_realm_builtin_named(program, object, name, native, Some(global))?;
        }
        self.set_builtin_value_named(global, "Object", object)?;
        let object_name = self.intern_atom("Object");
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
        let array_prototype = self.heap.alloc(Cell::array(
            object_prototype,
            Rc::new(Vec::new()),
        ));
        self.install_array_for_realm(program, global, array_prototype)?;
        let map = self.native_with_realm(Native::Map, global, global);
        let map_prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
        self.set_builtin_function_name(map, "Map")?;
        self.realm
            .intrinsics
            .builtin_prototypes
            .insert((global, Native::Map), map_prototype);
        self.set_builtin_value_named(map, "prototype", map_prototype)?;
        self.set_builtin_value_named(map_prototype, "constructor", map)?;
        let prototype_atom = self.prototype_atom();
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
        let regexp_prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
        self.install_regexp_intrinsics(global, regexp, regexp_prototype)?;
        self.install_regexp_accessors(program, regexp_prototype, global)?;
        self.install_regexp_legacy_accessors(program, regexp, global)?;
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
        let (array_buffer, _) =
            self.install_array_buffer_for_realm(program, global, object_prototype)?;
        self.set_builtin_value_named(global, "ArrayBuffer", array_buffer)?;
        let realm_error_prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
        self.realm
            .intrinsics
            .builtin_prototypes
            .insert((global, Native::Error), realm_error_prototype);
        let realm_error_constructor = self.native_with_realm(Native::Error, global, global);
        self.install_error_stack_api(program, realm_error_constructor)?;
        self.set_named_constant(
            program,
            realm_error_constructor,
            "prototype",
            realm_error_prototype,
        )?;
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
        self.set_builtin_function_name(realm_error_constructor, "Error")?;
        self.set_builtin_value_named(global, "Error", realm_error_constructor)?;
        self.set_realm_builtin_named(
            program,
            realm_error_prototype,
            "toString",
            Native::ErrorToString,
            Some(global),
        )?;
        self.object_data_mut(type_error_prototype)
            .expect("realm TypeError prototype")
            .proto = realm_error_prototype;
        self.object_data_mut(type_error)
            .expect("realm TypeError constructor")
            .proto = realm_error_constructor;
        self.set_builtin_function_name(type_error, "TypeError")?;
        for &(name, native) in ERROR_CONSTRUCTORS {
            if matches!(native, Native::Error | Native::TypeError) {
                continue;
            }
            let constructor = self.native_with_realm(native, global, global);
            self.set_builtin_function_name(constructor, name)?;
            let prototype = self
                .heap
                .alloc(Cell::Object(Self::empty_object(realm_error_prototype)));
            self.realm
                .intrinsics
                .builtin_prototypes
                .insert((global, native), prototype);
            self.set_named_constant(program, constructor, "prototype", prototype)?;
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
        self.install_realm_default_bindings(global)?;
        self.install_json_for_realm(program, global)?;
        self.install_symbol_for_realm(program, global, object_prototype)?;
        self.install_intl_for_realm(program, global, object_prototype)?;
        let realm = self.install_realm_host_api(program, global)?;
        for binding in self.host.globals() {
            if binding.capability == CapabilityId::CreateRealm {
                self.set_builtin_value_named(global, binding.name, realm)?;
            }
        }
        Ok(realm)
    }

    fn install_realm_host_api(
        &mut self,
        program: &ResidualProgram,
        global: Value,
    ) -> Result<Value, JsError> {
        let realm = self.object();
        self.set_named(program, realm, "global", global)?;
        let current_realm = global == self.realm.globals;
        for (name, native) in [
            ("createRealm", Native::CreateRealm),
            ("evalScript", Native::EvalScript),
            ("detachArrayBuffer", Native::DetachArrayBuffer),
            ("gc", Native::CollectGarbage),
            ("AbstractModuleSource", Native::AbstractModuleSource),
        ] {
            let method = self.realm_native_value(native, global, current_realm);
            // The primary module-source constructor installs its name with its prototype.
            if native != Native::AbstractModuleSource || !current_realm {
                self.set_builtin_function_name(method, name)?;
            }
            self.set_named(program, realm, name, method)?;
        }
        Ok(realm)
    }

    fn install_realm_default_bindings(&mut self, global: Value) -> Result<(), JsError> {
        const SHARED_BINDINGS: &[&str] = &[
            "AggregateError",
            "Array",
            "ArrayBuffer",
            "BigInt",
            "BigInt64Array",
            "BigUint64Array",
            "Boolean",
            "DataView",
            "Date",
            "EvalError",
            "Float16Array",
            "Float32Array",
            "Float64Array",
            "Int8Array",
            "Int16Array",
            "Int32Array",
            "SharedArrayBuffer",
            "Uint8Array",
            "Uint8ClampedArray",
            "Uint16Array",
            "Uint32Array",
            "WeakMap",
            "WeakRef",
            "WeakSet",
            "decodeURI",
            "decodeURIComponent",
            "encodeURI",
            "encodeURIComponent",
            "escape",
            "isFinite",
            "isNaN",
            "unescape",
        ];
        let source_global = self.realm.globals;
        for name in SHARED_BINDINGS {
            let atom = self.intern_atom(name);
            if self.own_property(global, atom).is_some() {
                continue;
            }
            let Some(value) = self.own_property(source_global, atom) else {
                continue;
            };
            let value = match self.heap.get(value) {
                _ if *name == "Symbol" => value,
                Some(Cell::Function {
                    kind: FunctionKind::Native(native),
                    ..
                }) => {
                    let function = self.native_with_realm(*native, Value::NULL, global);
                    self.set_builtin_function_name(function, name)?;
                    function
                }
                _ => value,
            };
            self.set_builtin_value_named(global, name, value)?;
        }
        for name in ["Atomics", "Math", "Reflect"] {
            let atom = self.intern_atom(name);
            if self.own_property(global, atom).is_none()
                && let Some(value) = self.own_property(source_global, atom)
            {
                self.set_builtin_value_named(global, name, value)?;
            }
        }
        Ok(())
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
        self.realm
            .intrinsics
            .builtin_prototypes
            .insert((global, Native::Function), prototype);
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
        let throw_type_error = self.throw_type_error_for_realm(global);
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

    pub(super) fn install_error_intrinsics(
        &mut self,
        program: &ResidualProgram,
    ) -> Result<(), JsError> {
        let error_prototype = self.object();
        for (index, (name, native)) in ERROR_CONSTRUCTORS.iter().enumerate() {
            let constructor = self.native_value(*native);
            let prototype = if index == 0 {
                error_prototype
            } else {
                self.heap
                    .alloc(Cell::Object(Self::empty_object(error_prototype)))
            };
            self.realm
                .intrinsics
                .builtin_prototypes
                .insert((self.realm.globals, *native), prototype);
            self.set_named_constant(program, constructor, "prototype", prototype)?;
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
            let name_value = self.heap.alloc(Cell::String(JsString::from_str(name)));
            self.set_builtin_value_named(prototype, "name", name_value)?;
            let empty_message = self.heap.alloc(Cell::String(JsString::from_str("")));
            self.set_builtin_value_named(prototype, "message", empty_message)?;
        }
        let error_constructor = self.native_value(Native::Error);
        for (_, native) in ERROR_CONSTRUCTORS.iter().skip(1) {
            let constructor = self.native_value(*native);
            if let Some(function) = self.object_data_mut(constructor) {
                function.proto = error_constructor;
            }
        }
        self.set_builtin_named(program, error_prototype, "toString", Native::ErrorToString)?;
        let error_constructor = self.native_value(Native::Error);
        self.install_error_stack_api(program, error_constructor)?;
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

    pub(super) fn install_errors(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        for &(name, native) in ERROR_CONSTRUCTORS {
            self.global(program, name, self.native_value(native))?;
        }
        Ok(())
    }

    fn install_error_stack_api(
        &mut self,
        program: &ResidualProgram,
        constructor: Value,
    ) -> Result<(), JsError> {
        self.set_builtin_named(
            program,
            constructor,
            "captureStackTrace",
            Native::ErrorCaptureStackTrace,
        )?;
        self.set_builtin_value_named(
            constructor,
            "stackTraceLimit",
            Value::number(DEFAULT_STACK_TRACE_LIMIT as f64),
        )?;
        self.set_builtin_value_named(constructor, "prepareStackTrace", Value::UNDEFINED)
    }

    pub(super) fn construct_error_native(
        &mut self,
        program: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let prototype = self
            .realm
            .intrinsics
            .builtin_prototypes
            .get(&(self.realm.globals, native))
            .copied()
            .unwrap_or_else(|| {
                let constructor = self.native_value(native);
                let atom = self.prototype_atom();
                self.own_property(constructor, atom)
                    .unwrap_or(self.object_proto)
            });
        self.construct_error_with_prototype(program, native, args, prototype)
    }

    pub(super) fn construct_error_with_new_target(
        &mut self,
        program: &ResidualProgram,
        native: Native,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        self.with_call_roots(
            std::iter::once(new_target).chain(args.iter().copied()),
            |vm| {
                let prototype = vm
                    .native_constructor_prototype(program, new_target, native)?
                    .ok_or_else(|| {
                        JsError::validation("error constructor has no intrinsic prototype".into())
                    })?;
                vm.construct_error_with_prototype(program, native, args, prototype)
            },
        )
    }

    fn construct_error_with_prototype(
        &mut self,
        program: &ResidualProgram,
        native: Native,
        args: &[Value],
        prototype: Value,
    ) -> Result<Value, JsError> {
        let (message, options, suppressed) = match native {
            Native::SuppressedError => (
                args.get(SUPPRESSED_MESSAGE_ARGUMENT)
                    .copied()
                    .filter(|value| !value.is_undefined())
                    .map(|value| self.heap.root(value)),
                None,
                Some([
                    self.heap
                        .root(args.first().copied().unwrap_or(Value::UNDEFINED)),
                    self.heap
                        .root(args.get(1).copied().unwrap_or(Value::UNDEFINED)),
                ]),
            ),
            _ => (
                args.first()
                    .copied()
                    .filter(|value| !value.is_undefined())
                    .map(|value| self.heap.root(value)),
                args.get(ERROR_OPTIONS_ARGUMENT)
                    .copied()
                    .filter(|value| self.is_object_like(*value))
                    .map(|value| self.heap.root(value)),
                None,
            ),
        };
        let object = self.heap.alloc(Cell::Object(Object::error(prototype)));
        let root = self.heap.root(object);
        let outcome = (|| {
            if let Some(message) = message {
                let value = self.heap.root_value(message).unwrap();
                let message = self.to_string(program, value)?;
                let message = self.heap.alloc(Cell::String(JsString::from_str(&message)));
                let object = self.heap.root_value(root).unwrap();
                self.set_builtin_value_named(object, "message", message)?;
            }
            if let Some([error, suppressed]) = suppressed {
                for (name, value) in [("error", error), ("suppressed", suppressed)] {
                    let value = self.heap.root_value(value).unwrap();
                    let object = self.heap.root_value(root).unwrap();
                    self.set_builtin_value_named(object, name, value)?;
                }
            }
            if let Some(options) = options {
                self.install_error_cause(program, root, options)?;
            }
            Ok(self.heap.root_value(root).unwrap())
        })();
        for root in [Some(root), message, options]
            .into_iter()
            .flatten()
            .chain(suppressed.into_iter().flatten())
        {
            self.heap.release_root(root);
        }
        outcome
    }

    fn install_error_cause(
        &mut self,
        program: &ResidualProgram,
        object: RootId,
        options: RootId,
    ) -> Result<(), JsError> {
        let cause_atom = self.intern_atom("cause");
        let cause_key = self.heap.alloc(Cell::String("cause".into()));
        let value = self.heap.root_value(options).unwrap();
        if self.has_property(program, value, cause_key)? {
            let options = self.heap.root_value(options).unwrap();
            let cause = self.get_property(program, options, cause_atom)?;
            let object = self.heap.root_value(object).unwrap();
            self.set_builtin_value_named(object, "cause", cause)?;
        }
        Ok(())
    }

    pub(super) fn construct_aggregate_error(
        &mut self,
        program: &ResidualProgram,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        let input = self
            .heap
            .root(args.first().copied().unwrap_or(Value::UNDEFINED));
        let message = args
            .get(1)
            .copied()
            .filter(|value| !value.is_undefined())
            .map(|value| self.heap.root(value));
        let options = args
            .get(2)
            .copied()
            .filter(|value| self.is_object_like(*value))
            .map(|value| self.heap.root(value));
        let new_target = self.heap.root(new_target);
        let mut object = None;
        let mut errors_list = Vec::new();
        let outcome = (|| {
            let target = self.heap.root_value(new_target).unwrap();
            let prototype = self
                .native_constructor_prototype(program, target, Native::AggregateError)?
                .ok_or_else(|| {
                    JsError::validation("AggregateError has no intrinsic prototype".into())
                })?;
            let value = self.heap.alloc(Cell::Object(Object::error(prototype)));
            let root = self.heap.root(value);
            object = Some(root);
            if let Some(message) = message {
                let message = self.heap.root_value(message).unwrap();
                let message = self.to_string(program, message)?;
                let message = self.heap.alloc(Cell::String(JsString::from_str(&message)));
                let value = self.heap.root_value(root).unwrap();
                self.set_builtin_value_named(value, "message", message)?;
            }
            if let Some(options) = options {
                self.install_error_cause(program, root, options)?;
            }
            let input = self.heap.root_value(input).unwrap();
            errors_list = self.iterable_to_rooted_list(program, input)?;
            let prototype = self.array_prototype_for_realm(self.realm.globals);
            let errors = errors_list
                .iter()
                .map(|value| self.heap.root_value(*value).unwrap())
                .collect();
            let errors = self.new_array_with_prototype(errors, prototype);
            let value = self.heap.root_value(root).unwrap();
            self.set_builtin_value_named(value, "errors", errors)?;
            Ok(value)
        })();
        for root in [Some(input), message, options, Some(new_target), object]
            .into_iter()
            .flatten()
        {
            self.heap.release_root(root);
        }
        for value in errors_list {
            self.heap.release_root(value);
        }
        outcome
    }
}
