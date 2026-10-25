use super::object_descriptors::PropertyDescriptorRecord;
use super::property_key::PropertyKey;
use super::*;

const FUNCTION_PROTOTYPE_LENGTH: f64 = 0.0;
const ERROR_OPTIONS_ARGUMENT: usize = 1;
const SUPPRESSED_MESSAGE_ARGUMENT: usize = 2;
const DEFAULT_STACK_TRACE_LIMIT: usize = 10;

const CALL_SITE_METHODS: &[(&str, Native)] = &[
    ("getColumnNumber", Native::CallSiteGetColumnNumber),
    ("getEvalOrigin", Native::CallSiteGetEvalOrigin),
    ("getFileName", Native::CallSiteGetFileName),
    ("getFunction", Native::CallSiteGetFunction),
    ("getFunctionName", Native::CallSiteGetFunctionName),
    ("getLineNumber", Native::CallSiteGetLineNumber),
    ("getMethodName", Native::CallSiteGetMethodName),
    ("getThis", Native::CallSiteGetThis),
    ("getTypeName", Native::CallSiteGetTypeName),
    ("isConstructor", Native::CallSiteIsConstructor),
    ("isEval", Native::CallSiteIsEval),
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
        | Native::CallSiteConstructor
        | Native::ErrorStackGetter
        | Native::CallSiteGetFileName
        | Native::CallSiteGetThis
        | Native::CallSiteGetFunction
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
    source_location: Option<ErrorSourceLocation>,
}

#[derive(Debug)]
struct ErrorSourceLocation {
    file: String,
    line: u32,
    column: u32,
    source_line: Option<String>,
}

#[derive(Debug)]
enum ErrorDescription {
    Text { message: String },
    ExecutionTerminated,
    DataClone(String),
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
                description: ErrorDescription::Text { message },
                thrown: None,
                source_location: None,
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
            ErrorDescription::DataClone(message) => write!(f, "DataCloneError: {message}"),
            ErrorDescription::WasmTrap(trap) => fmt::Display::fmt(trap, f),
            ErrorDescription::WasmException => f.write_str("uncaught Wasm exception"),
            ErrorDescription::ExecutionTerminated => f.write_str("execution terminated"),
        }
    }
}

impl JsError {
    pub(crate) fn execution_terminated() -> Self {
        Self(ErrorMessage {
            payload: Box::new(ErrorPayload {
                description: ErrorDescription::ExecutionTerminated,
                thrown: None,
                source_location: None,
            }),
        })
    }
    /// A host termination is uncatchable, not a guest Error object or throw.
    /// A JavaScript exception: thrown by guest code or raised as a JavaScript error, as
    /// opposed to a Wasm exception, a trap, a link failure or a terminated execution.
    pub fn is_javascript_exception(&self) -> bool {
        matches!(
            self.0.payload.description,
            ErrorDescription::Text { .. } | ErrorDescription::DataClone(_)
        )
    }

    pub fn is_execution_terminated(&self) -> bool {
        matches!(
            self.0.payload.description,
            ErrorDescription::ExecutionTerminated
        )
    }

    pub(crate) fn thrown(value: Value, message: String) -> Self {
        Self(ErrorMessage {
            payload: Box::new(ErrorPayload {
                description: ErrorDescription::Text { message },
                thrown: Some(value),
                source_location: None,
            }),
        })
    }

    pub(crate) fn data_clone(value: Value, message: &str) -> Self {
        let mut error = Self::thrown(value, String::new());
        error.0.payload.description = ErrorDescription::DataClone(message.to_owned());
        error
    }

    /// A serialization failure generated by the clone algorithm, not a getter's throw.
    pub fn data_clone_failure_message(&self) -> Option<&str> {
        match &self.0.payload.description {
            ErrorDescription::DataClone(message) => Some(message),
            _ => None,
        }
    }

    /// The first escaping throw site, owned by compiler source metadata.
    pub fn source_location(&self) -> Option<(&str, u32, u32, Option<&str>)> {
        self.0.payload.source_location.as_ref().map(|location| {
            (
                location.file.as_str(),
                location.line,
                location.column,
                location.source_line.as_deref(),
            )
        })
    }

    pub(super) fn capture_source_location(
        &mut self,
        program: &ResidualProgram,
        function: u32,
        pc: u32,
    ) {
        if self.0.payload.source_location.is_some() {
            return;
        }
        let Some(position) = program.source_position(function, pc) else {
            return;
        };
        let source_line = program
            .source
            .as_ref()
            .and_then(|source| source.source_line(position.line))
            .map(str::to_owned);
        self.0.payload.source_location = Some(ErrorSourceLocation {
            file: program.source_name.clone(),
            line: position.line,
            column: position.column,
            source_line,
        });
    }

    pub fn thrown_value(&self) -> Option<Value> {
        self.0.payload.thrown
    }

