/// Classify the canonical `Value` representation without creating a second
/// primitive/object model. `BindingCell` is the sole indirection: borrow it
/// transiently and recurse, while every other variant is classified from its
/// tag. The borrowed cell remains owned by the value and is never consumed.
#[inline(always)]
#[must_use]
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

#[cfg(test)]
mod tiny_primitive_tests {
    use super::*;

    #[test]
    fn primitive_helpers_classify_without_allocating() {
        let values = [
            Value::Number(1.0),
            Value::Boolean(true),
            Value::String("x".to_string()),
            Value::Null,
            Value::Undefined,
        ];
        assert!(values.iter().all(|value| !is_object(value)));
        assert!(values[0].is_number());
        assert!(values[1].is_boolean());
        assert!(values[3].is_null());
        assert!(values[4].is_undefined());
        assert_eq!(values[0].as_number(), Some(1.0));
        assert_eq!(values[1].as_number(), None);
    }

    #[test]
    fn primitive_accessors_are_exhaustive_and_non_overlapping() {
        let values = [
            Value::Number(-0.0),
            Value::Boolean(false),
            Value::Boolean(true),
            Value::Null,
            Value::Undefined,
            Value::String(String::new()),
            Value::BigInt("1".to_string()),
        ];
        for value in &values {
            let matches = value.is_number() as u8
                + value.is_boolean() as u8
                + value.is_null() as u8
                + value.is_undefined() as u8;
            assert!(matches <= 1);
            assert_eq!(value.as_number().is_some(), value.is_number());
        }
        assert_eq!(values[0].as_number(), Some(-0.0));
        assert!(!values[5].is_number());
        assert!(!values[6].is_undefined());
    }

    #[test]
    fn binding_cell_classification_follows_canonical_value() {
        let primitive =
            Value::BindingCell(std::rc::Rc::new(std::cell::RefCell::new(Value::Undefined)));
        assert!(!is_object(&primitive));
    }

    #[test]
    fn value_representation_budget_is_compile_time_contract() {
        assert!(std::mem::size_of::<Value>() <= crate::value::VALUE_SIZE_BUDGET);
        assert_eq!(crate::value::VALUE_ALIGNMENT_TAG_BITS, 0);
    }
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

    #[inline(always)]
    #[must_use]
    pub(crate) fn is_number(&self) -> bool {
        matches!(self, Self::Number(_))
    }

    #[inline(always)]
    #[must_use]
    pub(crate) fn as_number(&self) -> Option<f64> {
        match self {
            Self::Number(number) => Some(*number),
            _ => None,
        }
    }

    #[inline(always)]
    #[must_use]
    pub(crate) fn is_boolean(&self) -> bool {
        matches!(self, Self::Boolean(_))
    }

    #[inline(always)]
    #[must_use]
    pub(crate) fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    #[inline(always)]
    #[must_use]
    pub(crate) fn is_undefined(&self) -> bool {
        matches!(self, Self::Undefined)
    }

    #[inline(always)]
    pub(crate) fn from_integer(value: i64) -> Self {
        Self::Number(value as f64)
    }
}
