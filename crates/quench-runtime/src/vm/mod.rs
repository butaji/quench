use crate::intl::tolocale::value::{is_finite, to_number, to_string};
use crate::ops::{Builtin, HostCapabilityKind, HostCapabilityRef, Op, RealmId};
use crate::value::Value;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

mod vm_arithmetic;
mod vm_ops;

pub use crate::intl::tolocale::value::is_truthy;

pub type OutputSink = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Clone)]
pub struct VmContext {
    output_sink: Option<OutputSink>,
    realm: RealmId,
    capabilities: Vec<HostCapabilityRef>,
}

impl Default for VmContext {
    fn default() -> Self {
        Self {
            output_sink: None,
            realm: RealmId::ROOT,
            capabilities: Vec::new(),
        }
    }
}

thread_local! {
    static CURRENT_CONTEXT: RefCell<Option<VmContext>> = const { RefCell::new(None) };
}

struct ContextGuard {
    previous: Option<VmContext>,
}

impl ContextGuard {
    fn install(context: &VmContext) -> Self {
        let previous = CURRENT_CONTEXT.with(|current| current.replace(Some(context.clone())));
        Self { previous }
    }
}

impl Drop for ContextGuard {
    fn drop(&mut self) {
        CURRENT_CONTEXT.with(|current| current.replace(self.previous.take()));
    }
}

impl VmContext {
    pub fn with_output_sink(output_sink: OutputSink) -> Self {
        Self {
            output_sink: Some(output_sink),
            ..Self::default()
        }
    }

    pub fn for_realm(realm: RealmId, capabilities: Vec<HostCapabilityKind>) -> Self {
        let capabilities = capabilities
            .into_iter()
            .map(|kind| HostCapabilityRef { realm, kind })
            .collect();
        Self {
            realm,
            capabilities,
            ..Self::default()
        }
    }

    pub fn realm(&self) -> RealmId {
        self.realm
    }

    pub fn has_capability(&self, kind: HostCapabilityKind) -> bool {
        self.capabilities
            .iter()
            .any(|capability| capability.kind == kind)
    }

