use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

use crate::value::Value;

#[derive(Debug, Default, PartialEq)]
struct SlotStore {
    values: RefCell<Vec<Value>>,
    bridges: RefCell<Vec<Option<Rc<RefCell<Value>>>>>,
}

impl SlotStore {
    fn from_values(values: Vec<Value>) -> Rc<Self> {
        Rc::new(Self {
            bridges: RefCell::new((0..values.len()).map(|_| None).collect()),
            values: RefCell::new(values),
        })
    }

    fn from_cell(cell: Rc<RefCell<Value>>) -> Rc<Self> {
        let value = cell.borrow().clone();
        Rc::new(Self {
            values: RefCell::new(vec![value]),
            bridges: RefCell::new(vec![Some(cell)]),
        })
    }

    fn ensure(&self, index: usize) {
        let mut values = self.values.borrow_mut();
        while values.len() <= index {
            values.push(Value::Undefined);
        }
        let mut bridges = self.bridges.borrow_mut();
        while bridges.len() <= index {
            bridges.push(None);
        }
    }

    fn len(&self) -> usize {
        self.values.borrow().len()
    }

    fn load(&self, index: usize) -> Value {
        self.ensure(index);
        self.bridges
            .borrow()
            .get(index)
            .and_then(Option::as_ref)
            .map_or_else(
                || self.values.borrow()[index].clone(),
                |cell| cell.borrow().clone(),
            )
    }

    fn store(&self, index: usize, value: Value) {
        self.ensure(index);
        if let Some(Some(cell)) = self.bridges.borrow().get(index) {
            *cell.borrow_mut() = value;
        } else {
            self.values.borrow_mut()[index] = value;
        }
    }

    fn bridge(&self, index: usize) -> Rc<RefCell<Value>> {
        self.ensure(index);
        if let Some(Some(cell)) = self.bridges.borrow().get(index) {
            return Rc::clone(cell);
        }
        let cell = Rc::new(RefCell::new(self.values.borrow()[index].clone()));
        self.bridges.borrow_mut()[index] = Some(Rc::clone(&cell));
        cell
    }
}

#[derive(Debug, Clone, PartialEq)]
struct BindingRef {
    store: Rc<SlotStore>,
    index: usize,
}

impl BindingRef {
    fn new(store: Rc<SlotStore>, index: usize) -> Self {
        store.ensure(index);
        Self { store, index }
    }

    fn load(&self) -> Value {
        self.store.load(self.index)
    }

    fn store(&self, value: Value) {
        self.store.store(self.index, value);
    }

    fn cell(&self) -> Rc<RefCell<Value>> {
        self.store.bridge(self.index)
    }

    fn same(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.store, &other.store) && self.index == other.index
    }
}

/// Shared indexed lexical bindings. Captured prefixes share their slot cells.
#[derive(Debug, Default, PartialEq)]
pub struct Environment {
    slots: RefCell<Vec<BindingRef>>,
    names: RefCell<Option<HashMap<String, BindingRef>>>,
    immutable_names: RefCell<Option<HashSet<String>>>,
    immutable_slots: RefCell<Option<HashSet<u16>>>,
    uninitialized: RefCell<Option<Rc<RefCell<HashSet<u16>>>>>,
    caller: Option<Rc<Self>>,
}

impl Environment {
    pub(crate) fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    fn refs(store: Rc<SlotStore>) -> Vec<BindingRef> {
        (0..store.len())
            .map(|index| BindingRef::new(Rc::clone(&store), index))
            .collect()
    }

    pub(crate) fn capture(environment: &Rc<Self>, count: u16) -> Rc<Self> {
        let count = usize::from(count);
        for index in 0..count {
            environment.ensure_slot(index as u16);
        }
        let slots = environment.slots.borrow();
        let refs = slots.iter().take(count).cloned().collect();
        Rc::new(Self {
            slots: RefCell::new(refs),
            names: RefCell::new(environment.names.borrow().clone()),
            immutable_names: RefCell::new(environment.immutable_names.borrow().clone()),
            immutable_slots: RefCell::new(environment.immutable_slots.borrow().clone()),
            uninitialized: RefCell::new(environment.uninitialized.borrow().clone()),
            caller: None,
        })
    }

