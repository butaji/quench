use std::{cell::RefCell, rc::Rc};

use crate::value::{ObjectAliasValue, ObjectData, PrivateSlot, PrivateSlots, Value, WeakObject};

pub(crate) fn set(properties: Rc<ObjectData>, key: &str, value: Value) -> Value {
    let self_reference = value_targets(&value, &properties);
    if self_reference {
        let parent_alias = alias(&properties);
        let parent = unsafe { &mut *(Rc::as_ptr(&properties) as *mut ObjectData) };
        let index = { parent.properties.iter().rposition(|(name, _)| name == key) };
        if let Some(index) = index {
            if let Some((_, mut current)) = parent.properties.iter_mut().nth(index) {
                *current = parent_alias;
            }
        } else {
            parent.properties.push((key.into(), parent_alias));
        }
        parent.ensure_creation_order();
        record_created(&mut parent.created, key);
        return Value::Object(properties);
    }
    if plain_index_write(&properties, key) {
        // Ordinary objects are identity-bearing mutable records. For a plain
        // indexed data write, mutate the canonical record in place instead of
        // retaining one full COW snapshot per sequential assignment.
        let object = unsafe { &mut *(Rc::as_ptr(&properties) as *mut ObjectData) };
        object.set_property_in_place(key, value);
        return Value::Object(properties);
    }
    if plain_named_write(&properties, key) {
        // The same identity rule applies to ordinary named data writes.  The
        // old COW path cloned and retargeted the complete property vector for
        // every new key, turning a sequence of distinct writes into O(n^2)
        // work.  Prototype/accessor/metadata cases stay on the complete COW
        // path below, and self-referential values were handled above.
        let object = unsafe { &mut *(Rc::as_ptr(&properties) as *mut ObjectData) };
        object.set_property_in_place(key, value);
        return Value::Object(properties);
    }
    crate::execution_trace::kernel("object_alias_rebuild", false);
    let object = Rc::new_cyclic(|weak| {
        let mut values = properties.properties.clone();
        // A subsequent define/set resurrects a property removed through the
        // COW object path. Remove the deletion marker before rebuilding the
        // property so own-property and descriptor lookups see the new state.
        let deleted = super::deleted_key(key);
        values.retain(|(name, _)| name != &deleted);
        for (name, mut value) in values.iter_mut() {
            if name == crate::intl::SLOT {
                continue;
            }
            if super::is_descriptor_key(name) {
                retarget_descriptor(&mut value, &properties, weak);
            } else {
                retarget(&mut value, &properties, weak);
            }
        }
        retarget_private_slots(&properties.private_slots, &properties, weak);
        let mut value = value.clone();
        retarget(&mut value, &properties, weak);
        if let Some(slot) = values.position_rev(key) {
            values.store_slot(slot, value);
        } else {
            values.push((key.into(), value));
        }
        sync_descriptor_value(&mut values, key);
        reattach_function_homes(&values, weak);
        let mut created = properties.creation_order_values();
        record_created(&mut created, key);
        ObjectData::with_creation_order(values, Rc::clone(&properties.private_slots), created)
    });
    Value::Object(object)
}

/// Admit only ordinary named data writes whose complete semantics have already
/// been resolved by the property setter.  Special objects and metadata remain
/// on the replacement path, which preserves prototype/accessor and alias
/// behavior without making the common object-literal write quadratic.
pub(crate) fn plain_named_write(properties: &Rc<ObjectData>, key: &str) -> bool {
    !key.starts_with('\0')
        && crate::arrays::array_index(key).is_none()
        && properties.original_prototype().is_none_or(|prototype| {
            matches!(
                prototype,
                Value::Builtin(crate::ops::Builtin::ObjectPrototype)
            )
        })
        && properties.has_default_internal_prototype()
        && properties.is_fast_extensible()
        && !properties.has_replacement()
        && crate::builtins::descriptor_metadata(properties.as_ref(), key).is_none()
}

pub(crate) fn plain_index_write(properties: &Rc<ObjectData>, key: &str) -> bool {
    crate::arrays::array_index(key).is_some()
        && properties.original_prototype().is_none_or(|prototype| {
            matches!(
                prototype,
                Value::Builtin(crate::ops::Builtin::ObjectPrototype)
            )
        })
        && properties.has_default_internal_prototype()
        && properties.is_fast_extensible()
        && !properties.has_replacement()
        && crate::builtins::descriptor_metadata(properties.as_ref(), key).is_none()
}

pub(crate) fn set_plain_index_number(
    properties: &Rc<ObjectData>,
    index: usize,
    value: f64,
) -> bool {
    let key = index.to_string();
    if !plain_index_write(properties, &key) {
        return false;
    }
    let object = unsafe { &mut *(Rc::as_ptr(properties) as *mut ObjectData) };
    object.set_property_in_place(&key, Value::Number(value));
    true
}

fn value_targets(value: &Value, object: &Rc<ObjectData>) -> bool {
    match value {
        Value::Object(value) => Rc::ptr_eq(value, object),
        Value::ObjectAlias(alias) => alias
            .target()
            .is_some_and(|value| Rc::ptr_eq(&value, object)),
        Value::BindingCell(cell) => value_targets(&cell.borrow(), object),
        _ => false,
    }
}

pub(crate) fn record_created(created: &mut Vec<crate::value::PropertyName>, key: &str) {
    if key.starts_with('\0') || created.iter().any(|name| name == key) {
        return;
    }
    created.push(key.into());
}

fn sync_descriptor_value(values: &mut crate::value::ObjectProperties, key: &str) {
    let value = values
        .iter()
        .rev()
        .find_map(|(name, value)| (name == key).then(|| value.clone()));
    let descriptor_key = super::descriptor_key(key);
    let Some(slot) = values.position_rev(&descriptor_key) else {
        return;
    };
    let Some(Value::Object(mut descriptor)) = values.slot_value(slot) else {
        return;
    };
    if let Some(field_slot) = descriptor.properties.position_rev("value") {
        Rc::make_mut(&mut descriptor)
            .properties
            .store_slot(field_slot, value.unwrap_or(Value::Undefined));
    }
    values.store_slot(slot, Value::Object(descriptor));
}

/// Re-anchor `\0home_object` aliases inside the clone's method values so `super`
/// always resolves to the live prototype rather than a stale clone.
fn reattach_function_homes(values: &crate::value::ObjectProperties, new_home: &WeakObject) {
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
fn retarget_descriptor(value: &mut Value, old: &Rc<ObjectData>, new: &WeakObject) {
    let Value::Object(properties) = value else {
        return;
    };
    for (name, mut field) in Rc::make_mut(properties).properties.iter_mut() {
        if !matches!(name.as_str(), "value" | "get" | "set") {
            continue;
        }
        retarget(&mut field, old, new);
    }
}

fn retarget(value: &mut Value, old: &Rc<ObjectData>, new: &WeakObject) {
    let targets_old = match value {
        Value::Object(object) if Rc::ptr_eq(object, old) => true,
        Value::Object(_) => false,
        Value::ObjectAlias(alias) => alias
            .target()
            .is_some_and(|object| Rc::ptr_eq(&object, old)),
        Value::BindingCell(cell) => {
            // Binding cells can be layered (notably for accessor metadata and
            // symbol-keyed properties). Propagate the COW home through every
            // layer rather than only rewriting a directly-held object.
            retarget(&mut cell.borrow_mut(), old, new);
            false
        }
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