    pub fn emit_output(&self, text: &str) {
        if let Some(output_sink) = &self.output_sink {
            output_sink(text);
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum VmError {
    RegisterOutOfBounds(u16),
    MissingReturn,
    Break(Option<String>),
    Continue(Option<String>),
    NotCallable,
    EvalError(String),
    Thrown(Value),
    Suspended(Rc<crate::value::PromiseData>),
}

impl VmError {
    pub fn render(&self) -> String {
        match self {
            VmError::Thrown(value) => render_thrown(value),
            VmError::Suspended(_) => "Suspended".to_string(),
            other => format!("{other:?}"),
        }
    }
}

pub fn execute(ops: &[Op]) -> Result<Value, VmError> {
    execute_with_context(ops, &VmContext::default())
}

pub fn execute_with_registers(ops: &[Op], registers: Vec<Value>) -> Result<Value, VmError> {
    execute_with_registers_context(ops, registers, &VmContext::default())
}

pub fn execute_in_place(ops: &[Op], registers: &mut Vec<Value>) -> Result<Value, VmError> {
    execute_in_place_context(ops, registers, &VmContext::default())
}

pub fn execute_with_context(ops: &[Op], context: &VmContext) -> Result<Value, VmError> {
    execute_with_registers_context(ops, Vec::new(), context)
}

pub fn execute_with_registers_context(
    ops: &[Op],
    mut registers: Vec<Value>,
    context: &VmContext,
) -> Result<Value, VmError> {
    execute_in_place_context(ops, &mut registers, context)
}

pub fn execute_in_place_context(
    ops: &[Op],
    registers: &mut Vec<Value>,
    context: &VmContext,
) -> Result<Value, VmError> {
    let _context_guard = ContextGuard::install(context);
    for op in ops {
        match run_op(registers, op, context)? {
            None => {}
            Some(value) => return Ok(value),
        }
    }
    Err(VmError::MissingReturn)
}

pub fn execute_builtin_with_receiver(
    builtin: Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    if builtin == Builtin::Print {
        return execute_print(arguments);
    }
    if matches!(
        builtin,
        Builtin::ObjectHasOwnProperty | Builtin::ObjectGetOwnPropertyDescriptor
    ) {
        return crate::builtins::object::execute_special(builtin, receiver, arguments);
    }
    if let Some(result) = early_dispatch(builtin, receiver, arguments) {
        return result;
    }
    if is_data_view_builtin(builtin) {
        return execute_data_view_builtin(builtin, receiver, arguments);
    }
    match builtin {
        _ if is_function_builtin(builtin) => {
            crate::functions::function_builtin(builtin, receiver, arguments)
        }
        _ if is_simple_builtin(builtin) => execute_simple_builtin(builtin, arguments, receiver),
        _ => vm_ops::execute_builtin_tail(builtin, arguments, receiver),
    }
}

fn execute_print(arguments: &[Value]) -> Result<Value, VmError> {
    let text = arguments
        .iter()
        .map(|value| to_string(Some(value)))
        .collect::<Vec<_>>()
        .join(" ");
    let context = CURRENT_CONTEXT.with(|current| current.borrow().clone());
    if let Some(context) = context {
        context.emit_output(&text);
    }
    Ok(Value::Undefined)
}

fn early_dispatch(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    crate::intl::tolocale::symbol::dispatch(builtin, arguments, receiver)
        .or_else(|| crate::arrays::execute_builtin(builtin, receiver, arguments))
        .or_else(|| crate::intl::tolocale::dispatch(builtin, receiver, arguments))
        .or_else(|| crate::collections::execute_builtin(builtin, receiver, arguments))
        .or_else(|| crate::promise::execute_builtin(builtin, receiver, arguments))
        .or_else(|| crate::date::execute(builtin, receiver, arguments))
}

fn is_function_builtin(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::FunctionCall | Builtin::FunctionBind | Builtin::ArrayJoin
    )
}

fn is_simple_builtin(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::Boolean
            | Builtin::Eval
            | Builtin::ReflectConstruct
            | Builtin::Escape
            | Builtin::IsFinite
            | Builtin::IsNaN
            | Builtin::Number
            | Builtin::NumberToString
            | Builtin::NumberValueOf
            | Builtin::ObjectPrototypeToString
            | Builtin::ObjectPrototypeValueOf
            | Builtin::FunctionPrototypeToString
            | Builtin::FunctionPrototypeValueOf
            | Builtin::NumberToFixed
            | Builtin::NumberToPrecision
            | Builtin::NumberToExponential
            | Builtin::ArrayBufferIsView
            | Builtin::Object
            | Builtin::Date
            | Builtin::Error
            | Builtin::RangeError
            | Builtin::ReferenceError
            | Builtin::SyntaxError
            | Builtin::EvalError
            | Builtin::URIError
            | Builtin::AggregateError
            | Builtin::TypeError
    )
}

fn execute_simple_builtin(
    builtin: Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    match builtin {
        Builtin::Boolean => Ok(Value::Boolean(arguments.first().is_some_and(is_truthy))),
        Builtin::Eval | Builtin::ReflectConstruct => crate::reflect::builtin(builtin, arguments),
        Builtin::Escape => Ok(crate::builtins::escape(arguments.first())),
        Builtin::IsFinite => Ok(Value::Boolean(is_finite(arguments.first()))),
        Builtin::IsNaN => Ok(Value::Boolean(to_number(arguments.first()).is_nan())),
        Builtin::Number => Ok(Value::Number(
            crate::intl::tolocale::value::to_number_result(arguments.first())?,
        )),
        Builtin::ArrayBufferIsView => Ok(Value::Boolean(matches!(
            arguments.first(),
            Some(
                Value::Float64Array(_)
                    | Value::Float32Array(_)
                    | Value::Int8Array(_)
                    | Value::Int16Array(_)
                    | Value::Uint16Array(_)
                    | Value::Int32Array(_)
                    | Value::Uint8Array(_)
                    | Value::Uint32Array(_)
                    | Value::Uint8ClampedArray(_)
                    | Value::DataView(_),
            )
        ))),
        Builtin::NumberToString => Ok(Value::String(to_string(arguments.first()))),
        Builtin::NumberValueOf => Ok(Value::Number(to_number(arguments.first()))),
        Builtin::ObjectPrototypeToString => Ok(crate::builtins::prototype_to_string(receiver)),
        Builtin::ObjectPrototypeValueOf => Ok(crate::builtins::prototype_value_of(receiver)),
        Builtin::FunctionPrototypeToString | Builtin::FunctionPrototypeValueOf => {
            function_prototype_builtin(builtin, receiver)
        }
        Builtin::NumberToFixed | Builtin::NumberToPrecision | Builtin::NumberToExponential => {
            crate::number_fmt::number_format(arguments.first(), arguments.get(1), builtin)
        }
        Builtin::Object => Ok(crate::builtins::object(arguments)),
        Builtin::Error
        | Builtin::RangeError
        | Builtin::ReferenceError
        | Builtin::SyntaxError
        | Builtin::EvalError
        | Builtin::URIError
        | Builtin::AggregateError
        | Builtin::TypeError => Ok(crate::builtins::error(builtin, arguments)),
        Builtin::Date => {
            crate::date::execute(builtin, receiver, arguments).unwrap_or(Ok(Value::Undefined))
        }
        _ => Ok(Value::Undefined),
    }
}

fn is_data_view_builtin(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::DataViewGetInt8
            | Builtin::DataViewGetUint8
            | Builtin::DataViewGetInt16
            | Builtin::DataViewGetUint16
            | Builtin::DataViewGetInt32
            | Builtin::DataViewGetUint32
            | Builtin::DataViewGetFloat16
            | Builtin::DataViewGetFloat32
            | Builtin::DataViewGetFloat64
            | Builtin::DataViewSetInt8
            | Builtin::DataViewSetUint8
            | Builtin::DataViewSetInt16
            | Builtin::DataViewSetUint16
            | Builtin::DataViewSetInt32
            | Builtin::DataViewSetUint32
            | Builtin::DataViewSetFloat16
            | Builtin::DataViewSetFloat32
            | Builtin::DataViewSetFloat64
    )
}

