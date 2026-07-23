//! Function built-in

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::object::helpers::PropertyFlags;
use crate::value::{
    to_js_string, to_number_unchecked, JsError, NativeConstructor, NativeFunction, Object,
    ObjectKind, Value, ValueFunction,
};
use crate::Context;

// Thread-local storage for Function.prototype and special function prototypes
thread_local! {
    static FUNCTION_PROTOTYPE: RefCell<Option<Rc<RefCell<Object>>>> = const { RefCell::new(None) };
    static GENERATOR_FUNCTION_PROTOTYPE: RefCell<Option<Rc<RefCell<Object>>>> =
        const { RefCell::new(None) };
    static ASYNC_FUNCTION_PROTOTYPE: RefCell<Option<Rc<RefCell<Object>>>> =
        const { RefCell::new(None) };
    static ASYNC_GENERATOR_FUNCTION_PROTOTYPE: RefCell<Option<Rc<RefCell<Object>>>> =
        const { RefCell::new(None) };
}

/// Get the Function.prototype object (for use by interpreter)
pub fn get_function_prototype() -> Option<Rc<RefCell<Object>>> {
    FUNCTION_PROTOTYPE.with(|fp| fp.borrow().clone())
}

/// Get the GeneratorFunction.prototype object
pub fn get_generator_function_prototype() -> Option<Rc<RefCell<Object>>> {
    GENERATOR_FUNCTION_PROTOTYPE.with(|p| p.borrow().clone())
}

/// Get the AsyncFunction.prototype object
pub fn get_async_function_prototype() -> Option<Rc<RefCell<Object>>> {
    ASYNC_FUNCTION_PROTOTYPE.with(|p| p.borrow().clone())
}

