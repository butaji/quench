pub(crate) fn array_to_sorted(receiver: Option<&Value>, _arguments: &[Value]) -> Value {
    let Some(Value::Array(values)) = receiver else {
        return Value::Undefined;
    };
    let mut sorted = values.to_vec();
    sorted.sort_by_key(|value| crate::intl::tolocale::value::to_string(Some(value)));
    Value::array(sorted)
}
