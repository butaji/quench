//! Function built-in

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{
    to_js_string, to_number_unchecked, JsError, NativeConstructor, NativeFunction, Object,
    ObjectKind, Value, ValueFunction,
};
use crate::Context;

// Thread-local storage for Function.prototype (used by interpreter for function expressions)
thread_local! {
    static FUNCTION_PROTOTYPE: RefCell<Option<Rc<RefCell<Object>>>> = const { RefCell::new(None) };
}

/// Get the Function.prototype object (for use by interpreter)
pub fn get_function_prototype() -> Option<Rc<RefCell<Object>>> {
    FUNCTION_PROTOTYPE.with(|fp| fp.borrow().clone())
}

/// Check if an object is Function.prototype (for special property access handling)
pub fn is_function_prototype(obj: &Rc<RefCell<Object>>) -> bool {
    FUNCTION_PROTOTYPE.with(|fp| {
        if let Some(ref func_proto) = *fp.borrow() {
            Rc::ptr_eq(obj, func_proto)
        } else {
            false
        }
    })
}

/// Get the error message for restricted function properties
pub fn get_restricted_prop_error() -> String {
    "TypeError: Function.prototype.caller and Function.prototype.arguments ".to_string()
        + "are not allowed to be accessed on this function"
}

// ============================================================================
// Function.prototype.call implementation
// ============================================================================

/// Function.prototype.call(thisArg, ...args)
fn proto_call(args: Vec<Value>) -> Result<Value, JsError> {
    let func = crate::builtins::get_native_this()
        .ok_or_else(|| JsError("Function.prototype.call called on non-function".to_string()))?;

    let this_arg = args.first().cloned().unwrap_or(Value::Undefined);
    let call_args = if args.len() > 1 {
        args[1..].to_vec()
    } else {
        vec![]
    };

    crate::interpreter::set_this_value(this_arg.clone());
    let result = crate::eval::call_value_with_this(func, call_args, this_arg);
    crate::interpreter::take_this_value();
    result
}

// ============================================================================
// Function.prototype.apply implementation
// ============================================================================

/// Function.prototype.apply(thisArg, argsArray)
fn proto_apply(args: Vec<Value>) -> Result<Value, JsError> {
    let func = crate::builtins::get_native_this()
        .ok_or_else(|| JsError("Function.prototype.apply called on non-function".to_string()))?;

    let this_arg = args.first().cloned().unwrap_or(Value::Undefined);
    let array_like = args.get(1);

    let call_args = extract_args_from_array_like(array_like)?;
    crate::interpreter::set_this_value(this_arg.clone());
    let result = crate::eval::call_value_with_this(func, call_args, this_arg);
    crate::interpreter::take_this_value();
    result
}

/// Extract arguments from an array-like object
fn extract_args_from_array_like(array_like: Option<&Value>) -> Result<Vec<Value>, JsError> {
    match array_like {
        None | Some(Value::Undefined) | Some(Value::Null) => Ok(vec![]),
        Some(Value::Object(o)) => {
            let obj = o.borrow();
            let len_val = obj.get("length");
            let len = len_val
                .as_ref()
                .map(|v| to_number_unchecked(v) as usize)
                .unwrap_or(0);
            let mut args = Vec::with_capacity(len);
            for i in 0..len {
                if let Some(arg) = obj.get(&i.to_string()) {
                    args.push(arg.clone());
                } else {
                    args.push(Value::Undefined);
                }
            }
            Ok(args)
        }
        _ => Ok(vec![]),
    }
}

// ============================================================================
// Function.prototype.bind implementation
// ============================================================================

/// Function.prototype.bind(thisArg, ...args)
fn proto_bind(args: Vec<Value>) -> Result<Value, JsError> {
    let target_func = crate::builtins::get_native_this()
        .ok_or_else(|| JsError("Function.prototype.bind called on non-function".to_string()))?;

    let bound_this = args.first().cloned().unwrap_or(Value::Undefined);
    let bound_args: Vec<Value> = if args.len() > 1 {
        args[1..].to_vec()
    } else {
        vec![]
    };

    // Bound functions get length = max(0, target.length - boundArgs.length)
    // and name = "bound " + target.name
    let (target_len, target_name) = match &target_func {
        Value::Function(f) => (f.length(), f.name.clone().unwrap_or_default()),
        _ => (0, String::new()),
    };
    let bound_len = target_len.saturating_sub(bound_args.len());

    let bound_func = NativeFunction::new(move |extra_args: Vec<Value>| {
        crate::interpreter::set_this_value(bound_this.clone());
        let mut all_args = bound_args.clone();
        all_args.extend(extra_args);
        let result =
            crate::eval::call_value_with_this(target_func.clone(), all_args, bound_this.clone());
        crate::interpreter::take_this_value();
        result
    });
    let _ = bound_func.set_property("length", Value::Number(bound_len as f64));
    let _ = bound_func.set_property("name", Value::String(format!("bound {}", target_name)));

    Ok(Value::NativeFunction(Rc::new(bound_func)))
}

