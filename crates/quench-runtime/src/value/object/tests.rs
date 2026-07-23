use crate::value::kind::ObjectKind;
use crate::value::object::helpers::as_array_index;
use crate::value::object::{
    define_accessor, has_getter, has_setter, set_getter_func, set_setter_func, Object,
    PropertyDescriptor, PropertyFlags, Value,
};
use crate::value::NativeFunction;
use std::cell::RefCell;
use std::rc::Rc;

// ─── R4 refactor pin: live store survives TComp deletion ────────────────

#[test]
fn array_assign_and_define_own_property_survive() {
    let mut obj = Object::new_array(1);
    obj.set("0", Value::Number(10.0));
    let pd = PropertyDescriptor {
        value: Some(Value::Number(20.0)),
        writable: Some(true),
        enumerable: Some(true),
        configurable: Some(true),
        ..Default::default()
    };
    assert!(obj.define_own_property("1", &pd));
    assert_eq!(obj.get("0"), Some(Value::Number(10.0)));
    assert_eq!(obj.get("1"), Some(Value::Number(20.0)));
    assert_eq!(
        obj.properties.get("length"),
        Some(&Value::Number(obj.elements.len() as f64))
    );
}

// ─── Core existing tests ────────────────────────────────────────────────

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
    assert_eq!(obj.get_own("foo"), Some(Value::String("bar".to_string())));
}

#[test]
fn test_set_then_get_own_native_function() {
    let mut obj = Object::new(ObjectKind::Ordinary);
    let nf = Rc::new(NativeFunction::new(|_| Ok(Value::Undefined)));
    obj.set("exec", Value::NativeFunction(nf));
    let got = obj.get_own("exec");
    assert!(got.is_some() && matches!(got, Some(Value::NativeFunction(_))));
}

#[test]
fn test_set_then_get_public_api() {
    let mut obj = Object::new(ObjectKind::Ordinary);
    obj.set("method", Value::String("test".to_string()));
    assert_eq!(obj.get("method"), Some(Value::String("test".to_string())));
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
    let nf = Rc::new(NativeFunction::new(|_| Ok(Value::Undefined)));
    obj.set("c", Value::NativeFunction(nf));
    assert_eq!(obj.get("a"), Some(Value::Number(1.0)));
    assert_eq!(obj.get("b"), Some(Value::String("two".to_string())));
    let c = obj.get("c");
    assert!(c.is_some() && matches!(c, Some(Value::NativeFunction(_))));
}

// ─── 1. Object::new() / with_prototype ────────────────────────────────

#[test]
fn test_object_new_various_kinds() {
    let obj = Object::new(ObjectKind::Ordinary);
    assert_eq!(obj.kind, ObjectKind::Ordinary);
    assert!(obj.extensible);
    assert!(obj.prototype.is_none());

    let arr = Object::new(ObjectKind::Array);
    assert_eq!(arr.kind, ObjectKind::Array);
    assert!(arr.elements.is_empty());

    for kind in [
        ObjectKind::Function,
        ObjectKind::Promise,
        ObjectKind::Map,
        ObjectKind::Set,
        ObjectKind::Date,
        ObjectKind::RegExp,
    ] {
        let o = Object::new(kind.clone());
        assert_eq!(o.kind, kind);
        assert!(o.extensible);
    }
}

#[test]
fn test_object_with_prototype_inherits() {
    let proto = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
    proto.borrow_mut().set("inherited", Value::Boolean(true));
    let obj = Object::with_prototype(ObjectKind::Ordinary, Rc::clone(&proto));
    assert!(obj.prototype.is_some());
    assert_eq!(obj.get("inherited"), Some(Value::Boolean(true)));
    let arr = Object::with_prototype(ObjectKind::Array, Rc::clone(&proto));
    assert_eq!(arr.kind, ObjectKind::Array);
}

// ─── 2. Set / Get / Define / Delete edge cases ──────────────────────

