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
    static ASYNC_FUNCTION_PROTOTYPE: RefCell<Option<Rc<RefCell<Object>>>> = const { RefCell::new(None) };
    static GENERATOR_FUNCTION_PROTOTYPE: RefCell<Option<Rc<RefCell<Object>>>> =
        const { RefCell::new(None) };
    static ASYNC_GENERATOR_FUNCTION_PROTOTYPE: RefCell<Option<Rc<RefCell<Object>>>> =
        const { RefCell::new(None) };
    /// %GeneratorPrototype% — the intrinsic prototype of generator instances.
    /// Accessible as %GeneratorFunctionPrototype%.prototype.
    static GENERATOR_PROTOTYPE: RefCell<Option<Rc<RefCell<Object>>>> = const { RefCell::new(None) };
    /// %AsyncGeneratorPrototype% — the intrinsic prototype of async generator instances.
    static ASYNC_GENERATOR_PROTOTYPE: RefCell<Option<Rc<RefCell<Object>>>> =
        const { RefCell::new(None) };
}

/// Get the Function.prototype object (for use by interpreter)
pub fn get_function_prototype() -> Option<Rc<RefCell<Object>>> {
    FUNCTION_PROTOTYPE.with(|fp| fp.borrow().clone())
}

pub fn get_async_function_prototype() -> Option<Rc<RefCell<Object>>> {
    ASYNC_FUNCTION_PROTOTYPE.with(|fp| fp.borrow().clone())
}

pub fn get_generator_function_prototype() -> Option<Rc<RefCell<Object>>> {
    GENERATOR_FUNCTION_PROTOTYPE.with(|fp| fp.borrow().clone())
}

pub fn get_async_generator_function_prototype() -> Option<Rc<RefCell<Object>>> {
    ASYNC_GENERATOR_FUNCTION_PROTOTYPE.with(|fp| fp.borrow().clone())
}

/// Get %GeneratorPrototype% — the intrinsic prototype of generator instances.
pub fn get_generator_prototype() -> Option<Rc<RefCell<Object>>> {
    GENERATOR_PROTOTYPE.with(|gp| gp.borrow().clone())
}

/// Get %AsyncGeneratorPrototype% — the intrinsic prototype of async generator instances.
pub fn get_async_generator_prototype() -> Option<Rc<RefCell<Object>>> {
    ASYNC_GENERATOR_PROTOTYPE.with(|gp| gp.borrow().clone())
}

/// Snapshot of all six function-family prototype caches (realm snapshot support)
pub(crate) type FunctionPrototypes = [Option<Rc<RefCell<Object>>>; 6];

/// Save all function-family prototype caches (realm snapshot support)
pub(crate) fn save_function_prototypes() -> FunctionPrototypes {
    [
        get_function_prototype(),
        get_async_function_prototype(),
        get_generator_function_prototype(),
        get_async_generator_function_prototype(),
        get_generator_prototype(),
        get_async_generator_prototype(),
    ]
}

/// Restore all function-family prototype caches (realm snapshot support)
pub(crate) fn restore_function_prototypes(saved: FunctionPrototypes) {
    let [fp, afp, gfp, agfp, gp, agp] = saved;
    FUNCTION_PROTOTYPE.with(|c| *c.borrow_mut() = fp);
    ASYNC_FUNCTION_PROTOTYPE.with(|c| *c.borrow_mut() = afp);
    GENERATOR_FUNCTION_PROTOTYPE.with(|c| *c.borrow_mut() = gfp);
    ASYNC_GENERATOR_FUNCTION_PROTOTYPE.with(|c| *c.borrow_mut() = agfp);
    GENERATOR_PROTOTYPE.with(|c| *c.borrow_mut() = gp);
    ASYNC_GENERATOR_PROTOTYPE.with(|c| *c.borrow_mut() = agp);
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
    let previous_new_target = crate::interpreter::get_new_target();
    crate::interpreter::set_new_target(None);
    let result = crate::eval::call_value_with_this(func, call_args, this_arg);
    crate::interpreter::set_new_target(previous_new_target);
    crate::interpreter::take_this_value();
    result
}

