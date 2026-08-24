use std::{
    cell::{RefCell, UnsafeCell},
    collections::{HashMap, HashSet},
    rc::Rc,
};

use crate::module_bindings::ModuleBindingCell;
use crate::value::Value;

/// Shared set of binding cells removed by direct-eval `delete`; matched by
/// cell identity so a reused slot number never shadows a live binding.
///
/// Environments are confined to the single-threaded VM. The vector is
/// mutated through `UnsafeCell`; entries retain their externally observable
/// `Rc<RefCell<Value>>` identity.
#[derive(Debug)]
struct DeletedCells(UnsafeCell<Vec<Rc<RefCell<Value>>>>);

impl PartialEq for DeletedCells {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

/// Execution-only TDZ metadata. Unlike binding cells, this set is never
/// exposed to host code, so its `RefCell` can be removed without changing
/// observable binding identity.
#[derive(Debug, Default)]
struct TdzCells(UnsafeCell<HashSet<u16>>);

impl PartialEq for TdzCells {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl TdzCells {
    fn new() -> Rc<Self> {
        Rc::new(Self(UnsafeCell::new(HashSet::new())))
    }

    fn clone_values(source: &Rc<Self>) -> Rc<Self> {
        let copy = unsafe { (&*source.0.get()).clone() };
        Rc::new(Self(UnsafeCell::new(copy)))
    }

    fn insert(&self, slot: u16) {
        unsafe {
            (&mut *self.0.get()).insert(slot);
        }
    }

    fn remove(&self, slot: u16) {
        unsafe {
            (&mut *self.0.get()).remove(&slot);
        }
    }

    fn contains(&self, slot: u16) -> bool {
        unsafe { (&*self.0.get()).contains(&slot) }
    }
}

#[derive(Debug)]
struct SlotStore {
    // SAFETY invariant: SlotStore is only reached through Rc and the VM is
    // single-threaded. `values` and `bridges` are mutated only by these
    // methods, and always have identical lengths. Before a bridge exists,
    // `values[index]` owns the slot value; after bridging, the Rc cell is the
    // authoritative source and `values[index]` is only the snapshot used to
    // create it.
    values: UnsafeCell<Vec<Value>>,
    bridges: UnsafeCell<Vec<Option<Rc<RefCell<Value>>>>>,
}

impl PartialEq for SlotStore {
    fn eq(&self, other: &Self) -> bool {
        self.values() == other.values() && self.bridges() == other.bridges()
    }
}

impl Default for SlotStore {
    fn default() -> Self {
        Self {
            values: UnsafeCell::new(Vec::new()),
            bridges: UnsafeCell::new(Vec::new()),
        }
    }
}

impl SlotStore {
    fn invariant(&self) {
        debug_assert_eq!(
            self.values().len(),
            self.bridges().len(),
            "slot value and bridge vectors must remain aligned"
        );
    }

    fn from_values(values: Vec<Value>) -> Rc<Self> {
        let store = Rc::new(Self {
            bridges: UnsafeCell::new((0..values.len()).map(|_| None).collect()),
            values: UnsafeCell::new(values),
        });
        store.invariant();
        store
    }

    fn from_cell(cell: Rc<RefCell<Value>>) -> Rc<Self> {
        let value = cell.borrow().clone();
        let store = Rc::new(Self {
            values: UnsafeCell::new(vec![value]),
            bridges: UnsafeCell::new(vec![Some(cell)]),
        });
        store.invariant();
        store
    }

    fn values(&self) -> &Vec<Value> {
        // SAFETY: VM execution is single-threaded; callers uphold the
        // SlotStore invariant and never retain this reference across mutation.
        unsafe { &*self.values.get() }
    }

    #[allow(clippy::mut_from_ref)]
    fn values_mut(&self) -> &mut Vec<Value> {
        // SAFETY: see `values`; mutation is confined to SlotStore methods.
        unsafe { &mut *self.values.get() }
    }

    fn bridges(&self) -> &Vec<Option<Rc<RefCell<Value>>>> {
        // SAFETY: see `values`.
        unsafe { &*self.bridges.get() }
    }

