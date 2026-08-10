pub(crate) fn current_global_object() -> Value {
    GLOBAL_OBJECT
        .with(|global| {
            global
                .borrow()
                .as_ref()
                .map(|object| Value::Object(object.clone()))
        })
        .unwrap_or(Value::Undefined)
}

pub(crate) fn is_global_object(value: &Value) -> bool {
    let Value::Object(object) = value else {
        return false;
    };
    GLOBAL_OBJECT.with(|global| {
        global
            .borrow()
            .as_ref()
            .is_some_and(|global| Rc::ptr_eq(global, object))
    })
}

pub(crate) fn synchronize_global_object(registers: &mut Vec<Value>, old: &Value, new: &Value) {
    let (Value::Object(old_object), Value::Object(new_object)) = (old, new) else {
        return;
    };
    let is_global = GLOBAL_OBJECT.with(|global| {
        global
            .borrow()
            .as_ref()
            .is_some_and(|object| Rc::ptr_eq(object, old_object))
    });
    if !is_global {
        return;
    }
    GLOBAL_OBJECT.with(|global| global.replace(Some(new_object.clone())));
    crate::locals::current().replace_value(old, new);
    for register in registers {
        if let Value::Object(object) = register {
            if Rc::ptr_eq(object, old_object) {
                *object = new_object.clone();
            }
        }
    }
}