/// Extract arguments from an array-like object
pub(crate) fn extract_args_from_array_like(
    array_like: Option<&Value>,
) -> Result<Vec<Value>, JsError> {
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
    if !target_func.is_callable() {
        return Err(JsError(
            "TypeError: Function.prototype.bind called on non-function".to_string(),
        ));
    }

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

#[derive(Clone, Copy)]
enum FunctionCtorKind {
    Ordinary,
    Async,
    Generator,
    AsyncGenerator,
}

impl FunctionCtorKind {
    fn parse_prefix(self) -> &'static str {
        match self {
            FunctionCtorKind::Ordinary => "function anonymous",
            FunctionCtorKind::Async => "async function anonymous",
            FunctionCtorKind::Generator => "function* anonymous",
            FunctionCtorKind::AsyncGenerator => "async function* anonymous",
        }
    }

    fn expected_flags(self) -> (bool, bool) {
        match self {
            FunctionCtorKind::Ordinary => (false, false),
            FunctionCtorKind::Async => (true, false),
            FunctionCtorKind::Generator => (false, true),
            FunctionCtorKind::AsyncGenerator => (true, true),
        }
    }
}

pub fn register_function(ctx: &mut Context) {
    let function_proto = make_function_prototype();
    FUNCTION_PROTOTYPE.with(|fp| {
        *fp.borrow_mut() = Some(Rc::clone(&function_proto));
    });
    if let Some(Value::Object(string)) = ctx.get_global("String") {
        string.borrow_mut().prototype = Some(Rc::clone(&function_proto));
    }

    let async_function_proto_rc = Rc::new(RefCell::new(Object::with_prototype(
        ObjectKind::Ordinary,
        Rc::clone(&function_proto),
    )));
    ASYNC_FUNCTION_PROTOTYPE.with(|fp| {
        *fp.borrow_mut() = Some(Rc::clone(&async_function_proto_rc));
    });

    let generator_function_proto_rc = Rc::new(RefCell::new(Object::with_prototype(
        ObjectKind::Ordinary,
        Rc::clone(&function_proto),
    )));
    GENERATOR_FUNCTION_PROTOTYPE.with(|fp| {
        *fp.borrow_mut() = Some(Rc::clone(&generator_function_proto_rc));
    });

    let async_generator_function_proto_rc = Rc::new(RefCell::new(Object::with_prototype(
        ObjectKind::Ordinary,
        Rc::clone(&function_proto),
    )));
    ASYNC_GENERATOR_FUNCTION_PROTOTYPE.with(|fp| {
        *fp.borrow_mut() = Some(Rc::clone(&async_generator_function_proto_rc));
    });

    // Create %GeneratorPrototype% — the intrinsic prototype of generator instances.
    // Inherits from %ObjectPrototype%, and is set as %GeneratorFunctionPrototype%.prototype.
    let generator_proto_rc = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
    if let Some(obj_proto) = crate::builtins::get_object_prototype() {
        generator_proto_rc.borrow_mut().prototype = Some(obj_proto);
    }
    // Set %GeneratorPrototype%.next — native, delegates to GeneratorObject::next
    generator_proto_rc.borrow_mut().set(
        "__next",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let this_val = crate::builtins::get_native_this().ok_or_else(|| {
                JsError("Generator.prototype.next called on incompatible receiver".to_string())
            })?;
            match this_val {
                Value::Generator(gen) => {
                    let arg = args.first().cloned().unwrap_or(Value::Undefined);
                    let result = gen.borrow_mut().next(arg)?;
                    Ok(result.to_object())
                }
                _ => Err(JsError(
                    "TypeError: Generator.prototype.next called on incompatible receiver"
                        .to_string(),
                )),
            }
        }))),
    );
    // Set %GeneratorPrototype%.return — native, delegates to GeneratorObject::return logic
    generator_proto_rc.borrow_mut().set(
        "__return",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let this_val = crate::builtins::get_native_this().ok_or_else(|| {
                JsError("Generator.prototype.return called on incompatible receiver".to_string())
            })?;
            match this_val {
                Value::Generator(gen) => {
                    use crate::value::generator::GeneratorState;
                    let arg = args.first().cloned().unwrap_or(Value::Undefined);
                    let mut g = gen.borrow_mut();
                    if g.state == GeneratorState::Completed {
                        return Ok(crate::value::generator::IteratorResult {
                            value: Value::Undefined,
                            done: true,
                        }
                        .to_object());
                    }
                    let suspended_start = g.state == GeneratorState::Suspended
                        && g.yield_index == 0
                        && g.pending_stmt.is_none();
                    if suspended_start {
                        g.state = GeneratorState::Completed;
                        g.call_env = None;
                        return Ok(crate::value::generator::IteratorResult {
                            value: arg,
                            done: true,
                        }
                        .to_object());
                    }
                    // Close inner iterator if mid for-of
                    if let Some(ref suspend) = g.for_of_suspend {
                        if let Some(close_err) =
                            crate::eval::object::call_iterator_return(&suspend.iterator)
                        {
                            return Err(close_err);
                        }
                    }
                    crate::interpreter::set_control_flow(crate::interpreter::ControlFlow::Return(
                        arg.clone(),
                    ));
                    drop(g);
                    let result = gen.borrow_mut().next(Value::Undefined)?;
                    Ok(result.to_object())
                }
                _ => Err(JsError(
                    "TypeError: Generator.prototype.return called on incompatible receiver"
                        .to_string(),
                )),
            }
        }))),
    );
    // Set %GeneratorPrototype%.throw — native, delegates to GeneratorObject::throw logic
    generator_proto_rc.borrow_mut().set(
        "__throw",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let this_val = crate::builtins::get_native_this().ok_or_else(|| {
                JsError("Generator.prototype.throw called on incompatible receiver".to_string())
            })?;
            match this_val {
                Value::Generator(gen) => {
                    use crate::value::generator::GeneratorState;
                    let arg = args.first().cloned().unwrap_or(Value::Undefined);
                    let mut g = gen.borrow_mut();
                    if g.state == GeneratorState::Completed {
                        return Ok(crate::value::generator::IteratorResult {
                            value: Value::Undefined,
                            done: true,
                        }
                        .to_object());
                    }
                    let suspended_start = g.state == GeneratorState::Suspended
                        && g.yield_index == 0
                        && g.pending_stmt.is_none();
                    if suspended_start {
                        g.state = GeneratorState::Completed;
                        g.call_env = None;
                        return Ok(crate::value::generator::IteratorResult {
                            value: arg,
                            done: true,
                        }
                        .to_object());
                    }
                    crate::interpreter::set_control_flow(crate::interpreter::ControlFlow::Throw(
                        arg.clone(),
                    ));
                    drop(g);
                    let result = gen.borrow_mut().next(Value::Undefined);
                    match result {
                        Ok(ir) => Ok(ir.to_object()),
                        Err(e) => Err(e),
                    }
                }
                _ => Err(JsError(
                    "TypeError: Generator.prototype.throw called on incompatible receiver"
                        .to_string(),
                )),
            }
        }))),
    );
    // Set %GeneratorPrototype%[@@iterator] = function() { return this; }
    // Generator instances are iterable via the iterator protocol.
    // Must use set_symbol() with the well-known Symbol.iterator key so that
    // resolve_iterator_method (which looks up by symbol property_key) finds it.
    if let Some(Value::Symbol(ref sym)) =
        crate::builtins::symbol::get_well_known_symbol_no_ctx("iterator")
    {
        let key = sym.property_key();
        generator_proto_rc.borrow_mut().set_symbol(
            &key,
            Value::NativeFunction(Rc::new(NativeFunction::new(|_args: Vec<Value>| {
                let this_val = crate::builtins::get_native_this().ok_or_else(|| {
                    JsError(
                        "Generator.prototype[Symbol.iterator] called on incompatible receiver"
                            .to_string(),
                    )
                })?;
                Ok(this_val)
            }))),
        );
    }
    for name in ["next", "return", "throw"] {
        let method = generator_proto_rc.borrow().get(name);
        if let Some(flags) = generator_proto_rc.borrow_mut().descriptors.get_mut(name) {
            flags.enumerable = false;
        }
        if let Some(Value::NativeFunction(function)) = method {
            function.define_property(
                "name",
                Value::String(name.to_string()),
                crate::value::PropertyFlags {
                    value: Some(Value::String(name.to_string())),
                    writable: false,
                    enumerable: false,
                    configurable: true,
                },
            );
            function.define_property(
                "length",
                Value::Number(1.0),
                crate::value::PropertyFlags {
                    value: Some(Value::Number(1.0)),
                    writable: false,
                    enumerable: false,
                    configurable: true,
                },
            );
        }
    }
    // Set %GeneratorPrototype%[@@toStringTag] = "Generator"
    if let Some(Value::Symbol(symbol)) =
        crate::builtins::symbol::get_well_known_symbol_no_ctx("toStringTag")
    {
        let key = symbol.property_key();
        let mut generator_proto = generator_proto_rc.borrow_mut();
        generator_proto.set_symbol(&key, Value::String("Generator".to_string()));
        if let Some(flags) = generator_proto.descriptors.get_mut(&key) {
            flags.writable = false;
            flags.enumerable = false;
        }
    }
    GENERATOR_PROTOTYPE.with(|gp| {
        *gp.borrow_mut() = Some(Rc::clone(&generator_proto_rc));
    });
    // Wire %GeneratorFunctionPrototype%.prototype = %GeneratorPrototype%
    generator_function_proto_rc
        .borrow_mut()
        .set("prototype", Value::Object(Rc::clone(&generator_proto_rc)));
    if let Some(descriptor) = generator_function_proto_rc
        .borrow_mut()
        .descriptors
        .get_mut("prototype")
    {
        descriptor.writable = false;
        descriptor.enumerable = false;
        descriptor.configurable = true;
    }

    // Create %AsyncGeneratorPrototype% — the intrinsic prototype of async generator instances.
    let async_generator_proto_rc = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
    async_generator_proto_rc.borrow_mut().prototype =
        crate::builtins::iterator::get_async_iterator_prototype();
    ASYNC_GENERATOR_PROTOTYPE.with(|gp| {
        *gp.borrow_mut() = Some(Rc::clone(&async_generator_proto_rc));
    });
    // Wire %AsyncGeneratorFunctionPrototype%.prototype = %AsyncGeneratorPrototype%
    async_generator_function_proto_rc.borrow_mut().set(
        "prototype",
        Value::Object(Rc::clone(&async_generator_proto_rc)),
    );

    // Set %AsyncGeneratorPrototype%.next — returns a Promise resolved with the step result
    async_generator_proto_rc.borrow_mut().set(
        "__next",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let this_val = crate::builtins::get_native_this().ok_or_else(|| {
                JsError("AsyncGenerator.prototype.next called on incompatible receiver".to_string())
            })?;
            match this_val {
                Value::Generator(gen) => {
                    let arg = args.first().cloned().unwrap_or(Value::Undefined);
                    let proto = crate::builtins::promise::get_promise_proto();
                    let result = gen.borrow_mut().next(arg);
                    match result {
                        Ok(ir) => crate::builtins::promise::promise_resolve_impl_static(
                            vec![ir.to_object()],
                            proto,
                        ),
                        Err(e) => crate::builtins::promise::promise_reject_impl_static(
                            vec![Value::String(e.to_string())],
                            proto,
                        ),
                    }
                }
                _ => Err(JsError(
                    "TypeError: AsyncGenerator.prototype.next called on incompatible receiver"
                        .to_string(),
                )),
            }
        }))),
    );
    // Set %AsyncGeneratorPrototype%.return — completes the generator and returns a resolved Promise
    async_generator_proto_rc.borrow_mut().set(
        "__return",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let this_val = crate::builtins::get_native_this().ok_or_else(|| {
                JsError(
                    "AsyncGenerator.prototype.return called on incompatible receiver".to_string(),
                )
            })?;
            match this_val {
                Value::Generator(ref gen) => crate::eval::function::call_value_with_this(
                    crate::value::generator::async_generator_return_fn(Rc::clone(gen)),
                    args,
                    this_val,
                ),
                _ => Err(JsError(
                    "TypeError: AsyncGenerator.prototype.return called on incompatible receiver"
                        .to_string(),
                )),
            }
        }))),
    );
    // Set %AsyncGeneratorPrototype%.throw — completes the generator and returns a rejected Promise
    async_generator_proto_rc.borrow_mut().set(
        "__throw",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let this_val = crate::builtins::get_native_this().ok_or_else(|| {
                JsError(
                    "AsyncGenerator.prototype.throw called on incompatible receiver".to_string(),
                )
            })?;
            match this_val {
                Value::Generator(gen) => {
                    use crate::value::generator::GeneratorState;
                    let arg = args.first().cloned().unwrap_or(Value::Undefined);
                    {
                        let mut g = gen.borrow_mut();
                        g.state = GeneratorState::Completed;
                    }
                    let proto = crate::builtins::promise::get_promise_proto();
                    crate::builtins::promise::promise_reject_impl_static(vec![arg], proto)
                }
                _ => Err(JsError(
                    "TypeError: AsyncGenerator.prototype.throw called on incompatible receiver"
                        .to_string(),
                )),
            }
        }))),
    );
    // Set %AsyncGeneratorPrototype%[@@toStringTag] = "AsyncGenerator"
    async_generator_proto_rc.borrow_mut().set(
        "Symbol.toStringTag",
        Value::String("AsyncGenerator".to_string()),
    );

    let function_constructor = make_function_constructor(
        function_proto.clone(),
        Rc::clone(ctx.env()),
        FunctionCtorKind::Ordinary,
    );
    function_constructor.set_name("Function");
    let func_ctor = Value::NativeConstructor(Rc::new(function_constructor));
    // Set Function.prototype.constructor = Function
    function_proto.borrow_mut().define(
        "constructor",
        func_ctor.clone(),
        crate::value::PropertyFlags {
            value: Some(func_ctor.clone()),
            writable: false,
            enumerable: false,
            configurable: true,
        },
    );
    ctx.set_global("Function".to_string(), func_ctor);

    // Register AsyncFunction, GeneratorFunction, AsyncGeneratorFunction
    // as native constructors that delegate to the Function constructor logic.
    let async_func_ctor = make_function_constructor(
        Rc::clone(&async_function_proto_rc),
        Rc::clone(ctx.env()),
        FunctionCtorKind::Async,
    );
    async_func_ctor.set_name("AsyncFunction");
    let async_ctor_val = Value::NativeConstructor(Rc::new(async_func_ctor));
    ASYNC_FUNCTION_PROTOTYPE.with(|fp| {
        if let Some(p) = fp.borrow().as_ref() {
            p.borrow_mut().define(
                "constructor",
                async_ctor_val.clone(),
                crate::value::PropertyFlags {
                    value: Some(async_ctor_val.clone()),
                    writable: false,
                    enumerable: false,
                    configurable: true,
                },
            );
        }
    });
    ctx.set_global("AsyncFunction".to_string(), async_ctor_val);

    let gen_func_ctor = make_function_constructor(
        Rc::clone(&generator_function_proto_rc),
        Rc::clone(ctx.env()),
        FunctionCtorKind::Generator,
    );
    gen_func_ctor.set_name("GeneratorFunction");
    let gen_ctor_val = Value::NativeConstructor(Rc::new(gen_func_ctor));
    GENERATOR_FUNCTION_PROTOTYPE.with(|fp| {
        if let Some(p) = fp.borrow().as_ref() {
            p.borrow_mut().define(
                "constructor",
                gen_ctor_val.clone(),
                crate::value::PropertyFlags {
                    value: Some(gen_ctor_val.clone()),
                    writable: false,
                    enumerable: false,
                    configurable: true,
                },
            );
        }
    });
    if let Some(generator_proto) = get_generator_prototype() {
        generator_proto.borrow_mut().define(
            "constructor",
            Value::Object(Rc::clone(&generator_function_proto_rc)),
            crate::value::PropertyFlags {
                value: Some(Value::Object(Rc::clone(&generator_function_proto_rc))),
                writable: false,
                enumerable: false,
                configurable: true,
            },
        );
    }
    ctx.set_global("GeneratorFunction".to_string(), gen_ctor_val);

    let async_gen_func_ctor = make_function_constructor(
        Rc::clone(&async_generator_function_proto_rc),
        Rc::clone(ctx.env()),
        FunctionCtorKind::AsyncGenerator,
    );
    async_gen_func_ctor.set_name("AsyncGeneratorFunction");
    let async_gen_ctor_val = Value::NativeConstructor(Rc::new(async_gen_func_ctor));
    ASYNC_GENERATOR_FUNCTION_PROTOTYPE.with(|fp| {
        if let Some(p) = fp.borrow().as_ref() {
            p.borrow_mut().define(
                "constructor",
                async_gen_ctor_val.clone(),
                crate::value::PropertyFlags {
                    value: Some(async_gen_ctor_val.clone()),
                    writable: false,
                    enumerable: false,
                    configurable: true,
                },
            );
        }
    });
    ctx.set_global("AsyncGeneratorFunction".to_string(), async_gen_ctor_val);
}