// ============================================================================
// Function
// ============================================================================

pub fn register_function(ctx: &mut Context) {
    let function_proto = make_function_prototype();
    FUNCTION_PROTOTYPE.with(|fp| {
        *fp.borrow_mut() = Some(Rc::clone(&function_proto));
    });

    let function_constructor =
        make_function_constructor(function_proto.clone(), Rc::clone(ctx.env()));
    function_constructor.set_name("Function");
    let func_ctor = Value::NativeConstructor(Rc::new(function_constructor));
    // Set Function.prototype.constructor = Function
    function_proto
        .borrow_mut()
        .set("constructor", func_ctor.clone());
    ctx.set_global("Function".to_string(), func_ctor);
}

fn make_function_prototype() -> Rc<RefCell<Object>> {
    let function_proto = Object::new(ObjectKind::Function);
    let function_proto_rc = Rc::new(RefCell::new(function_proto));

    if let Some(object_proto) = crate::builtins::get_object_prototype() {
        function_proto_rc.borrow_mut().prototype = Some(object_proto);
    }

    function_proto_rc.borrow_mut().set(
        "toString",
        Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
            Ok(Value::String("[Function]".to_string()))
        }))),
    );
    function_proto_rc
        .borrow_mut()
        .set("length", Value::Number(0.0));
    function_proto_rc
        .borrow_mut()
        .set("name", Value::String(String::new()));
    function_proto_rc.borrow_mut().set(
        "call",
        Value::NativeFunction(Rc::new(NativeFunction::new(proto_call))),
    );
    function_proto_rc.borrow_mut().set(
        "apply",
        Value::NativeFunction(Rc::new(NativeFunction::new(proto_apply))),
    );
    function_proto_rc.borrow_mut().set(
        "bind",
        Value::NativeFunction(Rc::new(NativeFunction::new(proto_bind))),
    );
    function_proto_rc
}

fn make_function_constructor(
    function_proto: Rc<RefCell<Object>>,
    global_env: Rc<RefCell<crate::env::Environment>>,
) -> NativeConstructor {
    NativeConstructor::new(
        move |args| {
            // new Function(arg1, ..., argN, body): compile a real function
            // whose closure is the global scope
            let body_src = args.last().map(to_js_string).unwrap_or_default();
            let params_src = args[..args.len().saturating_sub(1)]
                .iter()
                .map(to_js_string)
                .collect::<Vec<_>>()
                .join(",");
            let source = format!("function anonymous({}) {{\n{}\n}}", params_src, body_src);
            match crate::parser::parse_script(&source) {
                Ok(crate::ast::Program::Script(stmts)) => {
                    if let Some(crate::ast::Statement::FunctionDeclaration {
                        name,
                        params,
                        body,
                        is_async,
                        is_generator,
                    }) = stmts.into_iter().next()
                    {
                        Ok(Value::Function(ValueFunction::new(
                            Some(name),
                            params,
                            body,
                            Rc::clone(&global_env),
                            is_async,
                            is_generator,
                        )))
                    } else {
                        Err(JsError::new(
                            "SyntaxError: Function constructor produced no function",
                        ))
                    }
                }
                Err(e) => {
                    let (err_val, js_err) = crate::value::error::create_js_error_with_type(
                        &format!("Function constructor produced: {}", e.0),
                        "SyntaxError",
                    );
                    crate::value::set_thrown_value(err_val);
                    Err(js_err)
                }
            }
        },
        function_proto,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_function_constructor_compiles_real_function() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("var f = Function('a', 'return a'); f(3)").unwrap();
        assert_eq!(result, Value::Number(3.0));
    }

    #[test]
    fn test_function_constructor_multiple_params() {
        let mut ctx = Context::new().unwrap();
        let result = ctx
            .eval("Function('a', 'b', 'return a + b')(2, 5)")
            .unwrap();
        assert_eq!(result, Value::Number(7.0));
    }

    #[test]
    fn test_function_constructor_uses_global_scope() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("var g = 41; Function('return g + 1')()").unwrap();
        assert_eq!(result, Value::Number(42.0));
    }

    #[test]
    fn test_function_constructor_invalid_body_throws() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("Function('a', 'return a @ b')");
        assert!(result.is_err(), "invalid body must throw SyntaxError");
    }

    #[test]
    fn test_function_constructor_immediate_call() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("Function('a', 'return a')(3)").unwrap();
        assert_eq!(result, Value::Number(3.0));
    }

    #[test]
    fn test_bind_sets_length_and_name() {
        let mut ctx = Context::new().unwrap();
        // Exercise Function.prototype.bind explicitly (proto_bind path)
        let len = ctx
            .eval("Function.prototype.bind.call(function foo(a, b) {}, null, 1).length")
            .unwrap();
        assert_eq!(len, Value::Number(1.0));
        let name = ctx
            .eval("Function.prototype.bind.call(function foo(a, b) {}, null).name")
            .unwrap();
        assert_eq!(name, Value::String("bound foo".to_string()));
    }
}
