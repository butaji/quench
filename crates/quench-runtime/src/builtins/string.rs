//! String built-in - shared String.prototype object

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_number, NativeFunction, Object, ObjectKind, Value};
use crate::Context;

pub mod methods;

use methods::install_string_methods;

// Thread-local storage for String.prototype (created once, shared)
thread_local! {
    static STRING_PROTOTYPE: RefCell<Option<Rc<RefCell<Object>>>> = const { RefCell::new(None) };
}

/// Get the String.prototype object
pub fn get_string_prototype() -> Option<Rc<RefCell<Object>>> {
    STRING_PROTOTYPE.with(|sp| sp.borrow().clone())
}

/// Register String.fromCharCode and String.fromCodePoint methods
fn register_string_static_methods(string_obj: &Rc<RefCell<Object>>) {
    let from_char_code = NativeFunction::new(|args| {
        let chars: String = args
            .iter()
            .map(|v| {
                let code = to_number(v) as u16;
                std::char::from_u32(code as u32).unwrap_or('\u{FFFD}')
            })
            .collect();
        Ok(Value::String(chars))
    });
    from_char_code.set_property("name", Value::String("fromCharCode".to_string()));
    string_obj.borrow_mut().set(
        "fromCharCode",
        Value::NativeFunction(Rc::new(from_char_code)),
    );

    let from_code_point = NativeFunction::new(|args| {
        let chars: String = args
            .iter()
            .map(|v| {
                let code = to_number(v) as u32;
                std::char::from_u32(code).unwrap_or('\u{FFFD}')
            })
            .collect();
        Ok(Value::String(chars))
    });
    from_code_point.set_property("name", Value::String("fromCodePoint".to_string()));
    string_obj.borrow_mut().set(
        "fromCodePoint",
        Value::NativeFunction(Rc::new(from_code_point)),
    );
}

/// Register the String object and String.prototype
pub fn register_string(_ctx: &mut Context) {
    let string_obj = Object::new(ObjectKind::Ordinary);
    let string_obj = Rc::new(RefCell::new(string_obj));

    register_string_static_methods(&string_obj);

    // Create String.prototype and attach methods
    let string_proto = Object::new(ObjectKind::Ordinary);
    let string_proto_rc = Rc::new(RefCell::new(string_proto));

    install_string_methods(&string_proto_rc);
    string_obj
        .borrow_mut()
        .set("prototype", Value::Object(Rc::clone(&string_proto_rc)));

    STRING_PROTOTYPE.with(|sp| {
        *sp.borrow_mut() = Some(Rc::clone(&string_proto_rc));
    });

    // Note: String global is registered by date::register_type_converters
    // with proper constructor behavior for new String()
}
