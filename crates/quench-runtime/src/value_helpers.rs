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

impl Value {
    /// The extra-property metadata shared by all typed-array views.
    pub(crate) fn typed_array_meta(&self) -> Option<&TypedArrayMeta> {
        match self {
            Self::Float64Array(view) => Some(&view.meta),
            Self::Float32Array(view) => Some(&view.meta),
            Self::Int8Array(view) => Some(&view.meta),
            Self::Int16Array(view) => Some(&view.meta),
            Self::Int32Array(view) => Some(&view.meta),
            Self::BigInt64Array(view) => Some(&view.meta),
            Self::BigUint64Array(view) => Some(&view.meta),
            Self::Uint32Array(view) => Some(&view.meta),
            Self::Uint8Array(view) => Some(&view.meta),
            Self::Uint8ClampedArray(view) => Some(&view.meta),
            Self::Uint16Array(view) => Some(&view.meta),
            _ => None,
        }
    }

    #[inline]
    pub(crate) fn is_number(&self) -> bool {
        matches!(self, Self::Number(_))
    }

    #[inline]
    pub(crate) fn as_number(&self) -> Option<f64> {
        match self {
            Self::Number(number) => Some(*number),
            _ => None,
        }
    }

    #[inline]
    pub(crate) fn is_boolean(&self) -> bool {
        matches!(self, Self::Boolean(_))
    }

    #[inline]
    pub(crate) fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    #[inline]
    pub(crate) fn is_undefined(&self) -> bool {
        matches!(self, Self::Undefined)
    }

    #[inline]
    pub(crate) fn from_integer(value: i64) -> Self {
        Self::Number(value as f64)
    }
}
