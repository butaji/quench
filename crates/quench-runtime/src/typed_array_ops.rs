use crate::{execute::VmError, ops::Builtin, value::Value};

macro_rules! fill_view {
    ($view:expr, $value:expr, $convert:expr) => {{
        for index in 0..$view.length {
            $view.set(index, $convert($value));
        }
    }};
}

pub(crate) fn execute(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    (builtin == Builtin::TypedArrayFill).then(|| fill(receiver, arguments))
}

fn fill(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(crate::vm::not_callable)?;
    let number = crate::intl::tolocale::value::to_number(arguments.first());
    match receiver {
        Value::Float64Array(view) => fill_view!(view, number, |value| value),
        Value::Float32Array(view) => fill_view!(view, number, |value| value as f32),
        Value::Int8Array(view) => fill_view!(view, number, crate::construct::to_int8),
        Value::Int16Array(view) => fill_view!(view, number, crate::construct::to_int16),
        Value::Int32Array(view) => fill_view!(view, number, crate::construct::to_int32),
        Value::Uint8Array(view) => fill_view!(view, number, crate::construct::to_uint8),
        Value::Uint16Array(view) => fill_view!(view, number, crate::construct::to_uint16),
        Value::Uint32Array(view) => fill_view!(view, number, crate::construct::to_uint32),
        Value::Uint8ClampedArray(view) => fill_view!(view, number, |value| value),
        _ => return Err(crate::vm::not_callable()),
    }
    Ok(receiver.clone())
}
