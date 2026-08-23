#[cfg(test)]
mod tests {
    use super::index_of;
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
}