fn make_function_prototype() -> Rc<RefCell<Object>> {
    let function_proto = Object::new(ObjectKind::Function);
    let function_proto_rc = Rc::new(RefCell::new(function_proto));
    function_proto_rc.borrow_mut().callable = true;

    if let Some(object_proto) = crate::builtins::get_object_prototype() {
        function_proto_rc.borrow_mut().prototype = Some(object_proto);
    }

    function_proto_rc.borrow_mut().set(
        "__toString",
        Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
            use crate::builtins::get_native_this;
            match get_native_this() {
                Some(Value::Function(f)) => Ok(Value::String(f.source_text())),
                Some(Value::NativeFunction(f)) => Ok(Value::String(format!(
                    "function {}(){{[native code]}}",
                    f.name
                ))),
                Some(Value::NativeConstructor(f)) => Ok(Value::String(format!(
                    "function {}(){{[native code]}}",
                    f.name()
                ))),
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
        "__bind",
        Value::NativeFunction(Rc::new(NativeFunction::new(proto_bind))),
    );
    if let Some(Value::Symbol(symbol)) = crate::builtins::symbol::get_has_instance_symbol() {
        let key = symbol.property_key();
        function_proto_rc.borrow_mut().set_symbol(
            &key,
            Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
                let target = crate::builtins::get_native_this().ok_or_else(|| {
                    JsError("TypeError: @@hasInstance called on non-callable".to_string())
                })?;
                if !target.is_callable() {
                    return Err(JsError(
                        "TypeError: @@hasInstance called on non-callable".to_string(),
                    ));
                }
                let value = args.first().cloned().unwrap_or(Value::Undefined);
                crate::eval::operators::eval_instanceof(&value, &target)
            }))),
        );
        if let Some(Value::NativeFunction(function)) = function_proto_rc.borrow().get(&key) {
            function.define_property(
                "name",
                Value::String("[Symbol.hasInstance]".to_string()),
                crate::value::PropertyFlags {
                    value: Some(Value::String("[Symbol.hasInstance]".to_string())),
                    writable: false,
                    enumerable: false,
                    configurable: true,
                },
            );
            function.define_property(
                "length",
                Value::Number(1.0),
                crate::value::PropertyFlags {
                    value: Some(Value::Number(1.0)),
                    writable: false,
                    enumerable: false,
                    configurable: true,
                },
            );
        }
        if let Some(flags) = function_proto_rc.borrow_mut().descriptors.get_mut(&key) {
            flags.writable = false;
            flags.enumerable = false;
        }
    }
    for (name, length) in [
        ("__toString", 0.0),
        ("call", 1.0),
        ("apply", 2.0),
        ("__bind", 1.0),
    ] {
        if let Some(Value::NativeFunction(function)) = function_proto_rc.borrow().get(name) {
            function.define_property(
                "name",
                Value::String(name.to_string()),
                crate::value::PropertyFlags {
                    value: Some(Value::String(name.to_string())),
                    writable: false,
                    enumerable: false,
                    configurable: true,
                },
            );
            function.define_property(
                "length",
                Value::Number(length),
                crate::value::PropertyFlags {
                    value: Some(Value::Number(length)),
                    writable: false,
                    enumerable: false,
                    configurable: true,
                },
            );
        }
        if let Some(flags) = function_proto_rc.borrow_mut().descriptors.get_mut(name) {
            flags.writable = false;
            flags.enumerable = false;
        }
    }
    // ES §16.1: caller/arguments accessors throw TypeError for strict/class functions
    let thrower = crate::eval::function::throw_type_error();
    if let Value::NativeFunction(function) = &thrower {
        function.define_property(
            "name",
            Value::String(String::new()),
            crate::value::PropertyFlags {
                writable: false,
                enumerable: false,
                configurable: false,
                value: Some(Value::String(String::new())),
            },
        );
        function.define_property(
            "length",
            Value::Number(0.0),
            crate::value::PropertyFlags {
                writable: false,
                enumerable: false,
                configurable: false,
                value: Some(Value::Number(0.0)),
            },
        );
        function.set_extensible(false);
    }
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