#[test]
fn test_set_get_edge_cases() {
    let mut obj = Object::new(ObjectKind::Ordinary);
    obj.set("n", Value::Null);
    obj.set("u", Value::Undefined);
    assert_eq!(obj.get("n"), Some(Value::Null));
    assert_eq!(obj.get("u"), Some(Value::Undefined));
    assert_eq!(obj.get("nonexistent"), None);
    obj.set("k", Value::Number(1.0));
    obj.delete("k");
    obj.set("k", Value::Number(2.0));
    assert_eq!(obj.get("k"), Some(Value::Number(2.0)));
    assert_eq!(obj.get_own_value("ownval"), None);
    obj.set("ownval", Value::String("v".to_string()));
    assert_eq!(
        obj.get_own_value("ownval"),
        Some(Value::String("v".to_string()))
    );
}

#[test]
fn test_define_non_writable() {
    let mut obj = Object::new(ObjectKind::Ordinary);
    let flags = PropertyFlags {
        value: None,
        writable: false,
        enumerable: true,
        configurable: false,
    };
    obj.define("readonly", Value::String("fixed".to_string()), flags);
    assert_eq!(
        obj.get("readonly"),
        Some(Value::String("fixed".to_string()))
    );
    obj.set("readonly", Value::String("changed".to_string()));
    assert_eq!(
        obj.get("readonly"),
        Some(Value::String("fixed".to_string()))
    );
    obj.set("x", Value::Number(1.0));
    obj.define("x", Value::Number(99.0), PropertyFlags::default_data());
    assert_eq!(obj.get("x"), Some(Value::Number(99.0)));
}

#[test]
fn test_delete_non_configurable_and_element() {
    let mut obj = Object::new(ObjectKind::Ordinary);
    let flags = PropertyFlags {
        value: None,
        writable: true,
        enumerable: true,
        configurable: false,
    };
    obj.define("locked", Value::Number(42.0), flags);
    assert!(!obj.delete("locked"));
    assert_eq!(obj.get("locked"), Some(Value::Number(42.0)));
    let mut arr = Object::new_array(3);
    arr.elements[0] = Value::Number(10.0);
    arr.holes.remove(&0);
    assert!(arr.delete("0"));
    assert_eq!(arr.get("0"), Some(Value::Undefined));
}

// ─── 3. has / has_own / keys / get_descriptor ───────────────────────

#[test]
fn test_has_and_has_own() {
    let mut obj = Object::new(ObjectKind::Ordinary);
    obj.set("own", Value::Boolean(true));
    assert!(obj.has_own("own") && !obj.has_own("missing"));
    let mut proto = Object::new(ObjectKind::Ordinary);
    proto.set("parent", Value::String("yes".to_string()));
    let mut child = Object::new(ObjectKind::Ordinary);
    child.prototype = Some(Rc::new(RefCell::new(proto)));
    assert!(child.has("parent") && !child.has_own("parent") && !child.has("nonexistent"));
    let func = Value::NativeFunction(Rc::new(NativeFunction::new(|_| Ok(Value::Undefined))));
    set_getter_func(&mut obj, "computed", func);
    assert!(obj.has_own("computed"));
}

#[test]
fn test_own_keys_vs_property_names() {
    let mut obj = Object::new(ObjectKind::Ordinary);
    obj.set("visible", Value::Number(1.0));
    let flags = PropertyFlags {
        value: None,
        writable: true,
        enumerable: false,
        configurable: true,
    };
    obj.define("hidden", Value::Number(2.0), flags);
    assert!(obj.own_keys().contains(&"visible".to_string()));
    assert!(!obj.own_keys().contains(&"hidden".to_string()));
    let names = obj.own_property_names();
    assert!(names.contains(&"visible".to_string()) && names.contains(&"hidden".to_string()));
}

