use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

use crate::value::Value;

#[derive(Debug, Clone, PartialEq)]
struct SlotCell(Rc<RefCell<Value>>);

impl SlotCell {
    fn new(value: Value) -> Self {
        Self(Rc::new(RefCell::new(value)))
    }

    fn load(&self) -> Value {
        self.0.borrow().clone()
    }

    fn store(&self, value: Value) {
        *self.0.borrow_mut() = value;
    }

    fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Debug, Default, PartialEq)]
struct SlotStore {
    slots: RefCell<Vec<SlotCell>>,
}

impl SlotStore {
    fn from_values(values: Vec<Value>) -> Rc<Self> {
        Rc::new(Self {
            slots: RefCell::new(values.into_iter().map(SlotCell::new).collect()),
        })
    }

    fn prefix(&self, count: usize) -> Rc<Self> {
        let mut slots = self.slots.borrow_mut();
        while slots.len() < count {
            slots.push(SlotCell::new(Value::Undefined));
        }
        Rc::new(Self {
            slots: RefCell::new(slots.iter().take(count).cloned().collect()),
        })
    }

    fn len(&self) -> usize {
        self.slots.borrow().len()
    }

    fn get(&self, slot: u16) -> Option<SlotCell> {
        self.slots.borrow().get(usize::from(slot)).cloned()
    }

    fn ensure(&self, slot: u16) -> SlotCell {
        let index = usize::from(slot);
        let mut slots = self.slots.borrow_mut();
        while slots.len() <= index {
            slots.push(SlotCell::new(Value::Undefined));
        }
        slots[index].clone()
    }

    fn replace(&self, slot: u16, cell: SlotCell) -> SlotCell {
        let index = usize::from(slot);
        let mut slots = self.slots.borrow_mut();
        while slots.len() <= index {
            slots.push(SlotCell::new(Value::Undefined));
        }
        std::mem::replace(&mut slots[index], cell)
    }
}

/// Shared indexed lexical bindings. Captured prefixes share their slot cells.
#[derive(Debug, Default, PartialEq)]
pub struct Environment {
    slots: Rc<SlotStore>,
    names: RefCell<Option<HashMap<String, SlotCell>>>,
    immutable_names: RefCell<Option<HashSet<String>>>,
    immutable_slots: RefCell<Option<HashSet<u16>>>,
    uninitialized: RefCell<Option<Rc<RefCell<HashSet<u16>>>>>,
    caller: Option<Rc<Self>>,
}

impl Environment {
    pub(crate) fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    pub(crate) fn capture(environment: &Rc<Self>, count: u16) -> Rc<Self> {
        let count = usize::from(count);
        let slots = environment.slots.prefix(count);
        Rc::new(Self {
            slots,
            names: RefCell::new(environment.names.borrow().clone()),
            immutable_names: RefCell::new(environment.immutable_names.borrow().clone()),
            immutable_slots: RefCell::new(environment.immutable_slots.borrow().clone()),
            uninitialized: RefCell::new(environment.uninitialized.borrow().clone()),
            caller: None,
        })
    }

    pub(crate) fn child(captures: &Rc<Self>, values: Vec<Value>) -> Rc<Self> {
        let slots = SlotStore::from_values(values);
        let mut combined = captures.slots.slots.borrow().clone();
        combined.extend(slots.slots.borrow().iter().cloned());
        let caller = crate::locals::is_installed().then(crate::locals::current);
        let environment = Rc::new(Self {
            slots: Rc::new(SlotStore {
                slots: RefCell::new(combined),
            }),
            names: RefCell::new(None),
            immutable_names: RefCell::new(None),
            immutable_slots: RefCell::new(None),
            uninitialized: RefCell::new(None),
            caller,
        });
        environment
            .uninitialized
            .replace(captures.uninitialized.borrow().clone());
        environment
            .immutable_slots
            .replace(captures.immutable_slots.borrow().clone());
        environment
    }

    pub(crate) fn len(&self) -> usize {
        self.slots.len()
    }

    pub(crate) fn get(&self, slot: u16) -> Value {
        self.slots
            .get(slot)
            .map_or(Value::Undefined, |slot| slot.load())
    }

    pub(crate) fn set(&self, slot: u16, value: Value) {
        {
            let binding = self.ensure_slot(slot);
            binding.store(value);
        }
        self.initialize(slot);
    }

    fn slot(&self, slot: u16) -> Option<SlotCell> {
        self.slots.get(slot)
    }

    pub(crate) fn map_argument(
        &self,
        arguments: &mut crate::value::ArrayData,
        argument: usize,
        slot: u16,
    ) {
        if let Some(binding) = self.slot(slot) {
            arguments.map_index(argument, binding.0);
        }
    }

    pub(crate) fn slot_cell(&self, slot: u16) -> Rc<RefCell<Value>> {
        self.ensure_slot(slot).0
    }