fn execute_data_view_builtin(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let view = data_view_receiver(receiver)?;
    let offset = data_view_offset(arguments.first())?;
    let endian_argument = if is_data_view_setter(builtin) { 2 } else { 1 };
    let little_endian = arguments.get(endian_argument).is_some_and(is_truthy);
    if !is_data_view_setter(builtin) {
        return execute_data_view_get(builtin, view, offset, little_endian);
    }
    execute_data_view_set(builtin, view, offset, little_endian, arguments)
}

fn data_view_receiver(receiver: Option<&Value>) -> Result<&crate::value::DataViewData, VmError> {
    match receiver {
        Some(Value::DataView(view)) => Ok(view),
        _ => Err(type_error(
            "DataView method called on incompatible receiver",
        )),
    }
}

fn execute_data_view_get(
    builtin: Builtin,
    view: &crate::value::DataViewData,
    offset: usize,
    little_endian: bool,
) -> Result<Value, VmError> {
    let result = match builtin {
        Builtin::DataViewGetInt8 => {
            Value::Number(view.get_int8(offset).map_err(data_view_error)? as f64)
        }
        Builtin::DataViewGetUint8 => {
            Value::Number(view.get_uint8(offset).map_err(data_view_error)? as f64)
        }
        _ => return execute_data_view_wide_get(builtin, view, offset, little_endian),
    };
    Ok(result)
}

fn execute_data_view_wide_get(
    builtin: Builtin,
    view: &crate::value::DataViewData,
    offset: usize,
    little_endian: bool,
) -> Result<Value, VmError> {
    let value = match builtin {
        Builtin::DataViewGetInt16 => view.get_int16(offset, little_endian).map(|v| v as f64),
        Builtin::DataViewGetUint16 => view.get_uint16(offset, little_endian).map(|v| v as f64),
        Builtin::DataViewGetInt32 => view.get_int32(offset, little_endian).map(|v| v as f64),
        Builtin::DataViewGetUint32 => view.get_uint32(offset, little_endian).map(|v| v as f64),
        Builtin::DataViewGetFloat16 => view.get_float16(offset, little_endian),
        Builtin::DataViewGetFloat32 => view.get_float32(offset, little_endian).map(|v| v as f64),
        Builtin::DataViewGetFloat64 => view.get_float64(offset, little_endian),
        _ => return Err(VmError::NotCallable),
    };
    value.map(Value::Number).map_err(data_view_error)
}

fn is_data_view_setter(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::DataViewSetInt8
            | Builtin::DataViewSetUint8
            | Builtin::DataViewSetInt16
            | Builtin::DataViewSetUint16
            | Builtin::DataViewSetInt32
            | Builtin::DataViewSetUint32
            | Builtin::DataViewSetFloat16
            | Builtin::DataViewSetFloat32
            | Builtin::DataViewSetFloat64
    )
}

fn execute_data_view_set(
    builtin: Builtin,
    view: &crate::value::DataViewData,
    offset: usize,
    little_endian: bool,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let number = crate::intl::tolocale::value::to_number_result(arguments.get(1))?;
    let result = match builtin {
        Builtin::DataViewSetInt8 => view.set_int8(offset, to_i8(number)),
        Builtin::DataViewSetUint8 => view.set_uint8(offset, to_u8(number)),
        Builtin::DataViewSetInt16 => view.set_int16(offset, to_i16(number), little_endian),
        Builtin::DataViewSetUint16 => view.set_uint16(offset, to_u16(number), little_endian),
        Builtin::DataViewSetInt32 => view.set_int32(offset, to_i32(number), little_endian),
        Builtin::DataViewSetUint32 => view.set_uint32(offset, to_u32(number), little_endian),
        Builtin::DataViewSetFloat16 => view.set_float16(offset, number, little_endian),
        Builtin::DataViewSetFloat32 => view.set_float32(offset, number as f32, little_endian),
        Builtin::DataViewSetFloat64 => view.set_float64(offset, number, little_endian),
        _ => return Err(VmError::NotCallable),
    };
    result.map_err(data_view_error).map(|()| Value::Undefined)
}

fn data_view_offset(value: Option<&Value>) -> Result<usize, VmError> {
    let number = crate::intl::tolocale::value::to_number_result(value)?;
    if !number.is_finite() || number < 0.0 {
        return Err(range_error("Offset is outside the bounds of the DataView"));
    }
    Ok(number.trunc() as usize)
}

fn data_view_error(error: crate::value::DataViewError) -> VmError {
    let message = match error {
        crate::value::DataViewError::Detached => "Detached DataView",
        crate::value::DataViewError::OutOfBounds => "Offset is outside the bounds of the DataView",
    };
    range_error(message)
}

