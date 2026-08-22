#[cfg(test)]
mod tests {
    use super::{array_index, index_of};
    use crate::value::{ArrayData, ObjectData, Value};
    use std::rc::Rc;

    #[test]
    fn index_of_does_not_use_structural_object_equality() {
        let left = Value::Object(Rc::new(ObjectData::new(Vec::new())));
        let right = Value::Object(Rc::new(ObjectData::new(Vec::new())));
        let array = Value::Array(Rc::new(ArrayData::new(vec![left.clone()])));
        let result = index_of(Some(&array), &[right]);
        assert_eq!(result, Ok(Value::Number(-1.0)));
    }

    #[test]
    fn array_index_parser_preserves_canonical_indices_without_allocating() {
        assert_eq!(array_index("0"), Some(0));
        assert_eq!(array_index("42"), Some(42));
        assert_eq!(array_index(""), None);
        assert_eq!(array_index("01"), None);
        assert_eq!(array_index("4294967295"), None);
        assert_eq!(array_index("4294967296"), None);
        assert_eq!(array_index("1.0"), None);
    }
}
