//! Function call handling for the stack machine.

use std::rc::Rc;
use std::cell::RefCell;

use crate::ast::*;
use crate::value::{Value, JsError, Object, ObjectKind, ValueFunction, SetterStorage};
use crate::env::Environment;
use crate::interpreter as hir;
use crate::stack_machine::{Machine, Frame, Work};

use super::property::{read_property, property_key_static};

/// Create the JavaScript arguments object for a function call.
pub fn create_arguments_object(f: &ValueFunction, args: Vec<Value>) -> Value {
    let mut obj = Object::new(ObjectKind::Ordinary);
    for (i, arg) in args.iter().enumerate() {
        obj.set(&i.to_string(), arg.clone());
    }
    obj.set("length", Value::Number(args.len() as f64));
    obj.set("callee", Value::Function(f.clone()));
    Value::Object(Rc::new(RefCell::new(obj)))
}

/// Call a value as a function.
pub fn call_value(
    machine: &mut Machine,
    func: Value,
    args: Vec<Value>,
    this_val: Value,
) -> Result<(), JsError> {
    match func {
        Value::Function(f) => {
            let closure = Rc::clone(&f.closure);
            let params = f.params.clone();

            let mut call_env = Environment::with_parent(Rc::clone(&closure));
            call_env.current_scope_mut().set_this(this_val);

            for (i, param) in params.iter().enumerate() {
                let arg = args.get(i).cloned().unwrap_or(Value::Undefined);
                call_env.define(param.clone(), arg);
            }

            // Create arguments object for non-arrow functions
            if !f.is_arrow {
                let args_obj = create_arguments_object(&f, args);
                call_env.define("arguments".to_string(), args_obj);
                hir::predeclare_var(&f.body, &mut call_env);
                hir::predeclare_let_const(&f.body, &mut call_env);
            }

            let call_env = Rc::new(RefCell::new(call_env));

            if f.is_arrow {
                if let Some(ref arrow_body) = *f.arrow_body.as_ref() {
                    match arrow_body {
                        ArrowBody::Expression(expr) => {
                            machine.frames.push(Frame {
                                env: call_env,
                                values: Vec::new(),
                                work: vec![Work::EvalExpr(Rc::new(expr.clone()))],
                                catches: Vec::new(),
                            });
                        }
                        ArrowBody::Block(stmts) => {
                            machine.frames.push(Frame {
                                env: call_env,
                                values: Vec::new(),
                                work: Vec::new(),
                                catches: Vec::new(),
                            });
                            machine.push_stmt_list(stmts, true);
                        }
                    }
                } else {
                    machine.current_frame().values.push(Value::Undefined);
                }
            } else {
                machine.frames.push(Frame {
                    env: call_env,
                    values: Vec::new(),
                    work: Vec::new(),
                    catches: Vec::new(),
                });
                machine.push_stmt_list(&f.body, false);
            }
            Ok(())
        }
        Value::NativeFunction(nf) => {
            hir::set_native_this(this_val);
            let result = nf.call(args)?;
            machine.current_frame().values.push(result);
            Ok(())
        }
        Value::NativeConstructor(nc) => {
            let result = nc.call(args)?;
            machine.current_frame().values.push(result);
            Ok(())
        }
        Value::Object(o) => {
            let constructor_opt = {
                let obj = o.borrow();
                if let Some(constructor) = obj.get("constructor") {
                    if matches!(constructor, Value::Function(_) | Value::NativeFunction(_) | Value::NativeConstructor(_)) {
                        Some(constructor.clone())
                    } else {
                        None
                    }
                } else {
                    None
                }
            };

            if let Some(constructor) = constructor_opt {
                let new_obj = Object::new(ObjectKind::Ordinary);
                let new_obj_rc = Rc::new(RefCell::new(new_obj));
                {
                    let proto = o.borrow().get("prototype");
                    if proto.is_some() {
                        new_obj_rc.borrow_mut().set("constructor", Value::Object(Rc::clone(&o)));
                    }
                }
                call_value(machine, constructor, args, Value::Object(Rc::clone(&new_obj_rc)))?;
            } else {
                return Err(JsError("Object is not a constructor".to_string()));
            }
            Ok(())
        }
        _ => {
            Err(JsError("Value is not a function".to_string()))
        }
    }
}

/// Call a setter function.
pub fn call_setter(
    machine: &mut Machine,
    obj: &Rc<RefCell<Object>>,
    setter_storage: &SetterStorage,
    value: Value,
) -> Result<(), JsError> {
    let closure = Rc::clone(&setter_storage.closure);

    let mut call_env = Environment::with_parent(closure);
    call_env.current_scope_mut().set_this(Value::Object(Rc::clone(obj)));
    call_env.define(setter_storage.param.clone(), value);

    let call_env = Rc::new(RefCell::new(call_env));
    machine.frames.push(Frame {
        env: call_env,
        values: Vec::new(),
        work: Vec::new(),
        catches: Vec::new(),
    });
    machine.push_stmt_list(&setter_storage.body, false);
    Ok(())
}