fn type_error(message: &str) -> VmError {
    let arguments = [Value::String(message.to_string())];
    VmError::Thrown(crate::builtins::error(Builtin::TypeError, &arguments))
}

fn range_error(message: &str) -> VmError {
    let arguments = [Value::String(message.to_string())];
    VmError::Thrown(crate::builtins::error(Builtin::RangeError, &arguments))
}

fn integer_modulo(value: f64, modulus: f64) -> u64 {
    if !value.is_finite() || value == 0.0 {
        return 0;
    }
    value.trunc().rem_euclid(modulus) as u64
}

fn to_u8(value: f64) -> u8 {
    integer_modulo(value, 256.0) as u8
}
fn to_i8(value: f64) -> i8 {
    let value = to_u8(value);
    if value >= 128 {
        (value as i16 - 256) as i8
    } else {
        value as i8
    }
}
fn to_u16(value: f64) -> u16 {
    integer_modulo(value, 65536.0) as u16
}
fn to_i16(value: f64) -> i16 {
    let value = to_u16(value);
    if value >= 32768 {
        (value as i32 - 65536) as i16
    } else {
        value as i16
    }
}
fn to_u32(value: f64) -> u32 {
    integer_modulo(value, 4294967296.0) as u32
}
fn to_i32(value: f64) -> i32 {
    let value = to_u32(value);
    if value >= 2147483648 {
        (value as i64 - 4294967296) as i32
    } else {
        value as i32
    }
}

fn function_prototype_builtin(
    builtin: Builtin,
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    match builtin {
        Builtin::FunctionPrototypeToString => {
            Ok(crate::builtins::function_prototype_to_string(receiver))
        }
        Builtin::FunctionPrototypeValueOf => {
            Ok(crate::builtins::function_prototype_value_of(receiver))
        }
        _ => Ok(Value::Undefined),
    }
}

pub fn copy_register(registers: &mut Vec<Value>, dst: u16, src: u16) -> Result<(), VmError> {
    let value = read_register(registers, src)?;
    write_value(registers, dst, value);
    Ok(())
}

pub fn write_value(registers: &mut Vec<Value>, index: u16, value: Value) {
    let index = usize::from(index);
    if registers.len() <= index {
        registers.resize(index + 1, Value::Undefined);
    }
    registers[index] = value;
}

pub fn read_register(registers: &[Value], index: u16) -> Result<Value, VmError> {
    registers
        .get(usize::from(index))
        .cloned()
        .ok_or(VmError::RegisterOutOfBounds(index))
}

pub fn get_property(value: &Value, key: &str) -> Value {
    use Value::*;
    match value {
        Builtin(builtin) => builtin_property(*builtin, key),
        Array(values) => crate::arrays::property(values, key),
        ArrayBuffer(buffer) => array_buffer_property(buffer, key),
        Float64Array(view) => float64_array_property(view, key),
        Float32Array(view) => float32_array_property(view, key),
        Int8Array(view) => int8_array_property(view, key),
        Int16Array(view) => int16_array_property(view, key),
        Uint16Array(view) => uint16_array_property(view, key),
        Int32Array(view) => int32_array_property(view, key),
        Uint8Array(view) => uint8_array_property(view, key),
        Uint32Array(view) => uint32_array_property(view, key),
        Uint8ClampedArray(view) => uint8_clamped_array_property(view, key),
        DataView(view) => data_view_property(view, key),
        Object(properties) => object_property(properties, key),
        String(value) => string_property(value, key),
        Number(value) => number_property(*value, key),
        Boolean(value) => boolean_property(*value, key),
        Function(function) if key == "length" => Value::Number(f64::from(function.params)),
        Function(_) if key == "call" => Value::Builtin(crate::ops::Builtin::FunctionCall),
        Function(_) if key == "bind" => Value::Builtin(crate::ops::Builtin::FunctionBind),
        Function(function) => function
            .properties
            .borrow()
            .iter()
            .rev()
            .find(|(name, _)| name == key)
            .map_or(Value::Undefined, |(_, value)| value.clone()),
        Map(_) => crate::collections::map::property(key),
        Set(_) => crate::collections::set::property(key),
        Promise(_) => promise_property(value, key),
        _ => Value::Undefined,
    }
}

fn promise_property(value: &Value, key: &str) -> Value {
    let Some(builtin @ (Builtin::PromiseThen | Builtin::PromiseCatch | Builtin::PromiseFinally)) =
        (match crate::builtins::property(Builtin::PromisePrototype, key) {
            Value::Builtin(builtin) => Some(builtin),
            _ => None,
        })
    else {
        return crate::builtins::property(Builtin::PromisePrototype, key);
    };
    Value::BoundFunction(Rc::new(crate::value::BoundFunctionValue {
        target: Value::Builtin(builtin),
        receiver: value.clone(),
        arguments: Vec::new(),
    }))
}

