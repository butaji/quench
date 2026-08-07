//! Intl built-in — minimal implementation for test262 conformance.

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{NativeConstructor, NativeFunction, Object, ObjectKind, Value};
use crate::Context;

pub fn register_intl(ctx: &mut Context) {
    let intl = Object::new(ObjectKind::Ordinary);
    let intl_rc = Rc::new(RefCell::new(intl));

    // DateTimeFormat
    setup_datetimeformat(&intl_rc);
    // NumberFormat
    setup_numberformat(&intl_rc);
    // Segmenter
    setup_segmenter(&intl_rc);
    // Static methods
    setup_intl_static_methods(&intl_rc);

    ctx.set_global("Intl".to_string(), Value::Object(intl_rc));
}

fn make_builtin_constructor(
    make_proto: impl FnOnce() -> (Rc<RefCell<Object>>, Value),
    constructor_fn: impl Fn(Vec<Value>) -> Result<Value, crate::JsError> + 'static,
) -> Value {
    let (proto_rc, proto_val) = make_proto();
    let proto_for_ctor = Rc::clone(&proto_rc);
    let ctor = NativeConstructor::new(
        move |args| constructor_fn(args),
        Rc::clone(&proto_for_ctor),
    );
    let ctor_val = Value::NativeConstructor(ctor);
    proto_rc.borrow_mut().properties.insert("constructor".to_string(), ctor_val.clone());
    ctor_val
}

fn setup_datetimeformat(intl: &Rc<RefCell<Object>>) {
    let proto_val = make_builtin_constructor(
        || {
            let proto = Object::new(ObjectKind::Ordinary);
            proto.set("toStringTag", Value::String("Intl.DateTimeFormat".to_string()));
            proto.set("format", Value::NativeFunction(Rc::new(NativeFunction::new(|_| Ok(Value::Undefined)))));
            proto.set("resolvedOptions", Value::NativeFunction(Rc::new(NativeFunction::new(|_| {
                Ok(Value::Object(Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)))))
            }))));
            let proto_rc = Rc::new(RefCell::new(proto));
            (Rc::clone(&proto_rc), Value::Object(proto_rc))
        },
        |_| Ok(Value::Object(Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary))))),
    );
    intl.borrow_mut().set("DateTimeFormat", proto_val);
}

fn setup_numberformat(intl: &Rc<RefCell<Object>>) {
    let proto_val = make_builtin_constructor(
        || {
            let proto = Object::new(ObjectKind::Ordinary);
            proto.set("toStringTag", Value::String("Intl.NumberFormat".to_string()));
            proto.set("format", Value::NativeFunction(Rc::new(NativeFunction::new(|_| Ok(Value::Undefined)))));
            proto.set("resolvedOptions", Value::NativeFunction(Rc::new(NativeFunction::new(|_| {
                Ok(Value::Object(Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)))))
            }))));
            let proto_rc = Rc::new(RefCell::new(proto));
            (Rc::clone(&proto_rc), Value::Object(proto_rc))
        },
        |_| Ok(Value::Object(Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary))))),
    );
    intl.borrow_mut().set("NumberFormat", proto_val);
}

fn setup_segmenter(intl: &Rc<RefCell<Object>>) {
    let proto_val = make_builtin_constructor(
        || {
            let proto = Object::new(ObjectKind::Ordinary);
            proto.set("toStringTag", Value::String("Intl.Segmenter".to_string()));
            proto.set("segment", Value::NativeFunction(Rc::new(NativeFunction::new(|_| Ok(Value::Undefined)))));
            proto.set("resolvedOptions", Value::NativeFunction(Rc::new(NativeFunction::new(|_| {
                Ok(Value::Object(Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)))))
            }))));
            let proto_rc = Rc::new(RefCell::new(proto));
            (Rc::clone(&proto_rc), Value::Object(proto_rc))
        },
        |_| Ok(Value::Object(Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary))))),
    );
    intl.borrow_mut().set("Segmenter", proto_val);
}

fn setup_intl_static_methods(intl: &Rc<RefCell<Object>>) {
    let gc = NativeFunction::new(|_| {
        Ok(Value::Object(Rc::new(RefCell::new(Object::new(ObjectKind::Array)))))
    });
    intl.borrow_mut().set("getCanonicalLocales", Value::NativeFunction(Rc::new(gc)));
    let svo = NativeFunction::new(|_| {
        Ok(Value::Object(Rc::new(RefCell::new(Object::new(ObjectKind::Array)))))
    });
    intl.borrow_mut().set("supportedValuesOf", Value::NativeFunction(Rc::new(svo)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Context;

    fn eval_ok(src: &str) -> Value {
        let mut ctx = Context::new().unwrap();
        ctx.eval(src).unwrap()
    }

    #[test]
    fn intl_exists_as_global() {
        let result = eval_ok("typeof Intl");
        assert_eq!(result.to_string(), "object");
    }

    #[test]
    fn intl_datetimeformat_exists() {
        let result = eval_ok("typeof Intl.DateTimeFormat");
        assert_eq!(result.to_string(), "function");
    }

    #[test]
    fn intl_datetimeformat_creates_instance() {
        let result = eval_ok("(new Intl.DateTimeFormat()).format");
        assert_eq!(result.to_string(), "function");
    }

    #[test]
    fn intl_numberformat_exists() {
        let result = eval_ok("typeof Intl.NumberFormat");
        assert_eq!(result.to_string(), "function");
    }

    #[test]
    fn intl_numberformat_creates_instance() {
        let result = eval_ok("(new Intl.NumberFormat()).format");
        assert_eq!(result.to_string(), "function");
    }

    #[test]
    fn intl_segmenter_exists() {
        let result = eval_ok("typeof Intl.Segmenter");
        assert_eq!(result.to_string(), "function");
    }

    #[test]
    fn intl_get_canonical_locales_exists() {
        let result = eval_ok("typeof Intl.getCanonicalLocales");
        assert_eq!(result.to_string(), "function");
    }

    #[test]
    fn intl_get_canonical_locales_returns_array() {
        let result = eval_ok("Array.isArray(Intl.getCanonicalLocales(['en-US']))");
        assert_eq!(result.to_string(), "true");
    }

    #[test]
    fn intl_supported_values_of_exists() {
        let result = eval_ok("typeof Intl.supportedValuesOf");
        assert_eq!(result.to_string(), "function");
    }

    #[test]
    fn intl_supported_values_of_returns_array() {
        let result = eval_ok("Array.isArray(Intl.supportedValuesOf('timeZone'))");
        assert_eq!(result.to_string(), "true");
    }
}
