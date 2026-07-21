//! Proxy helper functions.

use crate::value::{JsError, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Find a proxy in the prototype chain, returning (proxy_rc, handler, target).
#[allow(clippy::type_complexity)]
pub fn find_proxy_in_prototype_chain(
    obj: &Rc<RefCell<crate::value::Object>>,
    _prop_name: &str,
) -> Option<(
    Rc<RefCell<crate::value::Object>>,
    Rc<RefCell<crate::value::Object>>,
    Value,
)> {
    let mut current = obj
        .borrow()
        .prototype
        .as_ref()
        .and_then(|p| p.borrow().prototype.clone());
    loop {
        let proto_rc = current?;
        let proto = proto_rc.borrow();
        if let Some(Value::Object(handler)) = proto.properties.get("__quench_proxy_handler") {
            if let Some(target) = proto.properties.get("__quench_proxy_target") {
                return Some((proto_rc.clone(), handler.clone(), target.clone()));
            }
        }
        current = proto.prototype.clone();
    }
}

/// Invoke the proxy set trap, returning whether the assignment succeeded.
pub fn call_proxy_set_trap(
    target: &Value,
    handler: &Rc<RefCell<crate::value::Object>>,
    this_val: &Value,
    prop_name: &str,
    value: Value,
) -> Result<bool, JsError> {
    let trap_opt: Option<Value> = handler
        .borrow()
        .get_own_value("set")
        .or_else(|| handler.borrow().get_setter_func("set"));
    let Some(trap) = trap_opt else {
        return Ok(true);
    };
    let trap = match trap {
        Value::Object(f) => Value::Object(f),
        Value::Function(f) => Value::Function(f),
        Value::NativeFunction(nf) => Value::NativeFunction(nf),
        Value::NativeConstructor(nc) => Value::NativeConstructor(nc),
        Value::Undefined => return Ok(true),
        _ => {
            let (err_val, js_err) = crate::value::error::create_js_error_with_type(
                "set trap is not callable",
                "TypeError",
            );
            crate::value::set_thrown_value(err_val);
            return Err(js_err);
        }
    };
    let trap_result = crate::eval::function::call_value_with_this(
        trap,
        vec![target.clone(), Value::String(prop_name.to_string()), value],
        this_val.clone(),
    );
    match trap_result {
        Ok(result) => Ok(crate::value::to_bool(&result)),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use crate::Context;
    use crate::Value;

    fn eval(src: &str) -> Result<Value, crate::value::JsError> {
        Context::new().unwrap().eval(src)
    }

    // ─── Proxy: basic get trap ──────────────────────────────────────────────

    #[test]
    fn proxy_basic_get() {
        let r = eval(
            "var target = {x: 1}; var handler = {get(o, k) { return o[k] * 2; }}; var p = new Proxy(target, handler); p.x",
        );
        assert!(r.is_ok());
    }

    // ─── Proxy: basic set trap ──────────────────────────────────────────────

    #[test]
    fn proxy_basic_set() {
        let r = eval(
            "var target = {}; var handler = {set(o, k, v) { o[k] = v + 1; return true; }}; var p = new Proxy(target, handler); p.a = 5; target.a",
        );
        assert!(r.is_ok());
    }
}