fn array_buffer_property(buffer: &crate::value::ArrayBufferData, key: &str) -> Value {
    match key {
        "byteLength" => Value::Number(buffer.byte_length() as f64),
        _ => crate::builtins::property(Builtin::ArrayBuffer, key),
    }
}

fn float64_array_property(view: &crate::value::Float64ArrayData, key: &str) -> Value {
    let detached = view.buffer.byte_length() == 0 && view.length != 0;
    match key {
        "buffer" => Value::ArrayBuffer(view.buffer.clone()),
        "byteLength" => Value::Number(if detached { 0 } else { view.byte_length() } as f64),
        "byteOffset" => Value::Number(if detached { 0 } else { view.byte_offset } as f64),
        "length" => Value::Number(if detached { 0 } else { view.length } as f64),
        "BYTES_PER_ELEMENT" => {
            Value::Number(crate::value::Float64ArrayData::BYTES_PER_ELEMENT as f64)
        }
        _ => crate::builtins::property(Builtin::Float64ArrayPrototype, key),
    }
}

fn float32_array_property(view: &crate::value::Float32ArrayData, key: &str) -> Value {
    let detached = view.buffer.byte_length() == 0 && view.length != 0;
    match key {
        "buffer" => Value::ArrayBuffer(view.buffer.clone()),
        "byteLength" => Value::Number(if detached { 0 } else { view.byte_length() } as f64),
        "byteOffset" => Value::Number(if detached { 0 } else { view.byte_offset } as f64),
        "length" => Value::Number(if detached { 0 } else { view.length } as f64),
        "BYTES_PER_ELEMENT" => {
            Value::Number(crate::value::Float32ArrayData::BYTES_PER_ELEMENT as f64)
        }
        _ => crate::builtins::property(Builtin::Float32ArrayPrototype, key),
    }
}

fn int8_array_property(view: &crate::value::Int8ArrayData, key: &str) -> Value {
    let detached = view.buffer.byte_length() == 0 && view.length != 0;
    match key {
        "buffer" => Value::ArrayBuffer(view.buffer.clone()),
        "byteLength" => Value::Number(if detached { 0 } else { view.byte_length() } as f64),
        "byteOffset" => Value::Number(if detached { 0 } else { view.byte_offset } as f64),
        "length" => Value::Number(if detached { 0 } else { view.length } as f64),
        "BYTES_PER_ELEMENT" => Value::Number(crate::value::Int8ArrayData::BYTES_PER_ELEMENT as f64),
        _ => crate::builtins::property(Builtin::Int8ArrayPrototype, key),
    }
}

fn int16_array_property(view: &crate::value::Int16ArrayData, key: &str) -> Value {
    let detached = view.buffer.byte_length() == 0 && view.length != 0;
    match key {
        "buffer" => Value::ArrayBuffer(view.buffer.clone()),
        "byteLength" => Value::Number(if detached { 0 } else { view.byte_length() } as f64),
        "byteOffset" => Value::Number(if detached { 0 } else { view.byte_offset } as f64),
        "length" => Value::Number(if detached { 0 } else { view.length } as f64),
        "BYTES_PER_ELEMENT" => {
            Value::Number(crate::value::Int16ArrayData::BYTES_PER_ELEMENT as f64)
        }
        _ => crate::builtins::property(Builtin::Int16ArrayPrototype, key),
    }
}

fn int32_array_property(view: &crate::value::Int32ArrayData, key: &str) -> Value {
    let detached = view.buffer.byte_length() == 0 && view.length != 0;
    match key {
        "buffer" => Value::ArrayBuffer(view.buffer.clone()),
        "byteLength" => Value::Number(if detached { 0 } else { view.byte_length() } as f64),
        "byteOffset" => Value::Number(if detached { 0 } else { view.byte_offset } as f64),
        "length" => Value::Number(if detached { 0 } else { view.length } as f64),
        "BYTES_PER_ELEMENT" => {
            Value::Number(crate::value::Int32ArrayData::BYTES_PER_ELEMENT as f64)
        }
        _ => crate::builtins::property(Builtin::Int32ArrayPrototype, key),
    }
}

fn uint16_array_property(view: &crate::value::Uint16ArrayData, key: &str) -> Value {
    let detached = view.buffer.byte_length() == 0 && view.length != 0;
    match key {
        "buffer" => Value::ArrayBuffer(view.buffer.clone()),
        "byteLength" => Value::Number(if detached { 0 } else { view.byte_length() } as f64),
        "byteOffset" => Value::Number(if detached { 0 } else { view.byte_offset } as f64),
        "length" => Value::Number(if detached { 0 } else { view.length } as f64),
        "BYTES_PER_ELEMENT" => {
            Value::Number(crate::value::Uint16ArrayData::BYTES_PER_ELEMENT as f64)
        }
        _ => crate::builtins::property(Builtin::Uint16ArrayPrototype, key),
    }
}

