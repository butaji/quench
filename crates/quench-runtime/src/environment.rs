use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

use crate::value::Value;

/// Shared indexed lexical bindings. Captured prefixes share their slot cells.
#[derive(Debug, Default, PartialEq)]
pub struct Environment {
    slots: RefCell<Vec<Rc<RefCell<Value>>>>,
    names: RefCell<Option<HashMap<String, Rc<RefCell<Value>>>>>,
    uninitialized: RefCell<Option<Rc<RefCell<HashSet<u16>>>>>,
    caller: Option<Rc<Self>>,
}

impl Environment {
    pub(crate) fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    /// Create a fresh declarative record whose name resolution continues in
    /// `outer` without sharing its indexed slots.
    pub(crate) fn with_outer(outer: Option<&Rc<Self>>, values: Vec<Value>) -> Rc<Self> {
        Rc::new(Self {
            slots: RefCell::new(
                values
                    .into_iter()
                    .map(|value| Rc::new(RefCell::new(value)))
                    .collect(),
            ),
            names: RefCell::new(None),
            uninitialized: RefCell::new(None),
            caller: outer.map(Rc::clone),
        })
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
            names: RefCell::new(environment.names.borrow().clone()),
            uninitialized: RefCell::new(environment.uninitialized.borrow().clone()),
            caller: None,
        })
    }

    pub(crate) fn child(captures: &Rc<Self>, values: Vec<Value>) -> Rc<Self> {
        let mut slots = captures.slots.borrow().clone();
        slots.extend(values.into_iter().map(|value| Rc::new(RefCell::new(value))));
        let caller = crate::locals::is_installed().then(crate::locals::current);
        let environment = Self::with_outer(caller.as_ref(), Vec::new());
        environment.slots.replace(slots);
        environment
            .uninitialized
            .replace(captures.uninitialized.borrow().clone());
        environment
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
        {
            let binding = self.ensure_slot(slot);
            *binding.borrow_mut() = value;
        }
        self.initialize(slot);
    }

    pub(crate) fn slot(&self, slot: u16) -> Option<Rc<RefCell<Value>>> {
        self.slots.borrow().get(usize::from(slot)).cloned()
    }

    pub(crate) fn slot_cell(&self, slot: u16) -> Rc<RefCell<Value>> {
        self.ensure_slot(slot)
    }

    pub(crate) fn install_slot_cell(&self, slot: u16, cell: Rc<RefCell<Value>>) {
        let index = usize::from(slot);
        let mut slots = self.slots.borrow_mut();
        while slots.len() <= index {
            slots.push(Rc::new(RefCell::new(Value::Undefined)));
        }
        slots[index] = cell;
    }

    pub(crate) fn alias_name(&self, name: &str, slot: u16) {
        let binding = self.ensure_slot(slot);
        self.insert_alias(name, binding);
    }

    pub(crate) fn has_own_name(&self, name: &str) -> bool {
        self.names
            .borrow()
            .as_ref()
            .is_some_and(|names| names.contains_key(name))
    }

    pub(crate) fn alias_caller_name(&self, name: &str, slot: u16) -> bool {
        let Some(caller) = &self.caller else {
            return false;
        };
        let binding = self.ensure_slot(slot);
        caller.insert_alias(name, binding);
        true
    }

    pub(crate) fn delete_caller_name(&self, name: &str, slot: u16) -> bool {
        let (Some(caller), Some(binding)) = (&self.caller, self.slot(slot)) else {
            return false;
        };
        let removed = caller.remove_own_alias(name, &binding);
        if removed {
            self.shared_tdz().borrow_mut().insert(slot);
        }
        removed
    }

    fn insert_alias(&self, name: &str, binding: Rc<RefCell<Value>>) {
        self.names
            .borrow_mut()
            .get_or_insert_with(HashMap::new)
            .insert(name.to_string(), binding);
        self.shared_tdz();
    }

    pub(crate) fn resolve_name(&self, name: &str) -> Option<Value> {
        self.named_binding(name)
            .map(|binding| binding.borrow().clone())
    }

    pub(crate) fn set_named(&self, name: &str, value: Value) -> bool {
        let binding = self
            .names
            .borrow()
            .as_ref()
            .and_then(|names| names.get(name).cloned());
        if let Some(binding) = binding {
            *binding.borrow_mut() = value;
            self.initialize_binding(&binding);
            return true;
        }
        self.caller
            .as_ref()
            .is_some_and(|caller| caller.set_named(name, value))
    }

    pub(crate) fn delete_named(&self, name: &str, slot: u16) -> bool {
        let Some(slot_binding) = self.slot(slot) else {
            return false;
        };
        let removed = self.remove_own_alias(name, &slot_binding);
        if removed {
            self.shared_tdz().borrow_mut().insert(slot);
        }
        removed
    }

    pub(crate) fn mark_uninitialized(&self, slot: u16) {
        self.ensure_slot(slot);
        self.writable_tdz().borrow_mut().insert(slot);
    }

    pub(crate) fn is_uninitialized(&self, slot: u16) -> bool {
        self.uninitialized
            .borrow()
            .as_ref()
            .is_some_and(|slots| slots.borrow().contains(&slot))
    }

    fn named_binding(&self, name: &str) -> Option<Rc<RefCell<Value>>> {
        let binding = self
            .names
            .borrow()
            .as_ref()
            .and_then(|names| names.get(name).cloned());
        binding.or_else(|| self.caller.as_ref()?.named_binding(name))
    }

    fn ensure_slot(&self, slot: u16) -> Rc<RefCell<Value>> {
        let index = usize::from(slot);
        let mut slots = self.slots.borrow_mut();
        while slots.len() <= index {
            slots.push(Rc::new(RefCell::new(Value::Undefined)));
        }
        slots[index].clone()
    }

    fn initialize(&self, slot: u16) {
        if let Some(slots) = self.uninitialized.borrow_mut().as_mut() {
            slots.borrow_mut().remove(&slot);
        }
    }

    fn writable_tdz(&self) -> Rc<RefCell<HashSet<u16>>> {
        let mut state = self.uninitialized.borrow_mut();
        if let Some(slots) = state.as_ref() {
            if Rc::strong_count(slots) == 1 {
                return Rc::clone(slots);
            }
            let detached = Rc::new(RefCell::new(slots.borrow().clone()));
            *state = Some(Rc::clone(&detached));
            return detached;
        }
        let slots = Rc::new(RefCell::new(HashSet::new()));
        *state = Some(Rc::clone(&slots));
        slots
    }

    fn shared_tdz(&self) -> Rc<RefCell<HashSet<u16>>> {
        let mut state = self.uninitialized.borrow_mut();
        let slots = state.get_or_insert_with(|| Rc::new(RefCell::new(HashSet::new())));
        Rc::clone(slots)
    }

    fn remove_own_alias(&self, name: &str, slot: &Rc<RefCell<Value>>) -> bool {
        let mut names = self.names.borrow_mut();
        let Some(bindings) = names.as_mut() else {
            return false;
        };
        let matches = bindings
            .get(name)
            .is_some_and(|binding| Rc::ptr_eq(binding, slot));
        if matches {
            bindings.remove(name);
        }
        if bindings.is_empty() {
            *names = None;
        }
        matches
    }

    fn initialize_binding(&self, binding: &Rc<RefCell<Value>>) {
        let slot = self
            .slots
            .borrow()
            .iter()
            .position(|candidate| Rc::ptr_eq(candidate, binding));
        if let Some(slot) = slot.and_then(|slot| u16::try_from(slot).ok()) {
            self.initialize(slot);
        }
    }

    pub(crate) fn replace_slot(&self, slot: u16, value: Value) -> Rc<RefCell<Value>> {
        let index = usize::from(slot);
        let mut slots = self.slots.borrow_mut();
        while slots.len() <= index {
            slots.push(Rc::new(RefCell::new(Value::Undefined)));
        }
        let previous = std::mem::replace(&mut slots[index], Rc::new(RefCell::new(value)));
        drop(slots);
        self.initialize(slot);
        previous
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