    #[allow(clippy::mut_from_ref)]
    fn bridges_mut(&self) -> &mut Vec<Option<Rc<RefCell<Value>>>> {
        // SAFETY: see `values_mut`.
        unsafe { &mut *self.bridges.get() }
    }

    fn ensure(&self, index: usize) {
        self.invariant();
        while self.values().len() <= index {
            self.values_mut().push(Value::Undefined);
            self.bridges_mut().push(None);
        }
        self.invariant();
    }

    fn len(&self) -> usize {
        self.invariant();
        self.values().len()
    }

    fn load(&self, index: usize) -> Value {
        self.ensure(index);
        self.bridges()
            .get(index)
            .and_then(Option::as_ref)
            .map_or_else(
                || self.values()[index].clone(),
                |cell| cell.borrow().clone(),
            )
    }

    fn load_number(&self, index: usize) -> Option<f64> {
        self.ensure(index);
        if let Some(Some(cell)) = self.bridges().get(index) {
            let value = cell.borrow();
            return match &*value {
                Value::Number(number) => Some(*number),
                _ => None,
            };
        }
        match &self.values()[index] {
            Value::Number(number) => Some(*number),
            _ => None,
        }
    }

    fn store(&self, index: usize, value: Value) {
        self.ensure(index);
        if let Some(Some(cell)) = self.bridges().get(index) {
            *cell.borrow_mut() = value;
        } else {
            self.values_mut()[index] = value;
        }
        self.invariant();
    }

    fn update_number(&self, index: usize, delta: f64) -> Option<(f64, f64)> {
        self.ensure(index);
        let value = if let Some(Some(cell)) = self.bridges().get(index) {
            let mut value = cell.borrow_mut();
            let Value::Number(old) = &mut *value else {
                return None;
            };
            let before = *old;
            *old += delta;
            return Some((before, *old));
        } else {
            &mut self.values_mut()[index]
        };
        let Value::Number(old) = value else {
            return None;
        };
        let before = *old;
        *old += delta;
        Some((before, *old))
    }

    fn bridge(&self, index: usize) -> Rc<RefCell<Value>> {
        self.ensure(index);
        if let Some(Some(cell)) = self.bridges().get(index) {
            return Rc::clone(cell);
        }
        let cell = Rc::new(RefCell::new(self.values()[index].clone()));
        self.bridges_mut()[index] = Some(Rc::clone(&cell));
        self.invariant();
        cell
    }
}

#[derive(Debug, Clone, PartialEq)]
struct BindingRef {
    store: Rc<SlotStore>,
    index: usize,
}

/// A closure's captured binding references are immutable facts: their values
/// mutate through `SlotStore`, but the mapping from slot to store does not.
/// Child calls therefore share that prefix and allocate only their local tail.
#[derive(Debug, Default, PartialEq)]
struct SlotRefs {
    prefix: Rc<[BindingRef]>,
    suffix: Vec<BindingRef>,
}

impl SlotRefs {
    fn from_prefix(prefix: Rc<[BindingRef]>) -> Self {
        Self {
            prefix,
            suffix: Vec::new(),
        }
    }

    fn len(&self) -> usize {
        self.prefix.len() + self.suffix.len()
    }

    fn get(&self, index: usize) -> Option<&BindingRef> {
        if index < self.prefix.len() {
            self.prefix.get(index)
        } else {
            self.suffix.get(index - self.prefix.len())
        }
    }

    fn shared_prefix(&self) -> Rc<[BindingRef]> {
        if self.suffix.is_empty() {
            return Rc::clone(&self.prefix);
        }
        self.prefix
            .iter()
            .chain(&self.suffix)
            .cloned()
            .collect::<Vec<_>>()
            .into()
    }

    fn push(&mut self, binding: BindingRef) {
        self.suffix.push(binding);
    }

    fn replace(&mut self, index: usize, binding: BindingRef) {
        if index < self.prefix.len() {
            let mut owned = self.prefix.to_vec();
            owned.append(&mut self.suffix);
            self.prefix = Rc::from([]);
            self.suffix = owned;
        }
        self.suffix[index - self.prefix.len()] = binding;
    }
}

impl BindingRef {
    fn new(store: Rc<SlotStore>, index: usize) -> Self {
        store.ensure(index);
        Self { store, index }
    }

