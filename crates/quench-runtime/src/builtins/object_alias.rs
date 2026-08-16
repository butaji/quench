use std::{cell::RefCell, rc::Rc};

use crate::value::{ObjectAliasValue, ObjectData, PrivateSlot, PrivateSlots, Value, WeakObject};

pub(crate) fn set(properties: Rc<ObjectData>, key: &str, value: Value) -> Value {
    let object = Rc::new_cyclic(|weak| {
        let mut values = properties.properties.clone();
        for (name, value) in &mut values {
            if !super::is_descriptor_key(name) && name != crate::intl::SLOT {
                retarget(value, &properties, weak);
            }
        }
        retarget_private_slots(&properties.private_slots, &properties, weak);
        let mut value = value.clone();
        retarget(&mut value, &properties, weak);
        if let Some((_, current)) = values.iter_mut().rev().find(|(name, _)| name == key) {
            *current = value;
        } else {
            values.push((key.to_string(), value));
        }
        sync_descriptor_value(&mut values, key);
        reattach_function_homes(&values, weak);
        ObjectData::with_private_slots(values, Rc::clone(&properties.private_slots))
    });
    Value::Object(object)
}

fn sync_descriptor_value(values: &mut [(String, Value)], key: &str) {
    let value = values
        .iter()
        .rev()
        .find_map(|(name, value)| (name == key).then(|| value.clone()));
    let Some((_, Value::Object(descriptor))) = values
        .iter_mut()
        .rev()
        .find(|(name, _)| name == &super::descriptor_key(key))
    else {
        return;
    };
    if let Some((_, current)) = Rc::make_mut(descriptor)
        .iter_mut()
        .find(|(name, _)| name == "value")
    {
        *current = value.unwrap_or(Value::Undefined);
    }
}

/// Re-anchor `\0home_object` aliases inside the clone's method values so `super`
/// always resolves to the live prototype rather than a stale clone.
fn reattach_function_homes(values: &[(String, Value)], new_home: &WeakObject) {
    for (_, value) in values.iter() {
        let Value::Function(function) = value else {
            continue;
        };
        let mut properties = function.properties.borrow_mut();
        let Some((_, home)) = properties
            .iter_mut()
            .rev()
            .find(|(name, _)| name == "\0home_object")
        else {
            continue;
        };
        if matches!(home, Value::ObjectAlias(_)) {
            *home = Value::ObjectAlias(ObjectAliasValue(Rc::new(RefCell::new(new_home.clone()))));
        }
    }
}

fn retarget_private_slots(slots: &PrivateSlots, old: &Rc<ObjectData>, new: &WeakObject) {
    for (_, slot) in slots.borrow_mut().iter_mut() {
        retarget_slot(slot, old, new);
    }
}

fn retarget_slot(slot: &mut PrivateSlot, old: &Rc<ObjectData>, new: &WeakObject) {
    match slot {
        PrivateSlot::Data(value) | PrivateSlot::Method(value) => retarget(value, old, new),
        PrivateSlot::Accessor { get, set } => {
            retarget_optional(get, old, new);
            retarget_optional(set, old, new);
        }
    }
}

fn retarget_optional(value: &mut Option<Value>, old: &Rc<ObjectData>, new: &WeakObject) {
    if let Some(value) = value {
        retarget(value, old, new);
    }
}

fn retarget(value: &mut Value, old: &Rc<ObjectData>, new: &WeakObject) {
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

pub(crate) fn alias(object: &Rc<ObjectData>) -> Value {
    Value::ObjectAlias(ObjectAliasValue(Rc::new(RefCell::new(Rc::downgrade(
        object,
    )))))
}
