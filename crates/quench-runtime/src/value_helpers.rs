pub(crate) fn is_object(value: &Value) -> bool {
    if let Value::BindingCell(cell) = value {
        return is_object(&cell.borrow());
    }
    !matches!(
        value,
        Value::Number(_)
            | Value::Boolean(_)
            | Value::String(_)
            | Value::StringUnits(_)
            | Value::BigInt(_)
            | Value::Null
            | Value::Undefined
            | Value::HostCapability(_)
    )
}

impl BoundFunctionValue {
    pub(crate) fn new(realm: crate::ops::RealmId, target: Value, receiver: Value) -> Self {
        Self {
            realm,
            target,
            receiver,
            arguments: Vec::new(),
            properties: std::cell::RefCell::new(Vec::new()),
        }
    }
}
