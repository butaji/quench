use std::{cell::RefCell, rc::Rc};

use crate::value::{ObjectAliasValue, Value};

pub(crate) fn set(properties: Rc<Vec<(String, Value)>>, key: &str, value: Value) -> Value {
    let object = Rc::new_cyclic(|weak| {
        let mut values = (*properties).clone();
        for (name, value) in &mut values {
            if !super::is_descriptor_key(name) {
                retarget(value, &properties, weak);
            }
        }
        let mut value = value.clone();
        retarget(&mut value, &properties, weak);
        if let Some((_, current)) = values.iter_mut().rev().find(|(name, _)| name == key) {
            *current = value;
        } else {
            values.push((key.to_string(), value));
        }
        values
    });
    Value::Object(object)
}

fn retarget(
    value: &mut Value,
    old: &Rc<Vec<(String, Value)>>,
    new: &std::rc::Weak<Vec<(String, Value)>>,
) {
    let targets_old = match value {
        Value::Object(object) if Rc::ptr_eq(object, old) => true,
        Value::Object(object) => {
            *value = alias(object);
            false
        }
        Value::ObjectAlias(alias) => alias
            .0
            .borrow()
            .upgrade()
            .is_some_and(|object| Rc::ptr_eq(&object, old)),
        _ => false,
    };
    if targets_old {
        *value = Value::ObjectAlias(ObjectAliasValue(Rc::new(RefCell::new(new.clone()))));
    }
}

fn alias(object: &Rc<Vec<(String, Value)>>) -> Value {
    Value::ObjectAlias(ObjectAliasValue(Rc::new(RefCell::new(Rc::downgrade(
        object,
    )))))
}