fn make_function_constructor(
    function_proto: Rc<RefCell<Object>>,
    global_env: Rc<RefCell<crate::env::Environment>>,
    kind: FunctionCtorKind,
) -> NativeConstructor {
    let generator_prototype = if matches!(kind, FunctionCtorKind::Generator) {
        function_proto.borrow().get("prototype").and_then(|value| {
            if let Value::Object(object) = value {
                Some(object)
            } else {
                None
            }
        })
    } else {
        None
    };
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
            let source = format!(
                "{}({}) {{\n{}\n}}",
                kind.parse_prefix(),
                params_src,
                body_src
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
            // Function constructor bodies are always sloppy mode per ES spec §20.2.1.1.1
            // (step 14: "Let strict be false"). The enclosing strict mode must not leak in.
            let saved_strict = crate::interpreter::is_strict_mode();
            crate::interpreter::set_strict_mode(false);
            let fc_result = match crate::parser::parse_script(&source) {
                Ok(crate::ast::Program::Script(stmts)) => {
                    if let Some(crate::ast::Statement::FunctionDeclaration {
                        name,
                        params,
                        body,
                        is_async,
                        is_generator,
                    }) = stmts.into_iter().next()
                    {
                        let (expect_async, expect_generator) = kind.expected_flags();
                        if is_async != expect_async || is_generator != expect_generator {
                            return Err(JsError::new(
                                "SyntaxError: Function constructor produced no function",
                            ));
                        }
                        let mut func = ValueFunction::new(
                            Some(name),
                            params,
                            body,
                            Rc::clone(&global_env),
                            is_async,
                            is_generator,
                        );
                        if matches!(
                            kind,
                            FunctionCtorKind::Generator | FunctionCtorKind::AsyncGenerator
                        ) {
                            func.set_empty_prototype(true);
                        }
                        if let Some(ref prototype) = generator_prototype {
                            func.set_generator_prototype(Rc::clone(prototype));
                        }
                        if let Some(Value::Object(this_obj)) = crate::builtins::get_native_this() {
                            if let Some(sub_proto) = this_obj.borrow().prototype.clone() {
                                func.set_instance_proto(sub_proto);
                            }
                        }
                        Ok(Value::Function(func))
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
            };
            crate::interpreter::set_strict_mode(saved_strict);
            fc_result
        },
        function_proto,
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn generator_function_constructor_has_standard_length_descriptor() {
        let mut context = crate::Context::new().unwrap();
        let value = context
            .eval("var C = Object.getPrototypeOf(function*() {}).constructor; var d = Object.getOwnPropertyDescriptor(C, 'length'); [d.value, d.writable, d.enumerable, d.configurable].join('|')")
            .unwrap();
        assert_eq!(value, crate::Value::String("1|false|false|true".into()));
    }

    #[test]
    fn generator_function_length_delete_and_restore_tracks_configurability() {
        let mut context = crate::Context::new().unwrap();
        let value = context
            .eval("var C = Object.getPrototypeOf(function*() {}).constructor; var deleted = delete C.length; var absent = Object.getOwnPropertyDescriptor(C, 'length') === undefined; Object.defineProperty(C, 'length', {value: 1, writable: false, enumerable: false, configurable: true}); [deleted, absent, C.length].join('|')")
            .unwrap();
        assert_eq!(value, crate::Value::String("true|true|1".into()));
    }

    #[test]
    fn generator_function_name_is_configurable() {
        let mut context = crate::Context::new().unwrap();
        let value = context
            .eval("var C = Object.getPrototypeOf(function*() {}).constructor; var d = Object.getOwnPropertyDescriptor(C, 'name'); [d.value, d.writable, d.enumerable, d.configurable].join('|')")
            .unwrap();
        assert_eq!(
            value,
            crate::Value::String("GeneratorFunction|false|false|true".into())
        );
    }

    #[test]
    fn generator_function_prototype_descriptor_is_non_configurable() {
        let mut context = crate::Context::new().unwrap();
        let value = context
            .eval("Object.getOwnPropertyDescriptor(function*() {}, 'prototype').configurable")
            .unwrap();
        assert_eq!(value, crate::Value::Boolean(false));
    }

    #[test]
    fn generator_function_prototype_prototype_is_non_writable() {
        let mut context = crate::Context::new().unwrap();
        let value = context
            .eval("var d = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(function*() {}), 'prototype'); [d.value === Object.getPrototypeOf(function*() {}.prototype), d.writable, d.enumerable, d.configurable].join('|')")
            .unwrap();
        assert_eq!(value, crate::Value::String("true|false|false|true".into()));
    }

    #[test]
    fn generator_function_constructor_uses_constructor_realm_generator_prototype() {
        let mut constructor_realm = crate::Context::new().unwrap();
        let generator_constructor = constructor_realm
            .eval("Object.getPrototypeOf(function*() {}).constructor")
            .unwrap();
        let constructor_generator_prototype = constructor_realm
            .eval("Object.getPrototypeOf(function*() {}.prototype)")
            .unwrap();
        let mut new_target_realm = crate::Context::new().unwrap();
        let new_target = new_target_realm
            .eval("var nt = new Function(); nt.prototype = null; nt")
            .unwrap();
        let new_target_generator_function_prototype = new_target_realm
            .eval("Object.getPrototypeOf(function*() {})")
            .unwrap();
        new_target_realm.set_global("generatorConstructor".into(), generator_constructor);
        new_target_realm.set_global("constructorGeneratorPrototype".into(), constructor_generator_prototype);
        new_target_realm.set_global("newTarget".into(), new_target);
        new_target_realm.set_global("newTargetGeneratorFunctionPrototype".into(), new_target_generator_function_prototype);
        let value = new_target_realm
            .eval("var fn = Reflect.construct(generatorConstructor, [''], newTarget); [Object.getPrototypeOf(fn) === newTargetGeneratorFunctionPrototype, Object.getPrototypeOf(fn.prototype) === constructorGeneratorPrototype].join('|')")
            .unwrap();
        assert_eq!(value, crate::Value::String("true|true".into()));
    }

    #[test]
    fn generator_prototype_has_symbol_to_string_tag() {
        let mut context = crate::Context::new().unwrap();
        let value = context
            .eval("Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Object.getPrototypeOf(function*() {}())), Symbol.toStringTag)")
            .unwrap();
        assert!(matches!(value, crate::Value::Object(_)));
    }

    #[test]
    fn generator_method_owns_a_generator_prototype() {
        let mut context = crate::Context::new().unwrap();
        let value = context
            .eval("var method = { *method() {} }.method; Object.getPrototypeOf(method.prototype) === Object.getPrototypeOf(function*() {}).prototype")
            .unwrap();
        assert_eq!(value, crate::Value::Boolean(true));
    }

    #[test]
    fn async_generator_method_owns_an_async_generator_prototype() {
        let mut context = crate::Context::new().unwrap();
        let value = context
            .eval("var method = { async *method() {} }.method; Object.getPrototypeOf(method.prototype) === Object.getPrototypeOf(async function*() {}).prototype")
            .unwrap();
        assert_eq!(value, crate::Value::Boolean(true));
    }

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
    fn test_function_constructor_strict_with_statement_throws() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("var f = Function(\"'use strict'; with ({}) {}\")");
        assert!(
            result.is_err(),
            "strict with in Function constructor body must throw"
        );
    }

    #[test]
    fn test_function_constructor_strict_with_in_nested_function_throws() {
        let mut ctx = Context::new().unwrap();
        let result = ctx
            .eval("Function(\"'use strict'; var f1 = function () { var o = {}; with (o) {}; }\");");
        assert!(
            result.is_err(),
            "strict with in nested function expression must throw"
        );
    }

    #[test]
    fn test_function_constructor_immediate_call() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("Function('a', 'return a')(3)").unwrap();
        assert_eq!(result, Value::Number(3.0));
    }

    #[test]
    fn function_prototype_invocation_returns_undefined() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(ctx.eval("Function.prototype()"), Ok(Value::Undefined));
    }

    #[test]
    fn repeated_function_prototype_calls_do_not_exhaust_call_depth() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("for (var i = 0; i < 10001; i++) Function.prototype(); 'ok'"),
            Ok(Value::String("ok".to_string()))
        );
    }

    #[test]
    fn user_function_is_instanceof_function_after_array_map_call() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval(
                "var f=function(a,b){}; f[0]=1; f[1]=2; \
                 Array.prototype.map.call(f, function(_,_,o){return o instanceof Function;})[0]"
            ),
            Ok(Value::Boolean(true))
        );
    }

    #[test]
    fn function_apply_and_bind_reject_non_callable_targets() {
        let mut ctx = Context::new().unwrap();
        assert!(ctx.eval("Function.prototype.apply.call({}, null)").is_err());
        assert!(ctx.eval("Function.prototype.bind.call({}, null)").is_err());
    }

    #[test]
    fn function_prototype_has_instance_is_callable() {
        let mut ctx = Context::new().unwrap();
        let value = ctx
            .eval("function F() {}; var o = new F(); var d = Object.getOwnPropertyDescriptor(Function.prototype, Symbol.hasInstance); [F[Symbol.hasInstance](o), F[Symbol.hasInstance]({}), d.value.name, d.value.length, d.writable, d.enumerable, d.configurable].join('|')")
            .unwrap();
        assert_eq!(
            value,
            Value::String("true|false|[Symbol.hasInstance]|1|false|false|true".into())
        );
    }

    #[test]
    fn native_function_to_string_uses_native_function_grammar() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("Function.prototype.toString.call(Array)"),
            Ok(Value::String("function Array(){[native code]}".to_string()))
        );
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
