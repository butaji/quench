use std::{cell::RefCell, rc::Rc};

use crate::value::{ObjectAliasValue, ObjectData, PrivateSlot, PrivateSlots, Value, WeakObject};

pub(crate) fn set(properties: Rc<ObjectData>, key: &str, value: Value) -> Value {
    let object = Rc::new_cyclic(|weak| {
        let mut values = properties.properties.clone();
        for (name, value) in &mut values {
            if !super::is_descriptor_key(name) {
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
        ObjectData::with_private_slots(values, Rc::clone(&properties.private_slots))
    });
    Value::Object(object)
}

fn retarget_private_slots(slots: &PrivateSlots, old: &Rc<ObjectData>, new: &WeakObject) {
    for (_, slot) in slots.borrow_mut().iter_mut() {
        retarget_slot(slot, old, new);
    }
}

fn retarget_slot(slot: &mut PrivateSlot, old: &Rc<ObjectData>, new: &WeakObject) {
    match slot {
        PrivateSlot::Data(value) => retarget(value, old, new),
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

fn alias(object: &Rc<ObjectData>) -> Value {
    Value::ObjectAlias(ObjectAliasValue(Rc::new(RefCell::new(Rc::downgrade(
        object,
    )))))
}