/// Apply a new expression.
pub fn apply_new(machine: &mut Machine, argc: usize) -> Result<(), JsError> {
    let mut args = Vec::with_capacity(argc);
    for _ in 0..argc {
        args.push(machine.pop_value());
    }
    args.reverse();
    let constructor_val = machine.pop_value();

    let actual_constructor = match &constructor_val {
        Value::Object(o) => {
            let obj = o.borrow();
            if let Some(constructor) = obj.get("constructor") {
                constructor.clone()
            } else {
                return Err(JsError("Object is not a constructor".to_string()));
            }
        }
        other => other.clone(),
    };

    let prototype: Option<Rc<RefCell<Object>>> = match &constructor_val {
        Value::Object(o) => {
            let proto = o.borrow().get("prototype");
            if let Some(Value::Object(proto_obj)) = proto {
                Some(proto_obj.clone())
            } else {
                None
            }
        }
        Value::Function(f) => Some(f.get_prototype()),
        Value::NativeConstructor(nc) => Some(Rc::clone(&nc.prototype)),
        _ => None,
    };

    let new_obj = if let Some(proto) = prototype {
        Object::with_prototype(ObjectKind::Ordinary, proto)
    } else {
        Object::new(ObjectKind::Ordinary)
    };
    let new_obj_rc = Rc::new(RefCell::new(new_obj));

    let use_constructor_result = match &constructor_val {
        Value::NativeConstructor(_) => true,
        Value::Function(f) => f.body.iter().any(Statement::has_explicit_return),
        _ => false,
    };

    machine.current_frame().work.push(Work::ApplyConstructorResult {
        new_obj: Rc::clone(&new_obj_rc),
        use_constructor_result,
    });
    call_value(machine, actual_constructor, args, Value::Object(Rc::clone(&new_obj_rc)))?;
    Ok(())
}

/// Apply constructor result handling.
pub fn apply_constructor_result(
    machine: &mut Machine,
    new_obj: Rc<RefCell<Object>>,
    use_constructor_result: bool,
) -> Result<(), JsError> {
    let result = machine.pop_value();
    if use_constructor_result && matches!(result, Value::Object(_)) {
        machine.current_frame().values.push(result);
    } else {
        machine.current_frame().values.push(Value::Object(new_obj));
    }
    Ok(())
}

/// Evaluate a callee expression for a function call.
pub fn eval_callee(machine: &mut Machine, callee: Rc<Expression>) -> Result<(), JsError> {
    match &*callee {
        Expression::Member { object, property, computed } => {
            let frame = machine.current_frame();
            frame.work.push(Work::ApplyMember { property: property.clone(), computed: *computed, callee_mode: true });
            if *computed {
                if let PropertyKey::Computed(key_expr) = property {
                    frame.work.push(Work::EvalExpr(Rc::new((**key_expr).clone())));
                } else {
                    return Err(JsError("Invalid computed property".to_string()));
                }
            }
            frame.work.push(Work::EvalExpr(Rc::new((**object).clone())));
        }
        _ => {
            let frame = machine.current_frame();
            frame.work.push(Work::PushValue(Value::Undefined));
            frame.work.push(Work::EvalExpr(callee));
        }
    }
    Ok(())
}

/// Apply a function call.
pub fn apply_call(machine: &mut Machine, argc: usize) -> Result<(), JsError> {
    let mut args = Vec::with_capacity(argc);
    for _ in 0..argc {
        args.push(machine.pop_value());
    }
    args.reverse();
    // Pop this_val first (pushed last by eval_callee), then func
    let this_val = machine.pop_value();
    let func = machine.pop_value();
    call_value(machine, func, args, this_val)
}

/// Apply member property access.
pub fn apply_member(
    machine: &mut Machine,
    property: PropertyKey,
    computed: bool,
    callee_mode: bool,
) -> Result<(), JsError> {
    use crate::value::to_js_string;

    let prop_name = if computed {
        to_js_string(&machine.pop_value())
    } else {
        property_key_static(&property)?.to_string()
    };
    let obj_val = machine.pop_value();
    let result = read_property(&obj_val, &prop_name, &machine.current_frame().env)?;

    if callee_mode {
        machine.current_frame().values.push(result);
        machine.current_frame().values.push(obj_val);
    } else {
        machine.current_frame().values.push(result);
    }
    Ok(())
}

/// Handle variable declarations in the stack machine.
pub fn var_decl(machine: &mut Machine, _kind: VarKind, name: String) -> Result<(), JsError> {
    let value = machine.pop_value();
    machine.current_frame().env.borrow_mut().initialize_declared(&name, value);
    machine.current_frame().values.push(Value::Undefined);
    Ok(())
}

/// Handle for-loop variable initialization.
pub fn for_init_var(machine: &mut Machine, _kind: VarKind, name: String) -> Result<(), JsError> {
    let value = machine.pop_value();
    machine.current_frame().env.borrow_mut().initialize_declared(&name, value);
    Ok(())
}