fn uint8_array_property(view: &crate::value::Uint8ArrayData, key: &str) -> Value {
    let detached = view.buffer.byte_length() == 0 && view.length != 0;
    match key {
        "buffer" => Value::ArrayBuffer(view.buffer.clone()),
        "byteLength" => Value::Number(if detached { 0 } else { view.byte_length() } as f64),
        "byteOffset" => Value::Number(if detached { 0 } else { view.byte_offset } as f64),
        "length" => Value::Number(if detached { 0 } else { view.length } as f64),
        "BYTES_PER_ELEMENT" => {
            Value::Number(crate::value::Uint8ArrayData::BYTES_PER_ELEMENT as f64)
        }
        _ => crate::builtins::property(Builtin::Uint8ArrayPrototype, key),
    }
}

fn uint32_array_property(view: &crate::value::Uint32ArrayData, key: &str) -> Value {
    let detached = view.buffer.byte_length() == 0 && view.length != 0;
    match key {
        "buffer" => Value::ArrayBuffer(view.buffer.clone()),
        "byteLength" => Value::Number(if detached { 0 } else { view.byte_length() } as f64),
        "byteOffset" => Value::Number(if detached { 0 } else { view.byte_offset } as f64),
        "length" => Value::Number(if detached { 0 } else { view.length } as f64),
        "BYTES_PER_ELEMENT" => {
            Value::Number(crate::value::Uint32ArrayData::BYTES_PER_ELEMENT as f64)
        }
        _ => crate::builtins::property(Builtin::Uint32ArrayPrototype, key),
    }
}

fn uint8_clamped_array_property(view: &crate::value::Uint8ClampedArrayData, key: &str) -> Value {
    let detached = view.buffer.byte_length() == 0 && view.length != 0;
    match key {
        "buffer" => Value::ArrayBuffer(view.buffer.clone()),
        "byteLength" => Value::Number(if detached { 0 } else { view.byte_length() } as f64),
        "byteOffset" => Value::Number(if detached { 0 } else { view.byte_offset } as f64),
        "length" => Value::Number(if detached { 0 } else { view.length } as f64),
        "BYTES_PER_ELEMENT" => {
            Value::Number(crate::value::Uint8ClampedArrayData::BYTES_PER_ELEMENT as f64)
        }
        _ => crate::builtins::property(Builtin::Uint8ClampedArrayPrototype, key),
    }
}

fn data_view_property(view: &crate::value::DataViewData, key: &str) -> Value {
    match key {
        "buffer" => Value::ArrayBuffer(view.buffer.clone()),
        "byteLength" => Value::Number(view.byte_length() as f64),
        "byteOffset" => Value::Number(view.byte_offset as f64),
        _ => crate::builtins::property(Builtin::DataViewPrototype, key),
    }
}

fn object_property(properties: &Rc<Vec<(String, Value)>>, key: &str) -> Value {
    if let Some((_, value)) = properties.iter().rev().find(|(name, _)| name == key) {
        return value.clone();
    }
    let prototype = if properties.iter().any(|(name, _)| name == "timeValue") {
        crate::ops::Builtin::DatePrototype
    } else {
        crate::ops::Builtin::ObjectPrototype
    };
    crate::builtins::property(prototype, key)
}

fn run_op(
    registers: &mut Vec<Value>,
    op: &Op,
    _context: &VmContext,
) -> Result<Option<Value>, VmError> {
    if let Some(result) = run_simple_op(registers, op)? {
        return Ok(result);
    }
    if let Some(result) = run_control_op(registers, op)? {
        return Ok(result);
    }
    run_dispatch_op(registers, op)
}

fn run_simple_op(registers: &mut Vec<Value>, op: &Op) -> Result<Option<Option<Value>>, VmError> {
    use Op::*;
    match op {
        Const { dst, value } => write_value(registers, *dst, value.into()),
        StoreLocal { slot, src } => crate::locals::store(registers, *slot, *src)?,
        LoadLocal { dst, slot } => copy_register(registers, *dst, *slot)?,
        MakeArray { .. } => run_make_array(registers, op)?,
        MakeObject { .. } => run_make_object(registers, op)?,
        MakeBuiltin { dst, builtin } => write_value(registers, *dst, Value::Builtin(*builtin)),
        GetProperty { .. }
        | GetPropertyDynamic { .. }
        | SetProperty { .. }
        | SetPropertyDynamic { .. } => run_get_set_property(registers, op)?,
        DeleteProperty { .. } => run_delete_property(registers, op)?,
        MakeFunction { .. } | MakeFunctionWithKind { .. } => {
            crate::functions::write_op(registers, op)
        }
        Call { .. } => run_call(registers, op)?,
        Await { .. } => run_await(registers, op)?,
        Unary { dst, operator, src } => {
            vm_arithmetic::execute_unary(registers, *dst, *operator, *src)?
        }
        Binary { .. } => run_binary(registers, op)?,
        _ => return Ok(None),
    }
    Ok(Some(None))
}

