use std::{cell::RefCell, rc::Rc};

use crate::value::Value;

/// Shared indexed lexical bindings. Captured prefixes share their slot cells.
#[derive(Debug, Default, PartialEq)]
pub struct Environment {
    slots: RefCell<Vec<Rc<RefCell<Value>>>>,
    caller: Option<Rc<Self>>,
}

impl Environment {
    pub(crate) fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    pub(crate) fn capture(environment: &Rc<Self>, count: u16) -> Rc<Self> {
        let count = usize::from(count);
        let mut source = environment.slots.borrow_mut();
        while source.len() < count {
            source.push(Rc::new(RefCell::new(Value::Undefined)));
        }
        let slots = source.iter().take(count).cloned().collect();
        Rc::new(Self {
            slots: RefCell::new(slots),
            caller: None,
        })
    }

    pub(crate) fn child(captures: &Rc<Self>, values: Vec<Value>) -> Rc<Self> {
        let mut slots = captures.slots.borrow().clone();
        slots.extend(values.into_iter().map(|value| Rc::new(RefCell::new(value))));
        Rc::new(Self {
            slots: RefCell::new(slots),
            caller: crate::locals::is_installed().then(crate::locals::current),
        })
    }

    pub(crate) fn len(&self) -> usize {
        self.slots.borrow().len()
    }

    pub(crate) fn get(&self, slot: u16) -> Value {
        self.slots
            .borrow()
            .get(usize::from(slot))
            .map_or(Value::Undefined, |value| value.borrow().clone())
    }

    pub(crate) fn set(&self, slot: u16, value: Value) {
        let index = usize::from(slot);
        let mut slots = self.slots.borrow_mut();
        while slots.len() <= index {
            slots.push(Rc::new(RefCell::new(Value::Undefined)));
        }
        *slots[index].borrow_mut() = value;
    }

    pub(crate) fn slot(&self, slot: u16) -> Option<Rc<RefCell<Value>>> {
        self.slots.borrow().get(usize::from(slot)).cloned()
    }

    pub(crate) fn replace_slot(&self, slot: u16, value: Value) -> Rc<RefCell<Value>> {
        let index = usize::from(slot);
        let mut slots = self.slots.borrow_mut();
        while slots.len() <= index {
            slots.push(Rc::new(RefCell::new(Value::Undefined)));
        }
        std::mem::replace(&mut slots[index], Rc::new(RefCell::new(value)))
    }

    pub(crate) fn restore_slot(&self, slot: u16, value: Rc<RefCell<Value>>) {
        self.slots.borrow_mut()[usize::from(slot)] = value;
    }

    pub(crate) fn replace_value(&self, old: &Value, new: &Value) {
        if let Some(caller) = &self.caller {
            caller.replace_value(old, new);
        }
        for slot in self.slots.borrow().iter() {
            let mut value = slot.borrow_mut();
            if same_identity(&value, old) {
                *value = new.clone();
                replace_nested(&mut value, old, new);
            } else {
                replace_nested(&mut value, old, new);
            }
        }
    }
}

fn replace_nested(value: &mut Value, old: &Value, new: &Value) {
    if !contains_nested(value, old) {
        return;
    }
    let values = match value {
        Value::Array(values) => Rc::make_mut(values),
        Value::Object(values) => {
            for (_, value) in values.iter() {
                retarget_nested_alias(value, old, new);
            }
            return;
        }
        _ => return,
    };
    for value in values.values_mut() {
        replace_alias(value, old, new);
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
            for value in values.iter() {
                retarget_nested_alias(value, old, new);
            }
        }
        Value::Object(values) => {
            for (_, value) in values.iter() {
                retarget_nested_alias(value, old, new);
            }
        }
        _ => {}
    }
}

fn contains_nested(value: &Value, target: &Value) -> bool {
    match value {
        Value::ObjectAlias(alias) => alias_targets(alias, target),
        Value::Array(values) => values
            .iter()
            .any(|value| same_identity(value, target) || contains_nested(value, target)),
        Value::Object(values) => values
            .iter()
            .any(|(_, value)| same_identity(value, target) || contains_nested(value, target)),
        _ => false,
    }
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
        .is_some_and(|object| Rc::ptr_eq(&object, target))
}

fn same_identity(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Object(left), Value::Object(right)) => Rc::ptr_eq(left, right),
        (Value::Array(left), Value::Array(right)) => Rc::ptr_eq(left, right),
        (Value::Map(left), Value::Map(right)) => Rc::ptr_eq(left, right),
        (Value::Set(left), Value::Set(right)) => Rc::ptr_eq(left, right),
        _ => false,
    }
}