    fn load(&self) -> Value {
        self.store.load(self.index)
    }

    fn load_number(&self) -> Option<f64> {
        self.store.load_number(self.index)
    }

    fn store(&self, value: Value) {
        self.store.store(self.index, value);
    }

    fn update_number(&self, delta: f64) -> Option<(f64, f64)> {
        self.store.update_number(self.index, delta)
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
    slots: RefCell<SlotRefs>,
    names: RefCell<Option<HashMap<String, BindingRef>>>,
    eval_names: RefCell<Option<HashMap<String, BindingRef>>>,
    immutable_names: RefCell<Option<HashSet<String>>>,
    immutable_slots: RefCell<Option<HashSet<u16>>>,
    uninitialized: RefCell<Option<Rc<TdzCells>>>,
    deleted_cells: RefCell<Option<Rc<DeletedCells>>>,
    caller: Option<Rc<Self>>,
}

fn clone_tdz(source: &Option<Rc<TdzCells>>) -> Option<Rc<TdzCells>> {
    source.as_ref().map(TdzCells::clone_values)
}

impl Environment {
    pub(crate) fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    pub(crate) fn capture(environment: &Rc<Self>, count: u16) -> Rc<Self> {
        let count = usize::from(count);
        for index in 0..count {
            environment.ensure_slot(index as u16);
        }
<<<<<<< HEAD
        let refs: Rc<[BindingRef]> = (0..count)
            .filter_map(|index| environment.slot(index as u16))
            .collect::<Vec<_>>()
            .into();
        // A captured environment may contain bindings belonging to its
        // caller.  Do not carry immutable-slot markers for those discarded
        // slots into the new function-local frame: slot numbers are reused
        // by the callee after the captured prefix.
        let immutable_slots = environment.immutable_slots.borrow().as_ref().map(|slots| {
            slots
                .iter()
                .copied()
                .filter(|slot| usize::from(*slot) < count)
                .collect()
        });
        Rc::new(Self {
            slots: RefCell::new(SlotRefs::from_prefix(refs)),
            names: RefCell::new(environment.names.borrow().clone()),
            eval_names: RefCell::new(environment.eval_names.borrow().clone()),
            immutable_names: RefCell::new(environment.immutable_names.borrow().clone()),
            immutable_slots: RefCell::new(immutable_slots),
            uninitialized: RefCell::new(environment.uninitialized.borrow().clone()),
            deleted_cells: RefCell::new(environment.deleted_cells.borrow().clone()),
            caller: Some(Rc::clone(environment)),
        })
    }

    pub(crate) fn child(captures: &Rc<Self>, values: Vec<Value>) -> Rc<Self> {
        crate::execution_trace::environment_child(captures.len(), values.len());
        let store = SlotStore::from_values(values);
        let prefix = captures.slots.borrow().shared_prefix();
        let suffix = (0..store.len())
            .map(|index| BindingRef::new(Rc::clone(&store), index))
            .collect();
        let environment = Rc::new(Self {
            slots: RefCell::new(SlotRefs { prefix, suffix }),
            names: RefCell::new(None),
            eval_names: RefCell::new(None),
            immutable_names: RefCell::new(None),
            immutable_slots: RefCell::new(None),
            uninitialized: RefCell::new(None),
            deleted_cells: RefCell::new(None),
            caller: Some(Rc::clone(captures)),
        });
        environment
            .uninitialized
            .replace(clone_tdz(&captures.uninitialized.borrow()));
        environment
            .deleted_cells
            .replace(captures.deleted_cells.borrow().clone());
        environment
            .immutable_slots
            .replace(captures.immutable_slots.borrow().clone());
        environment
    }

    pub(crate) fn in_place_child(captures: &Rc<Self>, values: Vec<Value>) -> Rc<Self> {
        Self::child(captures, values)
    }
    pub(crate) fn len(&self) -> usize {
        self.slots.borrow().len()
    }

    pub(crate) fn get(&self, slot: u16) -> Value {
        self.slot(slot).map_or(Value::Undefined, |slot| slot.load())
    }

