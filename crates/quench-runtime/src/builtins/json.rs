//! JSON built-in

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_js_string, NativeFunction, Object, Value};
use crate::Context;
use super::JsValueProxy;

// ============================================================================
// JSON
// ============================================================================

pub fn register_json(ctx: &mut Context) {
    let json_obj = Object::new(crate::value::ObjectKind::Ordinary);
    let json = Rc::new(RefCell::new(json_obj));

    json.borrow_mut().set("stringify", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let val = args.first().cloned().unwrap_or(Value::Undefined);
        let result = serde_json::to_string(&JsValueProxy(&val)).unwrap_or_else(|_| "null".to_string());
        Ok(Value::String(result))
    }))));

    json.borrow_mut().set("parse", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let text = args.first().map(to_js_string).unwrap_or_default();
        Ok(Value::String(text))
    }))));

    ctx.set_global("JSON".to_string(), Value::Object(json));
}