/// Get the AsyncGeneratorFunction.prototype object
pub fn get_async_generator_function_prototype() -> Option<Rc<RefCell<Object>>> {
    ASYNC_GENERATOR_FUNCTION_PROTOTYPE.with(|p| p.borrow().clone())
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

/// Get the GeneratorFunction.prototype object (for %GeneratorPrototype% [[GetPrototypeOf]])
pub fn get_generator_function_prototype() -> Option<Rc<RefCell<Object>>> {
    GENERATOR_FUNCTION_PROTOTYPE.with(|fp| fp.borrow().clone())
}

/// Get the AsyncFunction.prototype object (for %AsyncFunctionPrototype% [[GetPrototypeOf]])
pub fn get_async_function_prototype() -> Option<Rc<RefCell<Object>>> {
    ASYNC_FUNCTION_PROTOTYPE.with(|fp| fp.borrow().clone())
}

/// Get the AsyncGeneratorFunction.prototype object (for %AsyncGeneratorPrototype% [[GetPrototypeOf]])
pub fn get_async_generator_function_prototype() -> Option<Rc<RefCell<Object>>> {
    ASYNC_GENERATOR_FUNCTION_PROTOTYPE.with(|fp| fp.borrow().clone())
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

    let target_for_closure = target_func.clone();
    let bound_func = NativeFunction::new(move |extra_args: Vec<Value>| {
        crate::interpreter::set_this_value(bound_this.clone());
        let mut all_args = bound_args.clone();
        all_args.extend(extra_args);
        let result = crate::eval::call_value_with_this(
            target_for_closure.clone(),
            all_args,
            bound_this.clone(),
        );
        crate::interpreter::take_this_value();
        result
    });
    let _ = bound_func.set_property("length", Value::Number(bound_len as f64));
    let _ = bound_func.set_property("name", Value::String(format!("bound {}", target_name)));
    let _ = bound_func.set_property("__quench_bound_target", target_func);

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
        make_function_constructor("", function_proto.clone(), Rc::clone(ctx.env()));
    function_constructor.set_name("Function");
    let func_ctor = Value::NativeConstructor(Rc::new(function_constructor));
    // Set Function.prototype.constructor = Function
    function_proto
        .borrow_mut()
        .set("constructor", func_ctor.clone());
    ctx.set_global("Function".to_string(), func_ctor);

    // Register AsyncFunction, GeneratorFunction, AsyncGeneratorFunction
    // as native constructors that delegate to the Function constructor logic
    // but generate the correct source text per kind.
    // Each has its own prototype object so Object.getPrototypeOf(genFn) returns
    // the correct prototype (GeneratorFunction.prototype, not Function.prototype).
    let async_func_ctor =
        make_function_constructor("async ", function_proto.clone(), Rc::clone(ctx.env()));
    async_func_ctor.set_name("AsyncFunction");
    let async_func_ctor_val = Value::NativeConstructor(Rc::new(async_func_ctor));
    let async_func_proto = make_function_kind_prototype(async_func_ctor_val.clone());
    ASYNC_FUNCTION_PROTOTYPE.with(|p| *p.borrow_mut() = Some(Rc::clone(&async_func_proto)));
    ctx.set_global("AsyncFunction".to_string(), async_func_ctor_val);

    let gen_func_ctor =
        make_function_constructor("*", function_proto.clone(), Rc::clone(ctx.env()));
    gen_func_ctor.set_name("GeneratorFunction");
    let gen_func_ctor_val = Value::NativeConstructor(Rc::new(gen_func_ctor));
    let gen_func_proto = make_function_kind_prototype(gen_func_ctor_val.clone());
    GENERATOR_FUNCTION_PROTOTYPE.with(|p| *p.borrow_mut() = Some(Rc::clone(&gen_func_proto)));
    ctx.set_global("GeneratorFunction".to_string(), gen_func_ctor_val);

    let async_gen_func_ctor =
        make_function_constructor("async *", function_proto.clone(), Rc::clone(ctx.env()));
    async_gen_func_ctor.set_name("AsyncGeneratorFunction");
    let async_gen_func_ctor_val = Value::NativeConstructor(Rc::new(async_gen_func_ctor));
    let async_gen_func_proto = make_function_kind_prototype(async_gen_func_ctor_val.clone());
    ASYNC_GENERATOR_FUNCTION_PROTOTYPE
        .with(|p| *p.borrow_mut() = Some(Rc::clone(&async_gen_func_proto)));
    ctx.set_global(
        "AsyncGeneratorFunction".to_string(),
        async_gen_func_ctor_val,
    );
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
            use crate::builtins::get_native_this;
            match get_native_this() {
                Some(Value::Function(f)) => Ok(Value::String(f.source_text())),
                Some(Value::NativeFunction(_)) | Some(Value::NativeConstructor(_)) => {
                    Ok(Value::String("[Function]".to_string()))
                }
                Some(Value::Generator(_)) | Some(Value::Class(_)) => {
                    Ok(Value::String("[Function]".to_string()))
                }
                _ => Ok(Value::String("[Function]".to_string())),
            }
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
    // ES §16.1: caller/arguments accessors throw TypeError for strict/class functions
    let thrower = Value::NativeFunction(Rc::new(NativeFunction::new(|_: Vec<Value>| {
        let (_, js_err) = crate::value::error::create_js_error_with_type(
            "'caller' and 'arguments' are restricted properties and cannot be accessed on this function",
            "TypeError",
        );
        Err(js_err)
    })));
    function_proto_rc.borrow_mut().define_accessor(
        "caller",
        Some(thrower.clone()),
        Some(thrower.clone()),
        crate::value::object::helpers::PropertyFlags {
            value: None,
            writable: false,
            enumerable: false,
            configurable: true,
        },
    );
    function_proto_rc.borrow_mut().define_accessor(
        "arguments",
        Some(thrower.clone()),
        Some(thrower.clone()),
        crate::value::object::helpers::PropertyFlags {
            value: None,
            writable: false,
            enumerable: false,
            configurable: true,
        },
    );
    function_proto_rc
}

/// Create a prototype object for a specific function kind (GeneratorFunction,
/// AsyncFunction, AsyncGeneratorFunction). This object serves as the [[Prototype]]
/// of instances created by that kind (e.g., `Object.getPrototypeOf(function*() {})`
/// returns GeneratorFunction.prototype). Its own [[Prototype]] is Function.prototype
/// so that generator/async functions inherit Function.prototype methods.
fn make_function_kind_prototype(ctor: Value) -> Rc<RefCell<Object>> {
    let proto = Object::new(ObjectKind::Ordinary);
    let proto_rc = Rc::new(RefCell::new(proto));
    if let Some(fp) = get_function_prototype() {
        proto_rc.borrow_mut().prototype = Some(fp);
    }
    proto_rc.borrow_mut().set("constructor", ctor);
    proto_rc
}

fn make_function_constructor(
    kind: &str,
    function_proto: Rc<RefCell<Object>>,
    global_env: Rc<RefCell<crate::env::Environment>>,
) -> NativeConstructor {
    let kind_owned = kind.to_string();
    NativeConstructor::new(
        move |args| {
            // new Function(arg1, ..., argN, body): compile a real function
            // whose closure is the global scope.
            // The `kind` prefix determines whether the function is a generator,
            // async, or async-generator function.
            //   ""        → function anonymous(...) { body }
            //   "*"       → function* anonymous(...) { body }
            //   "async "  → async function anonymous(...) { body }
            //   "async *" → async function* anonymous(...) { body }
            let body_src = args.last().map(to_js_string).unwrap_or_default();
            let params_src = args[..args.len().saturating_sub(1)]
                .iter()
                .map(to_js_string)
                .collect::<Vec<_>>()
                .join(",");
            let source = format!(
                "function{} anonymous({}) {{\n{}\n}}",
                kind_owned, params_src, body_src
            );
            // Per ES spec §16.1, a hashbang comment (#! ...) is only valid at the
            // very beginning of source text. The Function constructor wraps the body
            // in `function anonymous() { ... }`, so a hashbang inside the body is
            // not at the start and must be rejected as a SyntaxError.
            // OXC 0.47 accepts hashbang anywhere, so we check here.
            if let Some(body_start) = body_src.find("#!") {
                let line_before = &body_src[..body_start];
                if !line_before.contains('\n') && params_src.is_empty() {
                    // First-line hashbang in body: OK for the first argument
                } else {
                    let (err_val, js_err) = crate::value::error::create_js_error_with_type(
                        "Function constructor produced: Unexpected hashbang comment",
                        "SyntaxError",
                    );
                    crate::value::set_thrown_value(err_val);
                    return Err(js_err);
                }
            }
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
                        let param_count = params.len();
                        let func = Value::Function(ValueFunction::new(
                            Some(name.clone()),
                            params,
                            body,
                            Rc::clone(&global_env),
                            is_async,
                            is_generator,
                        ));
                        // When called via super() from a derived class (detected by
                        // native_this being an existing object), store the function
                        // on the existing object's internal slots instead of creating
                        // a new Value::Function. This preserves the derived class's
                        // prototype chain on the object.
                        if let Some(Value::Object(existing)) = crate::interpreter::get_native_this()
                        {
                            existing.borrow_mut().call_slot = Some(func);
                            {
                                let mut obj = existing.borrow_mut();
                                // Set .length as own property (writable: false, enumerable: false, configurable: true)
                                obj.define(
                                    "length",
                                    Value::Number(param_count as f64),
                                    PropertyFlags {
                                        value: None,
                                        writable: false,
                                        enumerable: false,
                                        configurable: true,
                                    },
                                );
                                // Set .name as own property ("anonymous") per CreateDynamicFunction
                                obj.define(
                                    "name",
                                    Value::String(name.clone()),
                                    PropertyFlags {
                                        value: None,
                                        writable: false,
                                        enumerable: false,
                                        configurable: true,
                                    },
                                );
                                // Set .prototype for non-Async functions (Normal, Generator, AsyncGenerator)
                                // Async functions (kind starts with "async " but not "async *") get no .prototype
                                if kind_owned.is_empty()
                                    || kind_owned == "*"
                                    || kind_owned == "async *"
                                {
                                    let mut proto = Object::new(ObjectKind::Ordinary);
                                    // Set the prototype of this object to Object.prototype
                                    // so that methods like hasOwnProperty work.
                                    if let Some(obj_proto) = crate::builtins::get_object_prototype()
                                    {
                                        proto.prototype = Some(obj_proto);
                                    }
                                    obj.define(
                                        "prototype",
                                        Value::Object(Rc::new(RefCell::new(proto))),
                                        PropertyFlags {
                                            value: None,
                                            writable: true,
                                            enumerable: false,
                                            configurable: false,
                                        },
                                    );
                                }
                            }
                            Ok(Value::Object(existing))
                        } else {
                            Ok(func)
                        }
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
    fn test_class_extends_function_is_callable() {
        let mut ctx = Context::new().unwrap();
        let result = ctx
            .eval(
                "class MyFn extends Function { constructor() { super('return 1'); } }
                 var fn = new MyFn();
                 fn();",
            )
            .unwrap();
        assert_eq!(result, Value::Number(1.0));
    }

    #[test]
    fn test_class_extends_function_instanceof() {
        let mut ctx = Context::new().unwrap();
        let result = ctx
            .eval(
                "class MyFn extends Function { constructor() { super('return 1'); } }
                 var fn = new MyFn();
                 fn instanceof MyFn;",
            )
            .unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_class_extends_function_instanceof_function() {
        let mut ctx = Context::new().unwrap();
        let result = ctx
            .eval(
                "class MyFn extends Function { constructor() { super('return 1'); } }
                 var fn = new MyFn();
                 fn instanceof Function;",
            )
            .unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_class_extends_function_no_explicit_ctor() {
        let mut ctx = Context::new().unwrap();
        let result = ctx
            .eval(
                "class MyFn extends Function {}
                 var fn = new MyFn('return 42');
                 fn();",
            )
            .unwrap();
        assert_eq!(result, Value::Number(42.0));
    }

    #[test]
    fn test_class_extends_function_is_callable_with_args() {
        let mut ctx = Context::new().unwrap();
        let result = ctx
            .eval(
                "class Adder extends Function { constructor() { super('a', 'b', 'return a + b'); } }
                 var add = new Adder();
                 add(3, 4);",
            )
            .unwrap();
        assert_eq!(result, Value::Number(7.0));
    }

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
    fn test_class_extends_function_has_length_own_prop() {
        let mut ctx = Context::new().unwrap();
        let result = ctx
            .eval(
                "class Fn extends Function {}
                 var fn = new Fn('a', 'b', 'return a + b');
                 Object.getOwnPropertyDescriptor(fn, 'length').value;",
            )
            .unwrap();
        assert_eq!(result, Value::Number(2.0));
        let desc = ctx
            .eval(
                "class Fn extends Function {}
                 var fn = new Fn('a', 'b', 'return a + b');
                 Object.getOwnPropertyDescriptor(fn, 'length');",
            )
            .unwrap();
        if let Value::Object(o) = desc {
            assert_eq!(o.borrow().get("writable"), Some(Value::Boolean(false)));
            assert_eq!(o.borrow().get("enumerable"), Some(Value::Boolean(false)));
            assert_eq!(o.borrow().get("configurable"), Some(Value::Boolean(true)));
        } else {
            panic!("expected object descriptor");
        }
    }

    #[test]
    fn test_class_extends_function_has_name_own_prop() {
        let mut ctx = Context::new().unwrap();
        let desc = ctx
            .eval(
                "class Fn extends Function {}
                 var fn = new Fn('a', 'b', 'return a + b');
                 Object.getOwnPropertyDescriptor(fn, 'name');",
            )
            .unwrap();
        if let Value::Object(o) = desc {
            assert_eq!(
                o.borrow().get("value"),
                Some(Value::String("anonymous".to_string()))
            );
            assert_eq!(o.borrow().get("writable"), Some(Value::Boolean(false)));
            assert_eq!(o.borrow().get("enumerable"), Some(Value::Boolean(false)));
            assert_eq!(o.borrow().get("configurable"), Some(Value::Boolean(true)));
        } else {
            panic!("expected object descriptor, got {:?}", desc);
        }
    }

    #[test]
    fn test_class_extends_generatorfunction_has_prototype() {
        let mut ctx = Context::new().unwrap();
        // Everything in one eval to avoid scoping issues
        let r = ctx
            .eval(
                "class GFn extends GeneratorFunction {}
                 var gfn = new GFn(';');
                 [Object.keys(gfn.prototype).length,
                  gfn.prototype.hasOwnProperty('constructor'),
                  Object.getOwnPropertyDescriptor(gfn, 'prototype').writable,
                  Object.getOwnPropertyDescriptor(gfn, 'prototype').enumerable,
                  Object.getOwnPropertyDescriptor(gfn, 'prototype').configurable];",
            )
            .unwrap();
        if let Value::Object(o) = r {
            assert_eq!(o.borrow().get("0"), Some(Value::Number(0.0)));
            assert_eq!(o.borrow().get("1"), Some(Value::Boolean(false)));
            assert_eq!(o.borrow().get("2"), Some(Value::Boolean(true)));
            assert_eq!(o.borrow().get("3"), Some(Value::Boolean(false)));
            assert_eq!(o.borrow().get("4"), Some(Value::Boolean(false)));
        } else {
            panic!("expected array, got {:?}", r);
        }
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