#[test]
fn test_get_descriptor() {
    let mut obj = Object::new(ObjectKind::Ordinary);
    obj.set("key", Value::Number(1.0));
    let d = obj.get_descriptor("key").unwrap();
    assert!(d.writable && d.enumerable && d.configurable);
    assert_eq!(d.value, Some(Value::Number(1.0)));
    let flags = PropertyFlags {
        value: None,
        writable: false,
        enumerable: false,
        configurable: true,
    };
    obj.define("locked", Value::Number(42.0), flags);
    let d2 = obj.get_descriptor("locked").unwrap();
    assert!(!d2.writable && !d2.enumerable);
    assert!(obj.get_descriptor("nothing").is_none());
}

// ─── 4. Prototype chain / boxed value / exotic kinds ────────────────

#[test]
fn test_deep_prototype_chain() {
    let mut l3 = Object::new(ObjectKind::Ordinary);
    l3.set("depth", Value::Number(3.0));
    let mut l2 = Object::new(ObjectKind::Ordinary);
    l2.prototype = Some(Rc::new(RefCell::new(l3)));
    let mut l1 = Object::new(ObjectKind::Ordinary);
    l1.prototype = Some(Rc::new(RefCell::new(l2)));
    assert_eq!(l1.get("depth"), Some(Value::Number(3.0)));
    assert!(!l1.has_own("depth") && l1.has("depth"));
    let proto = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
    proto.borrow_mut().set("a", Value::Number(1.0));
    let child = Object::with_prototype(ObjectKind::Ordinary, proto);
    assert_eq!(child.get("a"), Some(Value::Number(1.0)));
    assert_eq!(child.get("b"), None);
}

#[test]
fn test_boxed_value_pattern() {
    let mut obj = Object::new(ObjectKind::Ordinary);
    let flags = PropertyFlags {
        value: None,
        writable: true,
        enumerable: false,
        configurable: true,
    };
    obj.define("_value", Value::Number(42.0), flags.clone());
    assert_eq!(obj.get_own("_value"), Some(Value::Number(42.0)));
    assert!(!obj.is_enumerable("_value"));
    obj.define("_value", Value::String("prim".to_string()), flags);
    assert_eq!(
        obj.get_own_value("_value"),
        Some(Value::String("prim".to_string()))
    );
}

#[test]
fn test_array_elements_access() {
    let mut obj = Object::new_array(3);
    assert_eq!(obj.kind, ObjectKind::Array);
    assert_eq!(obj.elements.len(), 3);
    obj.set("0", Value::String("zero".to_string()));
    obj.set("1", Value::String("one".to_string()));
    assert_eq!(obj.get("0"), Some(Value::String("zero".to_string())));
    assert_eq!(obj.get("1"), Some(Value::String("one".to_string())));
    assert_eq!(obj.get("2"), Some(Value::Undefined));
    let mut empty = Object::new_array(0);
    empty.set("5", Value::Number(5.0));
    assert!(empty.elements.len() > 5);
    assert_eq!(empty.elements[5], Value::Number(5.0));
}

// ─── 5. Getters / Setters / Accessors ───────────────────────────────

#[test]
fn test_getter_setter_via_func() {
    let mut obj = Object::new(ObjectKind::Ordinary);
    let getter = Value::NativeFunction(Rc::new(NativeFunction::new(|_| Ok(Value::Number(99.0)))));
    let setter = Value::NativeFunction(Rc::new(NativeFunction::new(|_| Ok(Value::Undefined))));
    set_getter_func(&mut obj, "prop", getter);
    set_setter_func(&mut obj, "prop", setter);
    assert!(has_getter(&obj, "prop") && has_setter(&obj, "prop"));
    assert!(obj.get_getter("prop").is_some());
    assert!(obj.get_setter("prop").is_some());
    assert!(obj.get_setter_func("prop").is_some());
    assert!(!has_getter(&obj, "nosuch") && !has_setter(&obj, "nosuch"));
}