    pub(crate) fn get_number(&self, slot: u16) -> Option<f64> {
        self.slot(slot)?.load_number()
    }

    pub(crate) fn set(&self, slot: u16, value: Value) {
        {
            let binding = self.ensure_slot(slot);
            binding.store(value);
        }
        self.initialize(slot);
    }

    pub(crate) fn update_number(&self, slot: u16, delta: f64) -> Option<(f64, f64)> {
        if self.is_immutable_slot(slot) && !self.is_uninitialized(slot) {
            return None;
        }
        self.slot(slot)?.update_number(delta)
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
        self.ensure_slot(slot);
        self.slots
            .borrow_mut()
            .replace(index, BindingRef::new(SlotStore::from_cell(cell), 0));
    }

    pub(crate) fn alias_eval_caller_name(&self, name: &str, slot: u16) -> bool {
        let Some(caller) = &self.caller else {
            return false;
        };
        let binding = self.ensure_slot(slot);
        caller.clear_deleted_cell(&binding.cell());
        caller
            .eval_names
            .borrow_mut()
            .get_or_insert_with(HashMap::new)
            .insert(name.to_string(), binding.clone());
        if name == "arguments" {
            caller.alias_eval_binding(name, binding);
        }
        true
    }

    fn alias_eval_binding(&self, name: &str, binding: BindingRef) {
        self.eval_names
            .borrow_mut()
            .get_or_insert_with(HashMap::new)
            .insert(name.to_string(), binding.clone());
        if let Some(caller) = &self.caller {
            caller.alias_eval_binding(name, binding);
        }
    }

    pub(crate) fn alias_binding(&self, name: &str, binding: Rc<RefCell<Value>>) {
        self.alias_module_binding(name, ModuleBindingCell::from_shared(binding));
    }