    pub(crate) fn child(captures: &Rc<Self>, values: Vec<Value>) -> Rc<Self> {
        let store = SlotStore::from_values(values);
        let mut combined = captures.slots.borrow().clone();
        combined.extend((0..store.len()).map(|index| BindingRef::new(Rc::clone(&store), index)));
        let caller = crate::locals::is_installed().then(crate::locals::current);
        let environment = Rc::new(Self {
            slots: RefCell::new(combined),
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
        self.slots.borrow().len()
    }

    pub(crate) fn get(&self, slot: u16) -> Value {
        self.slot(slot).map_or(Value::Undefined, |slot| slot.load())
    }

    pub(crate) fn set(&self, slot: u16, value: Value) {
        {
            let binding = self.ensure_slot(slot);
            binding.store(value);
        }
        self.initialize(slot);
    }

    fn slot(&self, slot: u16) -> Option<BindingRef> {
        self.slots.borrow().get(usize::from(slot)).cloned()
    }

    pub(crate) fn map_argument(
        &self,
        arguments: &mut crate::value::ArrayData,
        argument: usize,
        slot: u16,
    ) {
        if let Some(binding) = self.slot(slot) {
            arguments.map_index(argument, binding.cell());
        }
    }

    pub(crate) fn slot_cell(&self, slot: u16) -> Rc<RefCell<Value>> {
        self.ensure_slot(slot).cell()
    }

    pub(crate) fn install_slot_cell(&self, slot: u16, cell: Rc<RefCell<Value>>) {
        let index = usize::from(slot);
        if let Some(binding) = self.slot(slot) {
            let _ = binding;
            self.slots.borrow_mut()[index] = BindingRef::new(SlotStore::from_cell(cell), 0);
        } else {
            let store = SlotStore::from_cell(cell);
            self.slots
                .borrow_mut()
                .resize_with(index, || BindingRef::new(Rc::clone(&store), 0));
            self.slots.borrow_mut().push(BindingRef::new(store, 0));
        }
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
        let store = SlotStore::from_cell(binding);
        let reference = BindingRef::new(store, 0);
        self.insert_alias(name, reference);
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

    fn insert_alias(&self, name: &str, binding: BindingRef) {
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

    fn named_binding(&self, name: &str) -> Option<BindingRef> {
        let binding = self
            .names
            .borrow()
            .as_ref()
            .and_then(|names| names.get(name).cloned());
        binding.or_else(|| self.caller.as_ref()?.named_binding(name))
    }

    fn ensure_slot(&self, slot: u16) -> BindingRef {
        if let Some(binding) = self.slot(slot) {
            return binding;
        }
        let index = usize::from(slot);
        let store = SlotStore::from_values(vec![Value::Undefined; index.saturating_add(1)]);
        let refs = Self::refs(store);
        let binding = refs[index].clone();
        self.slots.borrow_mut().extend(refs);
        binding
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

    fn remove_own_alias(&self, name: &str, slot: &BindingRef) -> bool {
        let mut names = self.names.borrow_mut();
        let Some(bindings) = names.as_mut() else {
            return false;
        };
        let matches = bindings.get(name).is_some_and(|binding| binding.same(slot));
        if matches {
            bindings.remove(name);
        }
        if bindings.is_empty() {
            *names = None;
        }
        matches
    }

    fn initialize_binding(&self, binding: &BindingRef) {
        let slot = (0..self.slots.borrow().len()).find(|slot| {
            self.slot(*slot as u16)
                .is_some_and(|candidate| candidate.same(binding))
        });
        if let Some(slot) = slot.and_then(|slot| u16::try_from(slot).ok()) {
            self.initialize(slot);
        }
    }

    pub(crate) fn replace_slot(&self, slot: u16, value: Value) -> Rc<RefCell<Value>> {
        let previous = self.ensure_slot(slot);
        let store = SlotStore::from_values(vec![value]);
        self.slots.borrow_mut()[usize::from(slot)] = BindingRef::new(store, 0);
        self.initialize(slot);
        previous.cell()
    }

    pub(crate) fn restore_slot(&self, slot: u16, value: Rc<RefCell<Value>>) {
        let binding = BindingRef::new(SlotStore::from_cell(value), 0);
        self.slots.borrow_mut()[usize::from(slot)] = binding;
    }

    pub(crate) fn replace_value(&self, old: &Value, new: &Value) {
        if let Some(caller) = &self.caller {
            caller.replace_value(old, new);
        }
        for index in 0..self.slots.borrow().len() {
            let Some(slot) = self.slot(index as u16) else {
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