#[test]
fn test_define_accessor_stores_funcs() {
    let mut obj = Object::new(ObjectKind::Ordinary);
    let getter = Value::NativeFunction(Rc::new(NativeFunction::new(|_| Ok(Value::Number(10.0)))));
    let setter = Value::NativeFunction(Rc::new(NativeFunction::new(|_| Ok(Value::Undefined))));
    let flags = PropertyFlags {
        value: None,
        writable: false,
        enumerable: true,
        configurable: true,
    };
    define_accessor(&mut obj, "x", Some(getter), Some(setter), flags);
    assert!(has_getter(&obj, "x") && has_setter(&obj, "x"));
    assert!(obj.get_getter("x").unwrap().func.is_some());
    assert!(obj.get_setter("x").unwrap().func.is_some());
    let g2 = Value::NativeFunction(Rc::new(NativeFunction::new(|_| Ok(Value::Boolean(true)))));
    define_accessor(
        &mut obj,
        "ro",
        Some(g2),
        None,
        PropertyFlags::default_accessor(),
    );
    assert!(has_getter(&obj, "ro") && !has_setter(&obj, "ro"));
}

// ─── 6. Array index helpers ─────────────────────────────────────────

#[test]
fn test_as_array_index_canonical() {
    assert_eq!(as_array_index("0"), Some(0));
    assert_eq!(as_array_index("42"), Some(42));
    assert_eq!(as_array_index("01"), None);
    assert_eq!(as_array_index("-1"), None);
    assert_eq!(as_array_index("abc"), None);
}

#[test]
fn test_element_hole_after_delete() {
    let mut obj = Object::new_array(3);
    obj.set("1", Value::Number(100.0));
    obj.delete("1");
    assert!(obj.holes.contains(&1));
    assert_eq!(obj.get("1"), Some(Value::Undefined));
    let mut empty = Object::new_array(0);
    empty.set("0", Value::Number(1.0));
    assert!(empty.properties.get("length").is_some());
}

// ─── 7. get_own_property / define_own_property / is_enumerable ──────

#[test]
fn test_get_and_define_own_property() {
    let mut obj = Object::new(ObjectKind::Ordinary);
    obj.set("key", Value::Number(7.0));
    let desc = obj.get_own_property("key").unwrap();
    assert_eq!(desc.value, Some(Value::Number(7.0)));
    assert!(obj.get_own_property("missing").is_none());
    let pd = PropertyDescriptor {
        value: Some(Value::Number(10.0)),
        writable: Some(false),
        enumerable: Some(true),
        configurable: Some(false),
        ..Default::default()
    };
    assert!(obj.define_own_property("prop", &pd));
    let d = obj.get_descriptor("prop").unwrap();
    assert!(!d.writable && !d.configurable);
    assert_eq!(d.value, Some(Value::Number(10.0)));
}

#[test]
fn test_is_enumerable() {
    let mut obj = Object::new(ObjectKind::Ordinary);
    obj.set("x", Value::Number(1.0));
    assert!(obj.is_enumerable("x"));
    let flags = PropertyFlags {
        value: None,
        writable: true,
        enumerable: false,
        configurable: true,
    };
    obj.define("hidden", Value::Number(0.0), flags);
    assert!(!obj.is_enumerable("hidden"));
    assert!(obj.is_enumerable("nobody"));
}

// ─── 8. new_array_checked / function helpers / symbols / non-extensible

#[test]
fn test_new_array_checked() {
    let obj = Object::new_array_checked(10).unwrap();
    assert_eq!(obj.kind, ObjectKind::Array);
    assert_eq!(obj.elements.len(), 10);
    assert!(Object::new_array_checked(1_000_000_000).is_err());
}

#[test]
fn test_function_property_helpers_non_function() {
    let mut obj = Object::new(ObjectKind::Ordinary);
    obj.set("x", Value::Number(1.0));
    assert!(!obj.set_function_property("x", "prop", Value::Undefined));
    assert!(obj.get_function_mut("x").is_none());
}