fn run_control_op(registers: &mut Vec<Value>, op: &Op) -> Result<Option<Option<Value>>, VmError> {
    use Op::*;
    match op {
        ForIn { .. } => run_for_in(registers, op).map(Some),
        ForOf { .. } => crate::loops::execute_for_of(registers, op).map(Some),
        Branch { .. } => run_branch(registers, op).map(Some),
        Try { .. } => run_try(registers, op).map(Some),
        Loop { .. } | Switch { .. } | Conditional { .. } => {
            run_loop_or_special(registers, op).map(Some)
        }
        Return { .. } | Throw { .. } => run_terminal(registers, op).map(Some).map(Some),
        Break { label } => Err(VmError::Break(label.clone())),
        Continue { label } => Err(VmError::Continue(label.clone())),
        _ => Ok(None),
    }
}

fn run_dispatch_op(registers: &mut Vec<Value>, op: &Op) -> Result<Option<Value>, VmError> {
    use Op::*;
    match op {
        CallMethod { .. } | Construct { .. } => run_method_or_construct(registers, op)?,
        _ => {}
    }
    Ok(None)
}

fn run_make_array(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    if let Op::MakeArray { dst, elements } = op {
        execute_array(registers, *dst, elements)?;
    }
    Ok(())
}

fn run_make_object(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    if let Op::MakeObject { dst, properties } = op {
        execute_object(registers, *dst, properties)?;
    }
    Ok(())
}

fn run_call(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    if let Op::Call {
        dst,
        callee,
        args,
        spreads,
    } = op
    {
        vm_ops::execute_call(registers, *dst, *callee, args, spreads)?;
    }
    Ok(())
}

fn run_await(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    if let Op::Await { dst, src } = op {
        vm_ops::execute_await(registers, *dst, *src)?;
    }
    Ok(())
}

fn run_binary(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    if let Op::Binary {
        dst,
        operator,
        lhs,
        rhs,
    } = op
    {
        vm_arithmetic::execute_binary(registers, *dst, *operator, *lhs, *rhs)?;
    }
    Ok(())
}

fn run_get_set_property(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    use Op::*;
    match op {
        GetProperty { .. } => crate::properties::execute_get(registers, op)?,
        GetPropertyDynamic { .. } => crate::properties::execute_get_dynamic(registers, op)?,
        SetProperty { .. } | SetPropertyDynamic { .. } => {
            crate::properties::execute_set_property(registers, op)?
        }
        _ => {}
    }
    Ok(())
}

fn run_delete_property(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    crate::properties::execute_delete_property(registers, op)
}

fn run_for_in(registers: &mut Vec<Value>, op: &Op) -> Result<Option<Value>, VmError> {
    crate::loops::execute_for_in(registers, op)
}

fn run_method_or_construct(registers: &mut Vec<Value>, op: &Op) -> Result<(), VmError> {
    use Op::*;
    match op {
        CallMethod { .. } => crate::methods::execute(registers, op)?,
        Construct { .. } => crate::construct::execute(registers, op)?,
        _ => {}
    }
    Ok(())
}

fn run_loop_or_special(registers: &mut Vec<Value>, op: &Op) -> Result<Option<Value>, VmError> {
    use Op::*;
    match op {
        Loop { .. } => crate::loops::execute(registers, op),
        Switch { .. } => crate::switch::execute(registers, op),
        Conditional { .. } => {
            crate::conditional::execute(registers, op)?;
            Ok(None)
        }
        _ => Ok(None),
    }
}

fn run_terminal(registers: &[Value], op: &Op) -> Result<Value, VmError> {
    execute_terminal(op, registers)
}

fn run_branch(registers: &mut Vec<Value>, op: &Op) -> Result<Option<Value>, VmError> {
    crate::branch::execute_or_continue(registers, op)
}

fn run_try(registers: &mut Vec<Value>, op: &Op) -> Result<Option<Value>, VmError> {
    crate::exceptions::execute(registers, op)
}

fn execute_terminal(op: &Op, registers: &[Value]) -> Result<Value, VmError> {
    match op {
        Op::Return { src } => read_register(registers, *src),
        Op::Throw { src } => Err(VmError::Thrown(read_register(registers, *src)?)),
        _ => Err(VmError::MissingReturn),
    }
}

fn render_thrown(value: &Value) -> String {
    if let Value::Object(properties) = value {
        let name = property_string(properties, "name");
        let message = property_string(properties, "message");
        match (name, message) {
            (Some(name), Some(message)) => format!("{name}: {message}"),
            (Some(name), None) => name,
            (None, Some(message)) => message,
            (None, None) => "[object Object]".to_string(),
        }
    } else {
        to_string(Some(value))
    }
}

fn property_string(properties: &[(String, Value)], key: &str) -> Option<String> {
    properties
        .iter()
        .rev()
        .find(|(name, _)| name == key)
        .map(|(_, value)| to_string(Some(value)))
}

fn execute_array(registers: &mut Vec<Value>, dst: u16, elements: &[u16]) -> Result<(), VmError> {
    let values = elements
        .iter()
        .map(|index| read_register(registers, *index))
        .collect::<Result<Vec<_>, _>>()?;
    write_value(registers, dst, Value::Array(Rc::new(values)));
    Ok(())
}

