use crate::intl::tolocale::value::{is_finite, to_number, to_string};
use crate::ops::{Builtin, Op};
use crate::value::Value;
use std::rc::Rc;

mod vm_arithmetic;
mod vm_ops;

pub use crate::intl::tolocale::value::is_truthy;

#[derive(Debug, Clone, PartialEq)]
pub enum VmError {
    RegisterOutOfBounds(u16),
    MissingReturn,
    Break(Option<String>),
    Continue(Option<String>),
    NotCallable,
    EvalError(String),
    Thrown(Value),
}

impl VmError {
    pub fn render(&self) -> String {
        match self {
            VmError::Thrown(value) => render_thrown(value),
            other => format!("{other:?}"),
        }
    }
}

pub fn execute(ops: &[Op]) -> Result<Value, VmError> {
    execute_with_registers(ops, Vec::new())
}

pub fn execute_with_registers(ops: &[Op], mut registers: Vec<Value>) -> Result<Value, VmError> {
    execute_in_place(ops, &mut registers)
}

pub fn execute_in_place(ops: &[Op], registers: &mut Vec<Value>) -> Result<Value, VmError> {
    for op in ops {
        match run_op(registers, op)? {
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
    if matches!(
        builtin,
        Builtin::ObjectHasOwnProperty | Builtin::ObjectGetOwnPropertyDescriptor
    ) {
        return crate::builtins::object::execute_special(builtin, receiver, arguments);
    }
    if let Some(result) = early_dispatch(builtin, receiver, arguments) {
        return result;
    }
    match builtin {
        _ if is_function_builtin(builtin) => {
            crate::functions::function_builtin(builtin, receiver, arguments)
        }
        _ if is_simple_builtin(builtin) => execute_simple_builtin(builtin, arguments, receiver),
        _ => vm_ops::execute_builtin_tail(builtin, arguments, receiver),
    }
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

fn run_op(registers: &mut Vec<Value>, op: &Op) -> Result<Option<Value>, VmError> {
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