#[test]
fn test_symbol_properties() {
    let sym = |d: &str| {
        Value::Symbol(Rc::new(crate::value::Symbol {
            desc: Some(Rc::from(d)),
            global: false,
        }))
    };
    let mut obj = Object::new(ObjectKind::Ordinary);
    obj.set_symbol("test", Value::Number(42.0));
    assert!(obj.has_symbol(&sym("test")));
    assert_eq!(obj.get_property(&sym("test")), Some(Value::Number(42.0)));
    assert!(!obj.has_symbol(&sym("missing")));
    assert_eq!(obj.get_property(&sym("missing")), None);
    obj.set_symbol_value(sym("sym"));
    assert!(obj.has_symbol(&sym("sym")));
}

#[test]
fn test_non_extensible_object() {
    let mut obj = Object::new(ObjectKind::Ordinary);
    obj.set("existing", Value::Number(1.0));
    obj.extensible = false;
    obj.set("new", Value::Number(2.0));
    assert_eq!(obj.get("new"), None);
    obj.set("existing", Value::Number(3.0));
    assert_eq!(obj.get("existing"), Some(Value::Number(3.0)));
}

// ─── 9. Descriptor / PropertyFlags helpers ──────────────────────────

#[test]
fn test_property_descriptor_type_checks() {
    let data = PropertyDescriptor {
        value: Some(Value::Number(1.0)),
        ..Default::default()
    };
    assert!(data.is_data() && !data.is_accessor());
    let acc = PropertyDescriptor {
        get: Some(Value::Null),
        ..Default::default()
    };
    assert!(!acc.is_data() && acc.is_accessor());
}

#[test]
fn test_property_flags_defaults() {
    let data = PropertyFlags::default_data();
    assert!(data.writable && data.enumerable && data.configurable);
    assert_eq!(data.value, None);
    let acc = PropertyFlags::default_accessor();
    assert!(!acc.writable && acc.enumerable && acc.configurable);
}

// ─── Requested test patterns ─────────────────────────────────────────────────

#[test]
fn object_get_missing() {
    let obj = Object::new(ObjectKind::Ordinary);
    assert_eq!(obj.get("nonexistent"), None);
}

#[test]
fn object_get_existing() {
    let mut obj = Object::new(ObjectKind::Ordinary);
    obj.set("key", Value::Number(42.0));
    assert_eq!(obj.get("key"), Some(Value::Number(42.0)));
}

#[test]
fn object_set_creates() {
    let mut obj = Object::new(ObjectKind::Ordinary);
    obj.set("newKey", Value::String("created".to_string()));
    assert_eq!(
        obj.get("newKey"),
        Some(Value::String("created".to_string()))
    );
}

#[test]
fn object_set_updates() {
    let mut obj = Object::new(ObjectKind::Ordinary);
    obj.set("x", Value::Number(1.0));
    obj.set("x", Value::Number(2.0));
    assert_eq!(obj.get("x"), Some(Value::Number(2.0)));
}

#[test]
fn object_define_accessor() {
    let mut obj = Object::new(ObjectKind::Ordinary);
    let getter = Value::NativeFunction(Rc::new(NativeFunction::new(|_| Ok(Value::Number(99.0)))));
    let flags = PropertyFlags {
        value: None,
        writable: false,
        enumerable: true,
        configurable: true,
    };
    obj.define_accessor("computed", Some(getter), None, flags);
    assert!(obj.has_getter("computed"));
    assert!(!obj.has_setter("computed"));
}

#[test]
fn object_numeric_keys() {
    let mut obj = Object::new(ObjectKind::Ordinary);
    obj.set("0", Value::Number(10.0));
    obj.set("1", Value::Number(20.0));
    assert_eq!(obj.get("0"), Some(Value::Number(10.0)));
    assert_eq!(obj.get("1"), Some(Value::Number(20.0)));
}