fn execute_object(
    registers: &mut Vec<Value>,
    dst: u16,
    properties: &[(String, u16)],
) -> Result<(), VmError> {
    let values = properties
        .iter()
        .map(|(key, index)| Ok((key.clone(), read_register(registers, *index)?)))
        .collect::<Result<Vec<_>, VmError>>()?;
    write_value(registers, dst, Value::Object(Rc::new(values)));
    Ok(())
}

fn builtin_property(builtin: crate::ops::Builtin, key: &str) -> Value {
    if builtin == Builtin::Object && key == "hasOwn" {
        return Value::Builtin(Builtin::ObjectHasOwnProperty);
    }
    if matches!(
        builtin,
        Builtin::Float64Array | Builtin::Float64ArrayPrototype
    ) && key == "BYTES_PER_ELEMENT"
    {
        return Value::Number(crate::value::Float64ArrayData::BYTES_PER_ELEMENT as f64);
    }
    if matches!(
        builtin,
        Builtin::Float32Array | Builtin::Float32ArrayPrototype
    ) && key == "BYTES_PER_ELEMENT"
    {
        return Value::Number(crate::value::Float32ArrayData::BYTES_PER_ELEMENT as f64);
    }
    if matches!(builtin, Builtin::Int8Array | Builtin::Int8ArrayPrototype)
        && key == "BYTES_PER_ELEMENT"
    {
        return Value::Number(crate::value::Int8ArrayData::BYTES_PER_ELEMENT as f64);
    }
    if matches!(builtin, Builtin::Int16Array | Builtin::Int16ArrayPrototype)
        && key == "BYTES_PER_ELEMENT"
    {
        return Value::Number(crate::value::Int16ArrayData::BYTES_PER_ELEMENT as f64);
    }
    if matches!(
        builtin,
        Builtin::Uint16Array | Builtin::Uint16ArrayPrototype
    ) && key == "BYTES_PER_ELEMENT"
    {
        return Value::Number(crate::value::Uint16ArrayData::BYTES_PER_ELEMENT as f64);
    }
    if matches!(builtin, Builtin::Int32Array | Builtin::Int32ArrayPrototype)
        && key == "BYTES_PER_ELEMENT"
    {
        return Value::Number(crate::value::Int32ArrayData::BYTES_PER_ELEMENT as f64);
    }
    if matches!(builtin, Builtin::Uint8Array | Builtin::Uint8ArrayPrototype)
        && key == "BYTES_PER_ELEMENT"
    {
        return Value::Number(crate::value::Uint8ArrayData::BYTES_PER_ELEMENT as f64);
    }
    if matches!(
        builtin,
        Builtin::Uint32Array | Builtin::Uint32ArrayPrototype
    ) && key == "BYTES_PER_ELEMENT"
    {
        return Value::Number(crate::value::Uint32ArrayData::BYTES_PER_ELEMENT as f64);
    }
    if matches!(
        builtin,
        Builtin::Uint8ClampedArray | Builtin::Uint8ClampedArrayPrototype
    ) && key == "BYTES_PER_ELEMENT"
    {
        return Value::Number(crate::value::Uint8ClampedArrayData::BYTES_PER_ELEMENT as f64);
    }
    let value = crate::builtins::property(builtin, key);
    if let Value::Builtin(symbol) = value {
        if let Some(name) = crate::intl::tolocale::symbol::name(symbol) {
            return Value::String(name.to_string());
        }
    }
    value
}

fn string_property(value: &str, key: &str) -> Value {
    use crate::ops::Builtin::*;
    match key {
        "length" => return Value::Number(value.chars().count() as f64),
        "toLocaleLowerCase" => return Value::Builtin(StringToLocaleLowerCase),
        "toLocaleUpperCase" => return Value::Builtin(StringToLocaleUpperCase),
        _ => {}
    }
    if let Some(method) = crate::strings::property_method(key) {
        return Value::Builtin(method);
    }
    key.parse::<usize>()
        .ok()
        .and_then(|index| value.chars().nth(index))
        .map(|character| Value::String(character.to_string()))
        .unwrap_or(Value::Undefined)
}

fn number_property(_value: f64, key: &str) -> Value {
    use crate::ops::Builtin::*;
    match key {
        "toLocaleString" => Value::Builtin(NumberToLocaleString),
        "toString" => Value::Builtin(NumberToString),
        "valueOf" => Value::Builtin(NumberValueOf),
        "toFixed" => Value::Builtin(NumberToFixed),
        "toPrecision" => Value::Builtin(NumberToPrecision),
        "toExponential" => Value::Builtin(NumberToExponential),
        _ => Value::Undefined,
    }
}

fn boolean_property(value: bool, key: &str) -> Value {
    match key {
        "toString" => Value::String(value.to_string()),
        "valueOf" => Value::Boolean(value),
        _ => Value::Undefined,
    }
}