    pub(crate) fn install_slot_cell(&self, slot: u16, cell: Rc<RefCell<Value>>) {
        self.slots.replace(slot, SlotCell(cell));
    }

    pub(crate) fn alias_name(&self, name: &str, slot: u16) {
        let binding = self.ensure_slot(slot);
        self.insert_alias(name, binding);
    }

    pub(crate) fn alias_caller_name(&self, name: &str, slot: u16) -> bool {
        let Some(caller) = &self.caller else {
            return false;
        };
        let binding = self.ensure_slot(slot);
        caller.insert_alias(name, binding);
        true
    }

    pub(crate) fn alias_binding(&self, name: &str, binding: Rc<RefCell<Value>>) {
        self.insert_alias(name, SlotCell(binding));
    }

    pub(crate) fn mark_immutable(&self, name: &str) {
        self.immutable_names
            .borrow_mut()
            .get_or_insert_with(HashSet::new)
            .insert(name.to_string());
    }

    pub(crate) fn mark_immutable_slot(&self, slot: u16) {
        self.immutable_slots
            .borrow_mut()
            .get_or_insert_with(HashSet::new)
            .insert(slot);
    }

    pub(crate) fn is_immutable_slot(&self, slot: u16) -> bool {
        self.immutable_slots
            .borrow()
            .as_ref()
            .is_some_and(|slots| slots.contains(&slot))
    }

    pub(crate) fn is_immutable_name(&self, name: &str) -> bool {
        self.immutable_names
            .borrow()
            .as_ref()
            .is_some_and(|names| names.contains(name))
            || self
                .caller
                .as_ref()
                .is_some_and(|caller| caller.is_immutable_name(name))
    }

    pub(crate) fn has_own_name(&self, name: &str) -> bool {
        self.names
            .borrow()
            .as_ref()
            .is_some_and(|names| names.contains_key(name))
    }

    pub(crate) fn has_name(&self, name: &str) -> bool {
        self.has_own_name(name)
            || self
                .caller
                .as_ref()
                .is_some_and(|caller| caller.has_name(name))
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

    fn insert_alias(&self, name: &str, binding: SlotCell) {
        self.names
            .borrow_mut()
            .get_or_insert_with(HashMap::new)
            .insert(name.to_string(), binding);
        self.shared_tdz();
    }

    pub(crate) fn resolve_name(&self, name: &str) -> Option<Value> {
        self.named_binding(name).map(|binding| binding.load())
    }

    pub(crate) fn set_named(&self, name: &str, value: Value) -> bool {
        let binding = self
            .names
            .borrow()
            .as_ref()
            .and_then(|names| names.get(name).cloned());
        if let Some(binding) = binding {
            binding.store(value);
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

    fn named_binding(&self, name: &str) -> Option<SlotCell> {
        let binding = self
            .names
            .borrow()
            .as_ref()
            .and_then(|names| names.get(name).cloned());
        binding.or_else(|| self.caller.as_ref()?.named_binding(name))
    }

    fn ensure_slot(&self, slot: u16) -> SlotCell {
        self.slots.ensure(slot)
    }

    pub(crate) fn initialize(&self, slot: u16) {
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

    fn remove_own_alias(&self, name: &str, slot: &SlotCell) -> bool {
        let mut names = self.names.borrow_mut();
        let Some(bindings) = names.as_mut() else {
            return false;
        };
        let matches = bindings
            .get(name)
            .is_some_and(|binding| binding.ptr_eq(slot));
        if matches {
            bindings.remove(name);
        }
        if bindings.is_empty() {
            *names = None;
        }
        matches
    }

    fn initialize_binding(&self, binding: &SlotCell) {
        let slot = (0..self.slots.len()).find(|slot| {
            self.slots
                .get(*slot as u16)
                .is_some_and(|candidate| candidate.ptr_eq(binding))
        });
        if let Some(slot) = slot.and_then(|slot| u16::try_from(slot).ok()) {
            self.initialize(slot);
        }
    }

    pub(crate) fn replace_slot(&self, slot: u16, value: Value) -> Rc<RefCell<Value>> {
        let previous = self.slots.replace(slot, SlotCell::new(value));
        self.initialize(slot);
        previous.0
    }

    pub(crate) fn restore_slot(&self, slot: u16, value: Rc<RefCell<Value>>) {
        self.slots.replace(slot, SlotCell(value));
    }

    pub(crate) fn replace_value(&self, old: &Value, new: &Value) {
        if let Some(caller) = &self.caller {
            caller.replace_value(old, new);
        }
        for index in 0..self.slots.len() {
            let Some(slot) = self.slots.get(index as u16) else {
                continue;
            };
            let mut value = slot.load();
            if same_identity(&value, old) {
                value = new.clone();
                replace_nested(&mut value, old, new);
            } else {
                replace_nested(&mut value, old, new);
            }
            slot.store(value);
        }
    }
}

include!("environment_alias.rs");
