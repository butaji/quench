fn replace_nested(value: &mut Value, old: &Value, new: &Value) {
    if !contains_nested(value, old) {
        return;
    }
    let values = match value {
        Value::BindingCell(cell) => {
            replace_alias(&mut cell.borrow_mut(), old, new);
            return;
        }
        Value::Array(values) => Rc::make_mut(values),
        Value::Object(values) => {
            for (_, value) in values.iter() {
                retarget_nested_alias(&value, old, new);
            }
            retarget_private_aliases(&values.private_slots, old, new);
            return;
        }
        Value::Function(function) => {
            for (_, value) in function.properties.borrow_mut().iter_mut() {
                replace_direct(value, old, new);
            }
            replace_private_slots(&function.private_slots, old, new);
            return;
        }
        _ => return,
    };
    for value in values.values_mut() {
        replace_alias(value, old, new);
    }
}

/// Retarget only aliases directly stored by a newly cloned object.  This is
/// intentionally shallow: recursive replacement walks can revisit a
/// self-referential object graph, while copy-on-write transitions only need
/// to repair the clone's own weak self-aliases.
pub(crate) fn retarget_aliases_for_identity(
    object: &std::rc::Rc<crate::value::ObjectData>,
    identity: u64,
) {
    for (_, value) in object.iter() {
        if let Value::ObjectAlias(alias) = value {
            if alias.target().is_some_and(|target| target.identity() == identity) {
                *alias.0.borrow_mut() = std::rc::Rc::downgrade(object);
            }
        }
    }
}

fn retarget_nested_alias(value: &Value, old: &Value, new: &Value) {
    match value {
        Value::ObjectAlias(alias) if alias_targets(alias, old) => {
            if let Value::Object(object) = new {
                *alias.0.borrow_mut() = Rc::downgrade(object);
            }
        }
        Value::Array(values) => {
            for value in values.snapshot() {
                retarget_nested_alias(&value, old, new);
            }
        }
        Value::Object(values) => {
            for (_, value) in values.iter() {
                retarget_nested_alias(&value, old, new);
            }
            retarget_private_aliases(&values.private_slots, old, new);
        }
        Value::Function(function) => retarget_private_aliases(&function.private_slots, old, new),
        _ => {}
    }
}

fn contains_nested(value: &Value, target: &Value) -> bool {
    match value {
        Value::BindingCell(cell) => contains_nested(&cell.borrow(), target),
        Value::ObjectAlias(alias) => alias_targets(alias, target),
        Value::Array(values) => values
            .snapshot()
            .iter()
            .any(|value| same_identity(value, target) || contains_nested(value, target)),
        Value::Object(values) => {
            values
                .iter()
                .any(|(_, value)| same_identity(&value, target) || contains_nested(&value, target))
                || private_slots_contain(&values.private_slots, target)
        }
        Value::Function(function) => {
            function.properties.borrow().iter().any(|(_, value)| {
                same_identity(value, target) || alias_targets_value(value, target)
            }) || private_slots_contain(&function.private_slots, target)
        }
        _ => false,
    }
}

fn retarget_private_aliases(slots: &crate::value::PrivateSlots, old: &Value, new: &Value) {
    for (_, slot) in slots.borrow().iter() {
        retarget_private_slot(slot, old, new);
    }
}

fn retarget_private_slot(slot: &crate::value::PrivateSlot, old: &Value, new: &Value) {
    match slot {
        crate::value::PrivateSlot::Data(value) | crate::value::PrivateSlot::Method(value) => {
            retarget_nested_alias(value, old, new)
        }
        crate::value::PrivateSlot::Accessor { get, set } => {
            get.as_ref()
                .iter()
                .for_each(|value| retarget_nested_alias(value, old, new));
            set.as_ref()
                .iter()
                .for_each(|value| retarget_nested_alias(value, old, new));
        }
    }
}

fn replace_private_slots(slots: &crate::value::PrivateSlots, old: &Value, new: &Value) {
    for (_, slot) in slots.borrow_mut().iter_mut() {
        replace_private_slot(slot, old, new);
    }
}

fn replace_private_slot(slot: &mut crate::value::PrivateSlot, old: &Value, new: &Value) {
    match slot {
        crate::value::PrivateSlot::Data(value) | crate::value::PrivateSlot::Method(value) => {
            replace_alias(value, old, new)
        }
        crate::value::PrivateSlot::Accessor { get, set } => {
            get.as_mut()
                .iter_mut()
                .for_each(|value| replace_alias(value, old, new));
            set.as_mut()
                .iter_mut()
                .for_each(|value| replace_alias(value, old, new));
        }
    }
}

fn private_slots_contain(slots: &crate::value::PrivateSlots, target: &Value) -> bool {
    slots
        .borrow()
        .iter()
        .any(|(_, slot)| private_slot_contains(slot, target))
}

fn private_slot_contains(slot: &crate::value::PrivateSlot, target: &Value) -> bool {
    match slot {
        crate::value::PrivateSlot::Data(value) | crate::value::PrivateSlot::Method(value) => {
            private_value_contains(value, target)
        }
        crate::value::PrivateSlot::Accessor { get, set } => {
            get.as_ref()
                .is_some_and(|value| private_value_contains(value, target))
                || set
                    .as_ref()
                    .is_some_and(|value| private_value_contains(value, target))
        }
    }
}

fn private_value_contains(value: &Value, target: &Value) -> bool {
    same_identity(value, target) || contains_nested(value, target)
}

fn replace_direct(value: &mut Value, old: &Value, new: &Value) {
    if let Value::ObjectAlias(alias) = value {
        if alias_targets(alias, old) {
            if let Value::Object(object) = new {
                *alias.0.borrow_mut() = Rc::downgrade(object);
            }
        }
    } else if same_identity(value, old) {
        *value = new.clone();
    }
}

fn alias_targets_value(value: &Value, target: &Value) -> bool {
    matches!(value, Value::ObjectAlias(alias) if alias_targets(alias, target))
}

fn replace_alias(value: &mut Value, old: &Value, new: &Value) {
    if let Value::ObjectAlias(alias) = value {
        if alias_targets(alias, old) {
            if let Value::Object(object) = new {
                *alias.0.borrow_mut() = Rc::downgrade(object);
                return;
            }
        }
    }
    if same_identity(value, old) {
        *value = new.clone();
    } else {
        replace_nested(value, old, new);
    }
}

fn alias_targets(alias: &crate::value::ObjectAliasValue, target: &Value) -> bool {
    let Value::Object(target) = target else {
        return false;
    };
    alias
        .0
        .borrow()
        .upgrade()
        .is_some_and(|object| object.identity() == target.identity())
}

fn same_identity(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Object(left), Value::Object(right)) => left.identity() == right.identity(),
        (Value::Array(left), Value::Array(right)) => Rc::ptr_eq(left, right),
        (Value::Map(left), Value::Map(right)) => Rc::ptr_eq(left, right),
        (Value::Set(left), Value::Set(right)) => Rc::ptr_eq(left, right),
        _ => false,
    }
}
