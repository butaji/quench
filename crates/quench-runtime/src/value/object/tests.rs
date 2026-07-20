use crate::value::kind::ObjectKind;
use crate::value::object::{Object, Value};
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn test_non_canonical_numeric_key_does_not_alias_elements() {
    let mut obj = Object::new_array(3);
    obj.elements[1] = Value::Number(2.0);
    obj.set("01", Value::Number(9.0));
    assert_eq!(obj.get("1"), Some(Value::Number(2.0)));
    assert_eq!(obj.get("01"), Some(Value::Number(9.0)));
    assert_eq!(obj.elements.len(), 3, "elements must not grow for '01'");
    obj.set("1", Value::Number(5.0));
    assert_eq!(obj.elements[1], Value::Number(5.0));
}

#[test]
fn test_huge_index_does_not_grow_elements() {
    let mut obj = Object::new(ObjectKind::Ordinary);
    obj.set("1000000000", Value::Number(1.0));
    assert!(obj.elements.is_empty());
    assert_eq!(obj.get("1000000000"), Some(Value::Number(1.0)));
}

#[test]
fn test_set_then_get_own_string_value() {
    let mut obj = Object::new(ObjectKind::Ordinary);
    obj.set("foo", Value::String("bar".to_string()));
    let got = obj.get_own("foo");
    assert_eq!(got, Some(Value::String("bar".to_string())));
}

#[test]
fn test_set_then_get_own_native_function() {
    let mut obj = Object::new(ObjectKind::Ordinary);
    let nf = Rc::new(crate::value::NativeFunction::new(|_| Ok(Value::Undefined)));
    obj.set("exec", Value::NativeFunction(nf));
    let got = obj.get_own("exec");
    assert!(got.is_some() && matches!(got, Some(Value::NativeFunction(_))));
}

#[test]
fn test_set_then_get_public_api() {
    let mut obj = Object::new(ObjectKind::Ordinary);
    obj.set("method", Value::String("test".to_string()));
    let got = obj.get("method");
    assert_eq!(got, Some(Value::String("test".to_string())));
}

#[test]
fn test_set_updates_existing_property() {
    let mut obj = Object::new(ObjectKind::Ordinary);
    obj.set("x", Value::Number(1.0));
    obj.set("x", Value::Number(2.0));
    assert_eq!(obj.get("x"), Some(Value::Number(2.0)));
    assert_eq!(obj.get_own("x"), Some(Value::Number(2.0)));
}

#[test]
fn test_prototype_get_finds_set_property() {
    let mut proto = Object::new(ObjectKind::Ordinary);
    proto.set("exec", Value::String("found".to_string()));
    let mut obj = Object::new(ObjectKind::Ordinary);
    obj.prototype = Some(Rc::new(RefCell::new(proto)));
    assert_eq!(obj.get("exec"), Some(Value::String("found".to_string())));
}

#[test]
fn test_set_own_property_shadows_prototype() {
    let mut proto = Object::new(ObjectKind::Ordinary);
    proto.set("x", Value::String("proto".to_string()));
    let mut obj = Object::new(ObjectKind::Ordinary);
    obj.prototype = Some(Rc::new(RefCell::new(proto)));
    obj.set("x", Value::String("own".to_string()));
    assert_eq!(obj.get("x"), Some(Value::String("own".to_string())));
    assert_eq!(obj.get_own("x"), Some(Value::String("own".to_string())));
}

#[test]
fn test_delete_after_set() {
    let mut obj = Object::new(ObjectKind::Ordinary);
    obj.set("foo", Value::Number(42.0));
    assert_eq!(obj.get("foo"), Some(Value::Number(42.0)));
    obj.delete("foo");
    assert_eq!(obj.get("foo"), None);
    assert_eq!(obj.get_own("foo"), None);
}

#[test]
fn test_multiple_properties_independent() {
    let mut obj = Object::new(ObjectKind::Ordinary);
    obj.set("a", Value::Number(1.0));
    obj.set("b", Value::String("two".to_string()));
    let nf = Rc::new(crate::value::NativeFunction::new(|_| Ok(Value::Undefined)));
    obj.set("c", Value::NativeFunction(nf));
    assert_eq!(obj.get("a"), Some(Value::Number(1.0)));
    assert_eq!(obj.get("b"), Some(Value::String("two".to_string())));
    let c = obj.get("c");
    assert!(c.is_some() && matches!(c, Some(Value::NativeFunction(_))));
}