    pub(crate) fn replace_thrown_value(&mut self, value: Value) {
        self.0.payload.thrown = Some(value);
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
                source_location: None,
            }),
        })
    }

    /// Inspect a typed WebAssembly trap without treating it as a JS throw.
    pub fn wasm_trap(&self) -> Option<crate::WasmTrap> {
        match self.0.payload.description {
            ErrorDescription::WasmTrap(trap) => Some(trap),
            ErrorDescription::Text { .. }
            | ErrorDescription::WasmLink(_)
            | ErrorDescription::DataClone(_)
            | ErrorDescription::WasmException
            | ErrorDescription::ExecutionTerminated => None,
        }
    }

    pub(crate) fn wasm_trap_error(trap: crate::WasmTrap) -> Self {
        Self(ErrorMessage {
            payload: Box::new(ErrorPayload {
                description: ErrorDescription::WasmTrap(trap),
                thrown: None,
                source_location: None,
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
                source_location: None,
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
            ErrorDescription::DataClone(message) => format!("DataCloneError: {message}"),
            ErrorDescription::WasmTrap(trap) => trap.to_string(),
            ErrorDescription::WasmException => "uncaught Wasm exception".into(),
            ErrorDescription::ExecutionTerminated => "execution terminated".into(),
        }
    }
}

/// The Function-constructor programs one VM offers the host's compile cache. Validator and
/// template compilers create a few large functions with the same text in every run (ajv's npm
/// scenarios create at most seven), which the cache saves compiling again (schema-utils: -9.5%
/// instructions); code generated with different text each time would otherwise write an entry
/// per call. Past the budget, programs still load from the cache but are no longer offered.
pub(super) const DYNAMIC_FUNCTIONS_KEPT: u16 = 64;

impl<H: Host> Vm<H> {
    pub(super) fn error_is_error(&self, value: Value) -> bool {
        self.heap
            .object_record(value)
            .is_some_and(Object::has_error_data)
    }

    pub(super) fn error_stack_getter(
        &mut self,
        program: &ResidualProgram,
        receiver: Value,
    ) -> Result<Value, JsError> {
        let records = match self.object_data(receiver).and_then(Object::stack_data) {
            Some(StackData::Formatted(value)) => return Ok(*value),
            Some(StackData::Captured { records, .. }) => records.clone(),
            _ => return Ok(Value::UNDEFINED),
        };

        let receiver_root = self.heap.root(receiver);
        let mut roots = Vec::with_capacity(2);
        let outcome = (|| {
            let formatter =
                match self.realm.intrinsics.error_intrinsics[&self.realm.globals].host_formatter {
                    Some(formatter) => Some(formatter),
                    None => match self.host.error_stack_formatter() {
                        Some(root) => Some(self.heap.root_value(root).ok_or_else(|| {
                            self.type_error(program, "Host stack formatter root expired".into())
                        })?),
                        None => None,
                    },
                };
            if let Some(formatter) = formatter {
                let formatter_root = self.heap.root(formatter);
                let global_root = self.heap.root(self.realm.globals);
                let result = (|| {
                    let sites = self.error_call_site_array(&records)?;
                    let sites_root = self.heap.root(sites);
                    let formatter = self.heap.root_value(formatter_root).unwrap();
                    let global = self.heap.root_value(global_root).unwrap();
                    let target = self.heap.root_value(receiver_root).unwrap();
                    let sites = self.heap.root_value(sites_root).unwrap();
                    let result = self.call_value(
                        program,
                        formatter,
                        Value::UNDEFINED,
                        &[global, target, sites],
                    );
                    self.heap.release_root(sites_root);
                    result
                })();
                self.heap.release_root(global_root);
                self.heap.release_root(formatter_root);
                return result;
            }

            let constructor =
                self.realm.intrinsics.error_intrinsics[&self.realm.globals].constructor;
            roots.push(self.heap.root(constructor));
            let prepare_atom = self.intern_atom("prepareStackTrace");
            let prepare = self.get_property(program, constructor, prepare_atom)?;
            roots.push(self.heap.root(prepare));
            let result = (|| {
                if self.is_function(prepare) {
                    let sites = self.error_call_site_array(&records)?;
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
        if let Ok(value) = outcome {
            let target = self.heap.root_value(receiver_root).unwrap();
            if let Some(object) = self.object_data_mut(target) {
                object.set_stack_data(StackData::Formatted(value));
            }
        }
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
        let header = self.error_to_string(program, target)?;
        let header = self.coerce_js_string(program, header)?;
        let mut stack = super::wtf16::JsStringBuilder::default();
        stack.append(&header);
        let prefix = JsString::from_str("\n    at ");
        for record in records {
            stack.append(&prefix);
            stack.append(&self.format_call_site(program, record)?);
        }
        let stack = self.string_build_result(program, stack.finish())?;
        Ok(self.heap.alloc_string(stack))
    }

    fn error_call_site_array(&mut self, records: &[CallSiteRecord]) -> Result<Value, JsError> {
        let mut roots = Vec::with_capacity(records.len());
        let result = (|| {
            let mut sites = Vec::with_capacity(records.len());
            for record in records {
                let intrinsics = self.realm.intrinsics.error_intrinsics[&self.realm.globals];
                let mut object = Self::empty_object(intrinsics.call_site_prototype);
                self.capture_constructed_name(&mut object, intrinsics.call_site_constructor, true);
                let site = self.heap.alloc(Cell::Object(object));
                let root = self.heap.root(site);
                roots.push(root);
                self.object_data_mut(site)
                    .expect("CallSite is an ordinary object")
                    .set_stack_data(StackData::CallSite(record.clone()));
                sites.push(self.heap.root_value(root).unwrap());
            }
            Ok(self.heap.alloc(Cell::Array {
                object: crate::heap::ArrayObject::new(Self::empty_object(self.array_proto), sites),
            }))
        })();
        for root in roots {
            self.heap.release_root(root);
        }
        result
    }

    /// Inspect descriptor identity without invoking accessors or Proxy traps.
    fn call_site_property_matches(
        &self,
        receiver: Value,
        key: Atom,
        function: Value,
    ) -> Option<bool> {
        let attributes = self.property_attributes(receiver, PropertyKey::string(key));
        if let Some(attributes) = attributes.filter(|attributes| attributes.accessor) {
            return Some(
                attributes.getter == Some(function) || attributes.setter == Some(function),
            );
        }
        self.own_property(receiver, key)
            .map(|value| value == function)
    }

    fn call_site_method_name(
        &mut self,
        record: &CallSiteRecord,
    ) -> Result<Option<JsString>, JsError> {
        let CallSiteName::Callable(function) = record.name else {
            return Ok(None);
        };
        if record.this_value.is_null() || record.this_value.is_undefined() {
            return Ok(None);
        }
        let receiver = self.box_object(record.this_value)?;
        if let Some(original) = self.original_function_name(function) {
            let name = self.atom_value(original);
            let units = name.units();
            let accessor_prefix = ["get ", "set "]
                .into_iter()
                .find(|prefix| units.iter().take(prefix.len()).eq(prefix.encode_utf16()));
            let name = if let Some(prefix) = accessor_prefix {
                JsString::from_unit_run(units.slice(prefix.len()..units.len()))
            } else {
                name
            };
            if let Some(key) = self.lookup_js_atom(&name) {
                let mut current = receiver;
                while let Some(object) = self.object_data(current) {
                    if self.proxy_parts(current).is_some() {
                        break;
                    }
                    if let Some(matches) = self.call_site_property_matches(current, key, function) {
                        if matches {
                            return Ok(Some(name));
                        }
                        break;
                    }
                    current = object.proto();
                }
            }
        }
        let mut found = None;
        let mut current = receiver;
        while let Some(object) = self.object_data(current) {
            if self.proxy_parts(current).is_some() {
                break;
            }
            let prototype = object.proto();
            for key in self.own_named_keys(current) {
                if self.atom_class(key).contains(AtomClass::ARRAY_INDEX)
                    || self
                        .property_attributes(current, PropertyKey::string(key))
                        .is_some_and(|attributes| !attributes.enumerable)
                {
                    continue;
                }
                if self.call_site_property_matches(current, key, function) == Some(true) {
                    if found.is_some_and(|previous| previous != key) {
                        return Ok(None);
                    }
                    found = Some(key);
                }
            }
            current = prototype;
        }
        Ok(found.map(|name| self.atom_value(name)))
    }

    fn call_site_type_name(
        &mut self,
        record: &CallSiteRecord,
    ) -> Result<Option<JsString>, JsError> {
        let receiver = record.this_value;
        if record.is_constructor
            || receiver.is_null()
            || receiver.is_undefined()
            || self
                .realm
                .intrinsics
                .function_constructors
                .contains_key(&(receiver, Native::Function))
        {
            return Ok(None);
        }
        let receiver = self.box_object(receiver)?;
        if self.proxy_parts(receiver).is_some() {
            return Ok(Some(JsString::from_str("Proxy")));
        }
        if matches!(self.heap.function(receiver), Some(FunctionRecord { kind, .. }) if !matches!(kind, FunctionKind::Native(Native::FunctionBoundCall)))
        {
            if let Some(name) = self.debug_function_name(receiver) {
                if !name.units().is_empty() {
                    return Ok(Some(name));
                }
            }
        }
        Ok(self.constructor_name(receiver))
    }

    /// A source projection of a genuine activation, without evaluating guest hooks.
    pub(super) fn source_call_site_record(
        &self,
        frame: &Frame,
        hide_identity: bool,
    ) -> Option<(CallSiteRecord, bool)> {
        if frame.context == CallContext::Internal && frame.function != ROOT_FUNCTION_ID {
            return None;
        }
        let residual = self.programs.get(frame.program)?;
        let function = residual.function_get(frame.function as usize)?;
        let pc = u32::try_from(frame.source_instruction_pc()).unwrap_or(u32::MAX);
        let position = if let Some((site, part)) = frame.source_field {
            if let Some((line, column)) = residual
                .field_sites
                .get(site)
                .and_then(|site| site.source(part))
            {
                residual.adjust_source_position(crate::bytecode::SourcePosition {
                    pc,
                    line,
                    column,
                })?
            } else {
                residual.source_position(frame.function, pc)?
            }
        } else {
            residual.source_position(frame.function, pc)?
        };
        let name = frame.context.callable().map_or_else(
            || {
                function.name.map_or(CallSiteName::Anonymous, |name| {
                    CallSiteName::Static(self.atom_value(name))
                })
            },
            CallSiteName::Callable,
        );
        Some((
            CallSiteRecord {
                location: CallSiteLocation::Source {
                    file_name: residual.source_name.clone(),
                    line: position.line,
                    column: position.column,
                },
                name,
                hide_identity,
                is_constructor: frame.context.is_constructor(),
                eval_origin: self.programs.eval_origin(frame.program),
                this_value: if frame.this == Value::DELETED {
                    Value::UNDEFINED
                } else {
                    frame.this
                },
            },
            residual.kind == crate::bytecode::ProgramKind::Module,
        ))
    }

    /// Snapshot the actual compiling caller without evaluating guest name hooks.
    pub(super) fn capture_eval_origin(
        &mut self,
        context: &ResidualProgram,
    ) -> Result<JsString, JsError> {
        let fallback = || JsString::from_str(super::program_store::ANONYMOUS_EVAL_ORIGIN);
        let (mut record, module, name) = {
            let Some(frame) = self.frames.last() else {
                return Ok(fallback());
            };
            let Some((record, module)) = self.source_call_site_record(frame, false) else {
                return Ok(fallback());
            };
            let Some(program) = self.programs.get(frame.program) else {
                return Ok(fallback());
            };
            let Some(function) = program.function_get(frame.function as usize) else {
                return Ok(fallback());
            };
            let primary_constructor = self
                .programs
                .is_function_constructor_body(frame.program, frame.function);
            let name = function
                .name
                .or_else(|| {
                    (!primary_constructor)
                        .then(|| {
                            frame
                                .context
                                .callable()
                                .and_then(|callee| self.original_function_name(callee))
                        })
                        .flatten()
                })
                .map(|name| self.atom_value(name));
            (record, module, name.filter(|name| !name.units().is_empty()))
        };
        Self::map_call_site_source(&mut self.host, &mut record, module);
        let mut text = super::wtf16::JsStringBuilder::default();
        text.append(&JsString::from_str("eval at "));
        text.append(&name.unwrap_or_else(|| JsString::from_str("<anonymous>")));
        text.append(&JsString::from_str(" ("));
        if let Some(origin) = &record.eval_origin {
            text.append(origin);
        } else if let CallSiteLocation::Source {
            file_name,
            line,
            column,
        } = &record.location
        {
            text.append(&JsString::from_str(&format!("{file_name}:{line}:{column}")));
        } else {
            text.append(&JsString::from_str("unknown source"));
        }
        text.append(&JsString::from_str(")"));
        self.string_build_result(context, text.finish())
    }

    pub(super) fn map_call_site_source(host: &mut H, record: &mut CallSiteRecord, module: bool) {
        if let CallSiteLocation::Source { file_name, .. } = &mut record.location {
            *file_name = host.stack_source_name(file_name, module);
        }
    }

    pub(super) fn error_capture_stack_trace(
        &mut self,
        program: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.materialize_deferred_lane();
        let target = args.first().copied().unwrap_or(Value::UNDEFINED);
        if !self.is_object_like(target) {
            return Err(self.type_error(program, "invalid_argument".into()));
        }
        let constructor_opt = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let trim_constructor =
            constructor_opt != Value::UNDEFINED && self.is_function(constructor_opt);
        let capture_limit = self.error_stack_trace_limit();
        let limit = capture_limit.unwrap_or(0);
        let mut records = Vec::with_capacity(self.frames.len().min(limit));
        let mut source = None;
        let mut source_captured = false;
        let mut native_frames = self.native_stack_frames.iter().rev().peekable();
        let mut constructor_found = !trim_constructor;
        let mut hide_identity = false;
        for (depth, frame) in self.frames.iter().enumerate().rev() {
            while records.len() < limit
                && native_frames
                    .peek()
                    .is_some_and(|(native_depth, _)| *native_depth > depth)
            {
                let (_, name) = native_frames.next().unwrap();
                if constructor_found {
                    records.push(CallSiteRecord {
                        location: CallSiteLocation::Native,
                        name: CallSiteName::Static(JsString::from_str(name)),
                        this_value: Value::UNDEFINED,
                        hide_identity: true,
                        is_constructor: false,
                        eval_origin: None,
                    });
                }
            }
            if records.len() >= limit {
                break;
            }
            // A Script root has no exposed callee identity, but its compiler
            // source position is still a visible stack frame. Synthetic helper
            // activations remain hidden.
            if frame.context == CallContext::Internal && frame.function != ROOT_FUNCTION_ID {
                continue;
            }
            let Some(residual) = self.programs.get(frame.program) else {
                continue;
            };
            let Some(function) = residual.function_get(frame.function as usize) else {
                continue;
            };
            hide_identity |= function.strict;
            if !constructor_found {
                constructor_found = frame.context.callable() == Some(constructor_opt);
                continue;
            }
            if let Some((mut record, module)) = self.source_call_site_record(frame, hide_identity) {
                if !source_captured {
                    source_captured = true;
                    // Share immutable compiler text, not a copied line per Error.
                    // Captured positions remain valid after its Script is collected.
                    source = residual.source.clone();
                }
                Self::map_call_site_source(&mut self.host, &mut record, module);
                records.push(record);
            }
        }

        if constructor_found {
            for (_, name) in native_frames.take(limit.saturating_sub(records.len())) {
                records.push(CallSiteRecord {
                    location: CallSiteLocation::Native,
                    name: CallSiteName::Static(JsString::from_str(name)),
                    this_value: Value::UNDEFINED,
                    hide_identity: true,
                    is_constructor: false,
                    eval_origin: None,
                });
            }
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
            self.install_error_stack(
                program,
                target,
                match capture_limit {
                    Some(_) => StackData::Captured { records, source },
                    None => StackData::Formatted(Value::UNDEFINED),
                },
            )?;
            Ok(Value::UNDEFINED)
        })();
        self.heap.release_root(target_root);
        result
    }

    fn define_error_stack_property(
        &mut self,
        program: &ResidualProgram,
        target: Value,
    ) -> Result<(), JsError> {
        let intrinsics = self.realm.intrinsics.error_intrinsics[&self.realm.globals];
        let getter = intrinsics.stack_getter;
        let setter = intrinsics.stack_setter;
        let key = self.heap.alloc_string("stack".into());
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
        Ok(())
    }

    pub(super) fn install_error_stack(
        &mut self,
        program: &ResidualProgram,
        target: Value,
        data: StackData,
    ) -> Result<(), JsError> {
        self.define_error_stack_property(program, target)?;
        self.object_data_mut(target)
            .expect("stack target remains object-like")
            .set_stack_data(data);
        Ok(())
    }

    fn error_stack_trace_limit(&mut self) -> Option<usize> {
        let mut constructor =
            self.realm.intrinsics.error_intrinsics[&self.realm.globals].constructor;
        let atom = self.intern_atom("stackTraceLimit");
        loop {
            if self
                .property_attributes(constructor, PropertyKey::string(atom))
                .is_some_and(|attributes| attributes.accessor)
            {
                return None;
            }
            if let Some(value) = self.own_property(constructor, atom) {
                let number = value.as_number()?;
                return Some(if number.is_nan() || number <= 0.0 {
                    0
                } else if !number.is_finite() {
                    usize::MAX
                } else {
                    number.floor().min(usize::MAX as f64) as usize
                });
            }
            constructor = self.object_data(constructor)?.proto();
        }
    }

    pub(super) fn call_site_name(&self, record: &CallSiteRecord) -> Option<JsString> {
        let name = match &record.name {
            CallSiteName::Callable(function) => self.debug_function_name(*function),
            CallSiteName::Static(name) => Some(name.clone()),
            CallSiteName::Anonymous => None,
        };
        name.filter(|name| !name.units().is_empty()).or_else(|| {
            record
                .eval_origin
                .as_ref()
                .map(|_| JsString::from_str("eval"))
        })
    }

    fn format_call_site(
        &mut self,
        program: &ResidualProgram,
        record: &CallSiteRecord,
    ) -> Result<JsString, JsError> {
        let location = match &record.location {
            CallSiteLocation::Source {
                file_name,
                line,
                column,
            } => {
                let mut text = super::wtf16::JsStringBuilder::default();
                if let Some(origin) = &record.eval_origin {
                    text.append(origin);
                    text.append(&JsString::from_str(", <anonymous>"));
                } else {
                    text.append(&JsString::from_str(file_name));
                }
                text.append(&JsString::from_str(&format!(":{line}:{column}")));
                text.finish()
            }
            CallSiteLocation::Native => Ok(JsString::from_str("native")),
        };
        let location = self.string_build_result(program, location)?;
        let name = self.call_site_name(record).or_else(|| {
            record
                .is_constructor
                .then(|| JsString::from_str("<anonymous>"))
        });
        let Some(name) = name else {
            return Ok(location);
        };
        let mut text = super::wtf16::JsStringBuilder::default();
        if record.is_constructor {
            text.append(&JsString::from_str("new "));
        }
        text.append(&name);
        text.append(&JsString::from_str(" ("));
        text.append(&location);
        text.append(&JsString::from_str(")"));
        self.string_build_result(program, text.finish())
    }

    pub(super) fn call_site_constructor(
        &mut self,
        program: &ResidualProgram,
    ) -> Result<Value, JsError> {
        let message = self.heap.alloc_string(JsString::from_str("Not supported"));
        let error = self.construct_error_native(program, Native::Error, &[message])?;
        Err(JsError::thrown(error, "Error: Not supported".into()))
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
                let method = CALL_SITE_METHODS
                    .iter()
                    .find(|(_, candidate)| *candidate == native)
                    .map(|(name, _)| *name)
                    .expect("CallSite method");
                let message = if matches!(self.heap.get(receiver), Some(Cell::Proxy { .. })) {
                    format!("Method {method} called on incompatible receiver #<Object>")
                } else if !self.is_object_like(receiver) {
                    format!(
                        "Method {method} called on incompatible receiver {}",
                        self.detail_string(receiver)
                    )
                } else {
                    format!("CallSite method {method} expects CallSite as receiver")
                };
                return Err(self.type_error(program, message));
            }
        };
        match native {
            Native::CallSiteGetFileName if record.eval_origin.is_some() => Ok(Value::UNDEFINED),
            Native::CallSiteGetFileName => Ok(match record.location {
                CallSiteLocation::Source { file_name, .. } => {
                    self.heap.alloc_string(file_name.into())
                }
                CallSiteLocation::Native => Value::NULL,
            }),
            Native::CallSiteGetThis => Ok(if record.hide_identity {
                Value::UNDEFINED
            } else {
                record.this_value
            }),
            Native::CallSiteGetFunction => Ok(match record.name {
                CallSiteName::Callable(function) if !record.hide_identity => function,
                _ => Value::UNDEFINED,
            }),
            Native::CallSiteGetMethodName => Ok(self
                .call_site_method_name(&record)?
                .map(|name| self.heap.alloc_string(name))
                .unwrap_or(Value::NULL)),
            Native::CallSiteGetFunctionName => Ok(self
                .call_site_name(&record)
                .map(|name| self.heap.alloc_string(name))
                .unwrap_or(Value::NULL)),
            Native::CallSiteGetLineNumber => Ok(match record.location {
                CallSiteLocation::Source { line, .. } => Value::number(f64::from(line)),
                CallSiteLocation::Native => Value::NULL,
            }),
            Native::CallSiteGetColumnNumber => Ok(match record.location {
                CallSiteLocation::Source { column, .. } => Value::number(f64::from(column)),
                CallSiteLocation::Native => Value::NULL,
            }),
            Native::CallSiteIsEval => Ok(if record.eval_origin.is_some() {
                Value::TRUE
            } else {
                Value::FALSE
            }),
            Native::CallSiteIsConstructor => Ok(if record.is_constructor {
                Value::TRUE
            } else {
                Value::FALSE
            }),
            Native::CallSiteIsNative => {
                Ok(if matches!(record.location, CallSiteLocation::Native) {
                    Value::TRUE
                } else {
                    Value::FALSE
                })
            }
            Native::CallSiteGetTypeName => Ok(self
                .call_site_type_name(&record)?
                .map(|name| self.heap.alloc_string(name))
                .unwrap_or(Value::NULL)),
            Native::CallSiteGetEvalOrigin => Ok(record
                .eval_origin
                .map(|origin| self.heap.alloc_string(origin))
                .unwrap_or(Value::UNDEFINED)),
            Native::CallSiteToString => {
                let text = self.format_call_site(program, &record)?;
                Ok(self.heap.alloc_string(text))
            }
            _ => Err(JsError::validation(
                "invalid CallSite native dispatch".into(),
            )),
        }
    }

    pub(super) fn error_stack_setter(
        &mut self,
        _program: &ResidualProgram,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        if let Some(object) = self.object_data_mut(receiver) {
            if matches!(
                object.stack_data(),
                Some(StackData::Captured { .. } | StackData::Formatted(_))
            ) {
                object.set_stack_data(StackData::Formatted(value));
            }
        }
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
            JsString::from_str("Error")
        } else {
            self.coerce_js_string(program, name)?
        };
        let message = self.get_property(program, receiver, message_atom)?;
        let message = if message.is_undefined() {
            JsString::from_str("")
        } else {
            self.coerce_js_string(program, message)?
        };
        let text = if name.len() == 0 {
            message
        } else if message.len() == 0 {
            name
        } else {
            let mut text = super::wtf16::JsStringBuilder::default();
            text.append(&name);
            text.append(&JsString::from_str(": "));
            text.append(&message);
            self.string_build_result(program, text.finish())?
        };
        Ok(self.heap.alloc_string(text))
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
            .and_then(|value| self.heap.string(value).map(|name| name.to_string()))
            .unwrap_or_default();
        let message = self
            .get_property(program, value, message_atom)
            .ok()
            .and_then(|value| self.heap.string(value).map(|message| message.to_string()))
            .unwrap_or_default();
        match (name.is_empty(), message.is_empty()) {
            (true, true) => display,
            (true, false) => message,
            (false, true) => name,
            (false, false) => format!("{name}: {message}"),
        }
    }

    pub(super) fn type_error(&mut self, program: &ResidualProgram, text: String) -> JsError {
        let message = self.heap.alloc_string(JsString::from_str(&text));
        let object = self
            .construct_error_native(program, Native::TypeError, &[message])
            .unwrap_or(Value::UNDEFINED);
        JsError::thrown(object, text)
    }

    /// The ReferenceError for reading `atom` in its temporal dead zone, naming the binding as
    /// the source spells it.
    pub(super) fn uninitialized_binding_error(
        &mut self,
        program: &ResidualProgram,
        atom: Atom,
    ) -> JsError {
        let name = crate::bytecode::source_binding_name(self.atom_name(atom)).to_owned();
        self.reference_error(
            program,
            format!("Cannot access '{name}' before initialization"),
        )
    }

    /// A host's module-load failure as its error object, with the host's `code` as an
    /// ordinary data property.
    pub(super) fn module_load_error(
        &mut self,
        program: &ResidualProgram,
        error: crate::ModuleLoadError,
    ) -> JsError {
        let native = match error.kind {
            crate::ModuleLoadErrorKind::Error => Native::Error,
            crate::ModuleLoadErrorKind::TypeError => Native::TypeError,
        };
        let message = self.heap.alloc_string(JsString::from_str(&error.message));
        let object = match self.construct_error_native(program, native, &[message]) {
            Ok(object) => object,
            Err(failure) => return failure,
        };
        let code = error.code.map(|code| ("code", code.to_owned()));
        for (name, value) in code.into_iter().chain(error.fields) {
            let name = self.heap.alloc_string(JsString::from_str(name));
            let value = self.heap.alloc_string(JsString::from_str(&value));
            let defined = self.to_property_key(program, name).and_then(|key| {
                let record = super::object_descriptors::PropertyDescriptorRecord::data(value);
                self.define_own_property_record(program, object, key, record)
            });
            if let Err(failure) = defined {
                return failure;
            }
        }
        JsError::thrown(object, error.message)
    }

    pub(super) fn reference_error(&mut self, program: &ResidualProgram, text: String) -> JsError {
        let message = self.heap.alloc_string(JsString::from_str(&text));
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
            .alloc_string(crate::stack::STACK_EXHAUSTED_MESSAGE.into());
        self.set_builtin_value_named(object, "message", message)
            .expect("fresh error object accepts message");
        JsError::thrown(object, crate::stack::STACK_EXHAUSTED_MESSAGE.into())
    }

    pub(super) fn range_error(&mut self, program: &ResidualProgram, text: String) -> JsError {
        let message = self.heap.alloc_string(JsString::from_str(&text));
        let object = self
            .construct_error_native(program, Native::RangeError, &[message])
            .unwrap_or(Value::UNDEFINED);
        JsError::thrown(object, text)
    }

    pub(super) fn uri_error(&mut self, program: &ResidualProgram, text: String) -> JsError {
        let message = self.heap.alloc_string(JsString::from_str(&text));
        let object = self
            .construct_error_native(program, Native::URIError, &[message])
            .unwrap_or(Value::UNDEFINED);
        JsError::thrown(object, text)
    }

    pub(super) fn thrown_value_for(&mut self, program: &ResidualProgram, error: JsError) -> Value {
        if let Some(value) = error.thrown_value() {
            return value;
        }
        // Traps and link failures surface as the `WebAssembly` namespace's errors.
        let wasm_error = if error.wasm_trap().is_some() {
            Some(Native::WasmRuntimeError)
        } else if error.wasm_link_error().is_some() {
            Some(Native::WasmLinkError)
        } else {
            None
        };
        if let Some(native) = wasm_error {
            return self
                .wasm_js_error(program, native, error.into_message())
                .thrown_value()
                .unwrap_or(Value::UNDEFINED);
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
        let message = self.heap.alloc_string(JsString::from_str(&text));
        self.construct_error_native(program, native, &[message])
            .unwrap_or(Value::UNDEFINED)
    }

    pub(super) fn box_primitive_object(&mut self, value: Value) -> Result<Value, JsError> {
        if !self.heap.is_string(value)
            && !matches!(
                self.heap.get(value),
                Some(Cell::Symbol(_) | Cell::BigInt(_))
            )
            && value.as_number().is_none()
            && value.as_bool().is_none()
        {
            return Ok(self.object());
        }
        let prototype = self.primitive_prototype(value).unwrap_or(self.object_proto);
        let object = self.heap.alloc(Cell::Object(Self::empty_object(prototype)));
        if let Some(text) = self.heap.string(value).cloned() {
            for (index, unit) in text.units().iter().enumerate() {
                let key = self.intern_atom(&index.to_string());
                let character = self
                    .heap
                    .alloc_string(super::wtf16::JsString::from_units(&[unit]));
                self.set_property_with_attributes(
                    object,
                    key,
                    character,
                    PropertyAttributes {
                        writable: false,
                        enumerable: true,
                        configurable: false,
                        accessor: false,
                        getter: None,
                        setter: None,
                    },
                )?;
            }
            let length = self.length_atom;
            self.set_property_with_attributes(
                object,
                length,
                Value::number(text.units().len() as f64),
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: false,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            )?;
        }
        self.object_data_mut(object)
            .expect("primitive wrapper")
            .set_boxed_primitive(value);
        Ok(object)
    }

    pub(super) fn box_bigint_object(
        &mut self,
        _p: &ResidualProgram,
        value: Value,
    ) -> Result<Value, JsError> {
        let realm = self.active_native_env().unwrap_or(self.realm.globals);
        let prototype = self
            .realm
            .intrinsics
            .builtin_prototypes
            .get(&(realm, Native::BigInt))
            .copied()
            .or_else(|| self.primitive_prototype(value))
            .unwrap_or(self.object_proto);
        let object = self.heap.alloc(Cell::Object(Self::empty_object(prototype)));
        self.object_data_mut(object)
            .expect("bigint wrapper")
            .set_boxed_primitive(value);
        Ok(object)
    }

    pub(super) fn install_host_globals(
        &mut self,
        program: &ResidualProgram,
    ) -> Result<(), JsError> {
        self.global(program, "globalThis", self.realm.globals)?;
        let function = self.native_value(Native::Function);
        self.realm
            .intrinsics
            .function_constructors
            .insert((self.realm.globals, Native::Function), function);
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
        let prototype_length = self.length_atom;
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
        let length = self.length_atom;
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
        self.install_symbol_description(symbol_prototype, description)?;
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
            let native_function = self.native_value(value_of);
            self.set_named(program, prototype, "valueOf", native_function)?;
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
                    let native_is_h_t_m_l_d_d_a = self.native_value(Native::IsHTMLDDA);
                    self.set_named(program, realm, "IsHTMLDDA", native_is_h_t_m_l_d_d_a)?;
                    continue;
                }
                CapabilityId::WriteLine | CapabilityId::ClockMillis => continue,
            };
            let native_function = self.native_value(native);
            self.global(program, global.name, native_function)?;
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
        let name = self.heap.alloc_string("AbstractModuleSource".into());
        self.set_named(program, constructor, "name", name)?;
        let name_atom = self.intern_atom("name");
        let prototype_atom = self.prototype_atom();
        let constructor_atom = self.runtime_atoms.constructor;
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
            .get(WellKnownSymbol::ToStringTag)
            .expect("well-known toStringTag symbol installed before module source intrinsics");
        self.set_symbol_property(prototype, to_string_tag, Value::UNDEFINED)?;
        let native_abstract_module_source_to_string_tag =
            self.native_value(Native::AbstractModuleSourceToStringTag);
        self.set_property_attributes(
            prototype,
            PropertyKey::symbol(to_string_tag),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: true,
                getter: Some(native_abstract_module_source_to_string_tag),
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
                let message = self
                    .heap
                    .alloc_string("hashbang is not allowed in Function source".into());
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
        let parameters = argument_strings.join(",");
        let kind = match native {
            Native::AsyncFunction => crate::compile::DynamicFunctionKind::Async,
            Native::GeneratorFunction => crate::compile::DynamicFunctionKind::Generator,
            Native::AsyncGeneratorFunction => crate::compile::DynamicFunctionKind::AsyncGenerator,
            _ => crate::compile::DynamicFunctionKind::Ordinary,
        };
        let dynamic = crate::compile::DynamicFunctionSource::new(&parameters, &source, kind);
        let source_name = crate::compile::DYNAMIC_FUNCTION_SOURCE_NAME;
        // A Function body compiles from its text alone, so its compiled form goes through the
        // host's compile cache as a host script's does (`eval_host_script`).
        let kept_key = [self.specialization_part(), dynamic.text.as_str()];
        let kept = self
            .host
            .kept_compiled_program(source_name, &kept_key)
            .and_then(|bytes| {
                let text = crate::bytecode::SourceText::Shared(std::sync::Arc::from(
                    dynamic.text.as_str(),
                ));
                let specialized = self.specialized;
                self.decode_cached_program(bytes, text, 0, source_name, specialized)
                    .ok()
            });
        let compiled = kept.is_none();
        let residual = match kept {
            Some(residual) => residual,
            None => crate::Engine::specialize_function_constructor(&dynamic, source_name, &*self)
                .map_err(|diagnostics| {
                if diagnostics
                    .iter()
                    .any(crate::compile::Diagnostic::is_stack_exhausted)
                {
                    return self.stack_exhaustion_error();
                }
                let message = diagnostics
                    .first()
                    .map_or("invalid Function source".to_owned(), |diagnostic| {
                        diagnostic.syntax_message().to_owned()
                    });
                self.syntax_error_result(program, &message)
                    .expect_err("dynamic Function syntax errors must throw")
            })?,
        };
        let Some(program_id) = self.store_dynamic_program(residual) else {
            return Err(self.type_error(program, "dynamic program store is full".into()));
        };
        if compiled
            && self.dynamic_functions_to_keep > 0
            && let Some(compiled) = self.programs.get(program_id)
            && let Some(bytes) = self.encode_cached_program(&compiled)
        {
            self.host
                .keep_compiled_program(source_name, &kept_key, &bytes);
            self.dynamic_functions_to_keep -= 1;
        }
        let origin = self.capture_eval_origin(program)?;
        self.programs.set_origin(
            program_id,
            super::program_store::CompilationOrigin::FunctionConstructor { caller: origin },
        );
        let residual = self
            .programs
            .get(program_id)
            .ok_or_else(|| self.type_error(program, "dynamic program is unavailable".into()))?;
        let active_program = self.replace_active_program(program_id);
        let result = (|| {
            let function = residual
                .functions
                .iter()
                .enumerate()
                .find(|(_, function)| function.header().parent == Some(super::ROOT_FUNCTION_ID))
                .map(|(id, _)| id as u32)
                .ok_or_else(|| {
                    self.type_error(program, "dynamic Function body is unavailable".into())
                })?;
            self.closure_in_realm(&residual, function, Value::NULL, function_realm)
        })();
        self.set_active_program(active_program);
        let function = result?;
        self.set_function_name_text(
            function,
            crate::compile::DYNAMIC_FUNCTION_PUBLIC_NAME.to_owned(),
        );
        let function_source = kind.source(&parameters, &source);
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
            return Ok(self.heap.alloc(Cell::bigint(integer)));
        }
        if let Some(value) = primitive.as_bool() {
            return Ok(self.heap.alloc(Cell::bigint(u8::from(value).into())));
        }
        if let Some(text) = self.heap.string(primitive) {
            let parsed = crate::bigint::parse_string(text.host_string());
            return match parsed {
                Some(value) => Ok(self.heap.alloc(Cell::bigint(value))),
                None => self.syntax_error_result(program, "invalid BigInt value"),
            };
        }
        Err(self.type_error(program, "cannot convert value to BigInt".into()))
    }

    pub(super) fn create_realm(&mut self, program: &ResidualProgram) -> Result<Value, JsError> {
        let global = self.create_realm_global(program)?;
        let realm = self.install_realm_host_api(program, global)?;
        for binding in self.host.globals() {
            if binding.capability == CapabilityId::CreateRealm {
                self.set_builtin_value_named(global, binding.name, realm)?;
            }
        }
        Ok(realm)
    }

    /// Creates a realm's global object with its intrinsics, without host APIs.
    pub(super) fn create_realm_global(
        &mut self,
        program: &ResidualProgram,
    ) -> Result<Value, JsError> {
        let global = self.object();
        // Compiled code reaches its helper natives through internal global names.
        let creator = self.realm.globals;
        for atom in self.internal_named_keys(creator) {
            if let Some(value) = self.own_property(creator, atom) {
                let attributes = self.property_attributes(creator, PropertyKey::string(atom));
                self.set_property(global, atom, value)?;
                if let Some(attributes) = attributes {
                    self.set_property_attributes(global, PropertyKey::string(atom), attributes);
                }
            }
        }
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
        self.set_builtin_value_named(type_error_prototype, "constructor", type_error)?;
        let type_error_name = self.heap.alloc_string("TypeError".into());
        self.set_builtin_value_named(type_error_prototype, "name", type_error_name)?;
        let empty_message = self.heap.alloc_string("".into());
        self.set_builtin_value_named(type_error_prototype, "message", empty_message)?;
        self.set_builtin_function_length(type_error, 1.0)?;
        self.set_builtin_value_named(global, "TypeError", type_error)?;
        let eval = self.native_with_realm(Native::Eval, global, global);
        self.set_builtin_function_value_named(global, "eval", eval)?;
        let function = self.native_with_realm(Native::Function, global, global);
        self.set_named(program, global, "Function", function)?;
        // %Object.prototype% ends every prototype chain of the realm.
        let object_prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(Value::NULL)));
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
            .set_proto(object_prototype);
        self.install_string_for_realm(program, global, object_prototype)?;
        self.install_number_for_realm(program, global, object_prototype)?;
        self.install_function_prototype_for_realm(global, object_prototype, function)?;
        self.install_typed_array_constructors_for_realm(program, global)?;
        self.install_weak_collections_for_realm(program, global, object_prototype)?;
        let set = self.native_with_realm(Native::Set, global, global);
        let set_prototype = self.install_set_prototype(program, set, object_prototype)?;
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
        let disposable_stack = self.native_with_realm(Native::DisposableStack, global, global);
        let async_disposable_stack =
            self.native_with_realm(Native::AsyncDisposableStack, global, global);
        self.install_disposable_stacks(
            program,
            global,
            object_prototype,
            disposable_stack,
            async_disposable_stack,
        )?;
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
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
        self.install_async_iterator_prototype(
            realm_async_iterator_proto,
            object_prototype,
            Some(global),
        )?;
        let realm_async_generator_proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(realm_async_iterator_proto)));
        let native_iterator_next = self.native_value(Native::IteratorNext);
        self.set_builtin_value_named(realm_async_generator_proto, "next", native_iterator_next)?;
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
            self.set_builtin_function_name(constructor, name)?;
            self.realm
                .intrinsics
                .function_constructors
                .insert((global, native), constructor);
            self.object_data_mut(constructor)
                .expect("realm dynamic function")
                .set_proto(function);
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
                let constructor_atom = self.runtime_atoms.constructor;
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
                let constructor_atom = self.runtime_atoms.constructor;
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
        self.set_builtin_value_named(object_prototype, "constructor", object)?;
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
            ("groupBy", Native::ObjectGroupBy),
            ("seal", Native::ObjectSeal),
            ("isSealed", Native::ObjectIsSealed),
            ("freeze", Native::ObjectFreeze),
            ("isFrozen", Native::ObjectIsFrozen),
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
        let array_prototype = self.heap.alloc(Cell::Array {
            object: crate::heap::ArrayObject::new(Self::empty_object(object_prototype), Vec::new()),
        });
        self.install_array_for_realm(program, global, array_prototype)?;
        let map = self.native_with_realm(Native::Map, global, global);
        let map_prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
        self.install_map(program, global, map, map_prototype)?;
        self.set_builtin_value_named(global, "Map", map)?;
        let prototype_atom = self.prototype_atom();
        let [weak_map_prototype, weak_set_prototype] = ["WeakMap", "WeakSet"].map(|name| {
            let atom = self.intern_atom(name);
            self.own_property(global, atom)
                .and_then(|constructor| self.own_property(constructor, prototype_atom))
                .expect("realm weak collections are installed")
        });
        self.install_collection_symbols(
            map,
            map_prototype,
            set,
            set_prototype,
            weak_map_prototype,
            weak_set_prototype,
        )?;
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
        self.set_builtin_value_named(global, "RegExp", regexp)?;
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
        let realm_error_name = self.heap.alloc_string("Error".into());
        self.set_builtin_value_named(realm_error_prototype, "name", realm_error_name)?;
        let realm_empty_message = self.heap.alloc_string("".into());
        self.set_builtin_value_named(realm_error_prototype, "message", realm_empty_message)?;
        let realm_is_error = self.native_with_realm(Native::ErrorIsError, global, global);
        self.set_builtin_function_name(realm_is_error, "isError")?;
        self.set_builtin_value_named(realm_error_constructor, "isError", realm_is_error)?;
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
            .set_proto(realm_error_prototype);
        self.object_data_mut(type_error)
            .expect("realm TypeError constructor")
            .set_proto(realm_error_constructor);
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
            let name_value = self.heap.alloc_string(name.into());
            self.set_builtin_value_named(prototype, "name", name_value)?;
            let empty_message = self.heap.alloc_string("".into());
            self.set_builtin_value_named(prototype, "message", empty_message)?;
            self.set_builtin_value_named(global, name, constructor)?;
            self.object_data_mut(constructor)
                .unwrap()
                .set_proto(realm_error_constructor);
        }
        for (name, native) in [
            ("parseFloat", Native::NumberParseFloat),
            ("parseInt", Native::ParseInt),
        ] {
            let intrinsic = self.native_with_realm(native, global, global);
            self.set_builtin_function_name(intrinsic, name)?;
            self.set_builtin_value_named(global, name, intrinsic)?;
        }
        self.install_realm_default_bindings(global)?;
        self.install_json_for_realm(program, global)?;
        self.install_symbol_for_realm(program, global, object_prototype)?;
        self.install_intl_for_realm(program, global, object_prototype)?;
        Ok(global)
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
            let value = match self.heap.function(value) {
                _ if *name == "Symbol" => value,
                Some(FunctionRecord {
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
        for name in ["Atomics", "Math", "Reflect", "WebAssembly"] {
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
        let constructor_length = self.length_atom;
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
        let prototype = self.heap.alloc_function(FunctionRecord {
            object: Self::empty_object(object_prototype),
            realm: global,
            kind: FunctionKind::Native(Native::FunctionPrototype),
            env: Value::NULL,
        });
        self.realm
            .intrinsics
            .builtin_prototypes
            .insert((global, Native::Function), prototype);
        self.realm
            .intrinsics
            .function_constructors
            .insert((global, Native::Function), constructor);
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
        let length = self.length_atom;
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
        if let Some(symbol) = self.well_known_symbols.get(WellKnownSymbol::HasInstance) {
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
            self.set_property_with_attributes(
                prototype,
                atom,
                Value::UNDEFINED,
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: true,
                    accessor: true,
                    getter: Some(throw_type_error),
                    setter: Some(throw_type_error),
                },
            )?;
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
        for (index, &(name, native)) in ERROR_CONSTRUCTORS.iter().enumerate() {
            let prototype = if index == 0 {
                error_prototype
            } else {
                self.heap
                    .alloc(Cell::Object(Self::empty_object(error_prototype)))
            };
            self.install_error_class(program, native, name, prototype)?;
        }
        self.set_builtin_named(program, error_prototype, "toString", Native::ErrorToString)?;
        let error_constructor = self.native_value(Native::Error);
        self.install_error_stack_api(program, error_constructor)?;
        self.set_builtin_named(program, error_constructor, "isError", Native::ErrorIsError)?;
        let realm_constructor = self.native_value(Native::RealmTypeError);
        let realm_prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(error_prototype)));
        self.set_named(program, realm_constructor, "prototype", realm_prototype)?;
        self.set_named(program, realm_prototype, "constructor", realm_constructor)?;
        let realm_name = self.heap.alloc_string(JsString::from_str("TypeError"));
        self.set_named(program, realm_prototype, "name", realm_name)?;
        Ok(())
    }

    /// Installs error constructor `native` named `name` with `prototype` as its intrinsic
    /// prototype; every class but `Error` itself inherits from the `Error` constructor.
    pub(super) fn install_error_class(
        &mut self,
        program: &ResidualProgram,
        native: Native,
        name: &str,
        prototype: Value,
    ) -> Result<(), JsError> {
        let constructor = self.native_value(native);
        self.realm
            .intrinsics
            .builtin_prototypes
            .insert((self.realm.globals, native), prototype);
        self.set_named_constant(program, constructor, "prototype", prototype)?;
        self.set_builtin_value_named(prototype, "constructor", constructor)?;
        self.set_builtin_function_name(constructor, name)?;
        let name_value = self.heap.alloc_string(JsString::from_str(name));
        self.set_builtin_value_named(prototype, "name", name_value)?;
        let empty_message = self.heap.alloc_string(JsString::from_str(""));
        self.set_builtin_value_named(prototype, "message", empty_message)?;
        if native != Native::Error {
            let error_constructor = self.native_value(Native::Error);
            if let Some(function) = self.object_data_mut(constructor) {
                function.set_proto(error_constructor);
            }
        }
        Ok(())
    }

    pub(super) fn install_errors(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        for &(name, native) in ERROR_CONSTRUCTORS {
            let native_function = self.native_value(native);
            self.global(program, name, native_function)?;
        }
        Ok(())
    }

    fn install_error_stack_api(
        &mut self,
        program: &ResidualProgram,
        constructor: Value,
    ) -> Result<(), JsError> {
        let global = self
            .heap
            .function(constructor)
            .map_or(self.realm.globals, |function| function.realm);
        let (stack_getter, stack_setter) = if constructor == self.native_value(Native::Error) {
            (
                self.native_value(Native::ErrorStackGetter),
                self.native_value(Native::ErrorStackSetter),
            )
        } else {
            (
                self.native_with_realm(Native::ErrorStackGetter, global, global),
                self.native_with_realm(Native::ErrorStackSetter, global, global),
            )
        };
        self.set_builtin_function_name(stack_getter, "")?;
        self.set_builtin_function_name(stack_setter, "")?;
        let call_site_constructor =
            if global == self.realm.globals && constructor == self.native_value(Native::Error) {
                self.native_value(Native::CallSiteConstructor)
            } else {
                self.native_with_realm(Native::CallSiteConstructor, Value::NULL, global)
            };
        self.set_builtin_function_name(call_site_constructor, "CallSite")?;
        let call_site_prototype = self.heap.alloc(Cell::Object(Self::empty_object(
            self.realm_object_prototype(global),
        )));
        self.set_named_constant(
            program,
            call_site_constructor,
            "prototype",
            call_site_prototype,
        )?;
        self.set_builtin_value_named(call_site_prototype, "constructor", call_site_constructor)?;
        for &(name, native) in CALL_SITE_METHODS {
            let function = if constructor == self.native_value(Native::Error) {
                self.native_value(native)
            } else {
                self.native_with_realm(native, Value::NULL, global)
            };
            self.set_builtin_function_name(function, name)?;
            let key = self.intern_atom(name);
            self.set_property_with_attributes(
                call_site_prototype,
                key,
                function,
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: false,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            )?;
        }
        self.realm.intrinsics.error_intrinsics.insert(
            global,
            ErrorRealmIntrinsics {
                host_formatter: None,
                constructor,
                stack_getter,
                stack_setter,
                call_site_constructor,
                call_site_prototype,
            },
        );
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
        let prototype = self.intrinsic_builtin_prototype(native);
        self.construct_error_with_prototype(program, native, args, prototype, None)
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
                let stack_target = vm.error_constructor_stack_target(native, new_target);
                vm.construct_error_with_prototype(program, native, args, prototype, stack_target)
            },
        )
    }

    // V8 ErrorUtils::Construct selects SKIP_UNTIL_SEEN only for a plain
    // JSFunction newTarget. Callable proxies and bound functions are distinct
    // callable forms. Our intrinsic constructor has no JS activation to find.
    fn error_constructor_stack_target(&self, native: Native, new_target: Value) -> Option<Value> {
        match self.heap.function(new_target) {
            Some(FunctionRecord {
                kind: FunctionKind::Native(target),
                realm,
                ..
            }) if *target == native && *realm == self.realm.globals => None,
            Some(FunctionRecord { kind, .. })
                if !matches!(kind, FunctionKind::Native(Native::FunctionBoundCall)) =>
            {
                Some(new_target)
            }
            _ => None,
        }
    }

    fn construct_error_with_prototype(
        &mut self,
        program: &ResidualProgram,
        native: Native,
        args: &[Value],
        prototype: Value,
        stack_target: Option<Value>,
    ) -> Result<Value, JsError> {
        let stack_target = stack_target.map(|target| self.heap.root(target));
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
            // Reserve the native stack accessor before message/cause fields,
            // then capture after their observable conversions have completed.
            self.define_error_stack_property(program, self.heap.root_value(root).unwrap())?;
            if let Some(message) = message {
                let value = self.heap.root_value(message).unwrap();
                let message = self.coerce_js_string(program, value)?;
                let message = self.heap.alloc_string(message);
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
            let object = self.heap.root_value(root).unwrap();
            let stack_target = stack_target
                .and_then(|root| self.heap.root_value(root))
                .unwrap_or(Value::UNDEFINED);
            self.error_capture_stack_trace(program, &[object, stack_target])?;
            Ok(self.heap.root_value(root).unwrap())
        })();
        for root in [Some(root), message, options, stack_target]
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
        let cause_key = self.heap.alloc_string("cause".into());
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
            self.define_error_stack_property(program, value)?;
            if let Some(message) = message {
                let message = self.heap.root_value(message).unwrap();
                let message = self.coerce_js_string(program, message)?;
                let message = self.heap.alloc_string(message);
                let value = self.heap.root_value(root).unwrap();
                self.set_builtin_value_named(value, "message", message)?;
            }
            if let Some(options) = options {
                self.install_error_cause(program, root, options)?;
            }
            let value = self.heap.root_value(root).unwrap();
            let target = self.heap.root_value(new_target).unwrap();
            let stack_target = self
                .error_constructor_stack_target(Native::AggregateError, target)
                .unwrap_or(Value::UNDEFINED);
            self.error_capture_stack_trace(program, &[value, stack_target])?;
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