    pub(crate) fn alias_module_binding(&self, name: &str, binding: ModuleBindingCell) {
        let store = SlotStore::from_cell(binding.shared());
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

    pub(crate) fn resolve_eval_name(&self, name: &str) -> Option<Value> {
        self.eval_name_binding(name).map(|binding| binding.load())
    }

    pub(crate) fn eval_name_aliases_slot(&self, name: &str, slot: u16) -> bool {
        if self
            .eval_names
            .borrow()
            .as_ref()
            .is_some_and(|names| names.contains_key(name))
        {
            return true;
        }
        let Some(caller) = &self.caller else {
            return false;
        };
        // A call frame keeps captured bindings as the same cells while
        // appending its own slots.  Only continue through the caller chain
        // for a captured slot; a newly allocated local with the same index
        // must continue to shadow an eval binding in the caller.
        let captured = self
            .slot(slot)
            .zip(caller.slot(slot))
            .is_some_and(|(current, parent)| current.same(&parent));
        captured && caller.eval_name_aliases_slot(name, slot)
    }

    fn eval_name_binding(&self, name: &str) -> Option<BindingRef> {
        if let Some(binding) = self
            .eval_names
            .borrow()
            .as_ref()
            .and_then(|names| names.get(name).cloned())
        {
            return Some(binding);
        }
        self.caller.as_ref()?.eval_name_binding(name)
    }

    pub(crate) fn set_eval_named(&self, name: &str, value: Value) -> bool {
        let binding = self
            .eval_names
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
            .is_some_and(|caller| caller.set_eval_named(name, value))
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

    pub(crate) fn delete_eval_caller_name(&self, name: &str, slot: u16) -> bool {
        let (Some(caller), Some(binding)) = (&self.caller, self.slot(slot)) else {
            return false;
        };
        let removed = caller.remove_own_eval_alias(name, &binding);
        if removed {
            caller.mark_deleted_cell(binding.cell());
        }
        removed
    }

    pub(crate) fn mark_uninitialized(&self, slot: u16) {
        self.ensure_slot(slot);
        self.writable_tdz().insert(slot);
    }
    pub(crate) fn mark_uninitialized_shared(&self, slot: u16) {
        self.ensure_slot(slot);
        self.shared_tdz().insert(slot);
    }
    pub(crate) fn is_deleted(&self, cell: &Rc<RefCell<Value>>) -> bool {
        self.deleted_cells.borrow().as_ref().is_some_and(|cells| {
            // SAFETY: VM execution is single-threaded; all access is through
            // Environment methods and the Rc keeps the vector alive.
            let cells = unsafe { &*cells.0.get() };
            cells.iter().any(|candidate| Rc::ptr_eq(candidate, cell))
        }) || self
            .caller
            .as_ref()
            .is_some_and(|caller| caller.is_deleted(cell))
    }

    fn mark_deleted_cell(&self, cell: Rc<RefCell<Value>>) {
        let mut state = self.deleted_cells.borrow_mut();
        let cells = state.get_or_insert_with(|| Rc::new(DeletedCells(UnsafeCell::new(Vec::new()))));
        // SAFETY: VM execution is single-threaded; no aliased mutable access
        // occurs while this method runs.
        unsafe { &mut *cells.0.get() }.push(cell);
    }
    fn clear_deleted_cell(&self, cell: &Rc<RefCell<Value>>) {
        if let Some(cells) = self.deleted_cells.borrow().as_ref() {
            // SAFETY: VM execution is single-threaded; see mark_deleted_cell.
            unsafe { &mut *cells.0.get() }.retain(|candidate| !Rc::ptr_eq(candidate, cell));
        }
    }

    fn writable_tdz(&self) -> Rc<TdzCells> {
        let mut state = self.uninitialized.borrow_mut();
        if let Some(slots) = state.as_ref() {
            if Rc::strong_count(slots) == 1 {
                return Rc::clone(slots);
            }
            let detached = TdzCells::clone_values(slots);
            *state = Some(Rc::clone(&detached));
            return detached;
        }
        let slots = TdzCells::new();
        *state = Some(Rc::clone(&slots));
        slots
    }

    pub(crate) fn is_uninitialized(&self, slot: u16) -> bool {
        self.uninitialized
            .borrow()
            .as_ref()
            .is_some_and(|slots| slots.contains(slot))
    }

    fn named_binding(&self, name: &str) -> Option<BindingRef> {
        let binding = self
            .names
            .borrow()
            .as_ref()
            .and_then(|names| names.get(name).cloned());
        binding.or_else(|| self.caller.as_ref()?.named_binding(name))
    }

    fn remove_own_eval_alias(&self, name: &str, binding: &BindingRef) -> bool {
        let mut names = self.eval_names.borrow_mut();
        let Some(names) = names.as_mut() else {
            return false;
        };
        let Some(current) = names.get(name) else {
            return false;
        };
        if !current.same(binding) {
            return false;
        }
        names.remove(name);
        true
    }

    fn ensure_slot(&self, slot: u16) -> BindingRef {
        if let Some(binding) = self.slot(slot) {
            return binding;
        }
        let index = usize::from(slot);
        let mut slots = self.slots.borrow_mut();
        while slots.len() <= index {
            let store = SlotStore::from_values(vec![Value::Undefined]);
            slots.push(BindingRef::new(store, 0));
        }
        slots.get(index).expect("ensured environment slot").clone()
    }

    pub(crate) fn initialize(&self, slot: u16) {
        if let Some(slots) = self.uninitialized.borrow().as_ref() {
            slots.remove(slot);
        }
    }
    fn shared_tdz(&self) -> Rc<TdzCells> {
        let mut state = self.uninitialized.borrow_mut();
        let slots = state.get_or_insert_with(TdzCells::new);
        Rc::clone(slots)
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
        self.slots
            .borrow_mut()
            .replace(usize::from(slot), BindingRef::new(store, 0));
        self.initialize(slot);
        previous.cell()
    }

    pub(crate) fn restore_slot(&self, slot: u16, value: Rc<RefCell<Value>>) {
        let binding = BindingRef::new(SlotStore::from_cell(value), 0);
        self.slots.borrow_mut().replace(usize::from(slot), binding);
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

#[cfg(test)]
mod tests {
    use super::{Environment, SlotStore};
    use crate::value::Value;

    #[test]
    fn child_frames_share_captured_bindings_but_not_slot_replacements() {
        let root = Environment::new();
        let parent = Environment::child(&root, vec![Value::Number(1.0)]);
        let captures = Environment::capture(&parent, 1);
        let first = Environment::child(&captures, vec![Value::Number(10.0)]);
        let second = Environment::child(&captures, vec![Value::Number(20.0)]);

        first.set(0, Value::Number(2.0));
        assert_eq!(second.get(0), Value::Number(2.0));
        assert_eq!(parent.get(0), Value::Number(2.0));
        assert_eq!(first.get(1), Value::Number(10.0));
        assert_eq!(second.get(1), Value::Number(20.0));

        first.replace_slot(0, Value::Number(3.0));
        assert_eq!(first.get(0), Value::Number(3.0));
        assert_eq!(second.get(0), Value::Number(2.0));
    }

    #[test]
    fn slot_store_round_trips_values_without_borrow_tracking() {
        let store = SlotStore::from_values(vec![Value::Number(1.0)]);
        assert_eq!(store.load(0), Value::Number(1.0));
        store.store(0, Value::Number(2.0));
        assert_eq!(store.load(0), Value::Number(2.0));
        assert_eq!(store.load(2), Value::Undefined);
    }

    #[test]
    fn bridge_cells_preserve_shared_binding_identity() {
        let store = SlotStore::from_values(vec![Value::Undefined]);
        let cell = store.bridge(0);
        store.store(0, Value::Number(3.0));
        assert_eq!(*cell.borrow(), Value::Number(3.0));
    }

    #[test]
    fn bridged_cell_is_authoritative_source_after_external_update() {
        let cell = std::rc::Rc::new(std::cell::RefCell::new(Value::Number(4.0)));
        let store = SlotStore::from_cell(std::rc::Rc::clone(&cell));
        *cell.borrow_mut() = Value::Number(9.0);
        assert_eq!(store.load(0), Value::Number(9.0));
        store.store(0, Value::Number(12.0));
        assert_eq!(*cell.borrow(), Value::Number(12.0));
    }

    #[test]
    fn numeric_update_mutates_direct_and_bridged_slots_once() {
        let direct = SlotStore::from_values(vec![Value::Number(4.0)]);
        assert_eq!(direct.update_number(0, 1.0), Some((4.0, 5.0)));
        assert_eq!(direct.load(0), Value::Number(5.0));

        let bridged = SlotStore::from_values(vec![Value::Number(7.0)]);
        let cell = bridged.bridge(0);
        assert_eq!(bridged.update_number(0, -1.0), Some((7.0, 6.0)));
        assert_eq!(*cell.borrow(), Value::Number(6.0));
    }

    #[test]
    fn numeric_update_rejects_non_numbers_without_mutation() {
        let store = SlotStore::from_values(vec![Value::String("4".into())]);
        assert_eq!(store.update_number(0, 1.0), None);
        assert_eq!(store.load(0), Value::String("4".into()));
    }

    #[test]
    #[ignore = "release-only diagnostic; run with --release --ignored slot_access_cost_probe"]
    fn slot_access_cost_probe() {
        const ITERATIONS: usize = 20_000_000;
        let environment = Environment::new();
        environment.set(0, Value::Number(7.0));
        let _guard = crate::locals::EnvironmentGuard::install(std::rc::Rc::clone(&environment));
        let store = SlotStore::from_values(vec![Value::Number(7.0)]);

        let measure = |mut read: Box<dyn FnMut() -> f64>| {
            let started = std::time::Instant::now();
            let mut sum = 0.0;
            for _ in 0..ITERATIONS {
                sum += std::hint::black_box(read());
            }
            (started.elapsed(), sum)
        };
        let (raw, _) = measure(Box::new(|| store.load_number(0).unwrap()));
        let (slot, _) = measure(Box::new(|| environment.get_number(0).unwrap()));
        let (checked, _) = measure(Box::new(|| {
            assert!(!environment.is_uninitialized(0));
            environment.get_number(0).unwrap()
        }));
        let (current, _) = measure(Box::new(|| {
            let environment = crate::locals::current();
            assert!(!environment.is_uninitialized(0));
            environment.get_number(0).unwrap()
        }));
        eprintln!("slot_probe iterations={ITERATIONS} raw={raw:?} slot={slot:?} checked={checked:?} current={current:?}");
    }
}
