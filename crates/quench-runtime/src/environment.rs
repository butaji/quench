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
/// `Rc<crate::value::BindingCell>` identity.
#[derive(Debug)]
struct DeletedCells(UnsafeCell<Vec<Rc<crate::value::BindingCell>>>);

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
    values: UnsafeCell<crate::register_file::RegisterFile>,
    bridges: UnsafeCell<Option<Vec<Option<Rc<crate::value::BindingCell>>>>>,
}

impl PartialEq for SlotStore {
    fn eq(&self, other: &Self) -> bool {
        self.values() == other.values() && self.bridges() == other.bridges()
    }
}

impl Default for SlotStore {
    fn default() -> Self {
        Self {
            values: UnsafeCell::new(crate::register_file::RegisterFile::new()),
            bridges: UnsafeCell::new(None),
        }
    }
}

impl SlotStore {
    fn invariant(&self) {
        if let Some(bridges) = self.bridges() {
            debug_assert_eq!(self.values().len(), bridges.len());
        }
    }

    fn from_values(values: Vec<Value>) -> Rc<Self> {
        Self::from_registers(crate::register_file::RegisterFile::from_values(values))
    }

    fn from_registers(values: crate::register_file::RegisterFile) -> Rc<Self> {
        let store = Rc::new(Self {
            bridges: UnsafeCell::new(None),
            values: UnsafeCell::new(values),
        });
        store.invariant();
        store
    }

    fn from_cell(cell: Rc<crate::value::BindingCell>) -> Rc<Self> {
        let value = cell.load();
        let store = Rc::new(Self {
            values: UnsafeCell::new(crate::register_file::RegisterFile::from_values(vec![value])),
            bridges: UnsafeCell::new(Some(vec![Some(cell)])),
        });
        store.invariant();
        store
    }

    fn values(&self) -> &crate::register_file::RegisterFile {
        // SAFETY: VM execution is single-threaded; callers uphold the
        // SlotStore invariant and never retain this reference across mutation.
        unsafe { &*self.values.get() }
    }

    #[allow(clippy::mut_from_ref)]
    fn values_mut(&self) -> &mut crate::register_file::RegisterFile {
        // SAFETY: see `values`; mutation is confined to SlotStore methods.
        unsafe { &mut *self.values.get() }
    }

    fn bridges(&self) -> Option<&Vec<Option<Rc<crate::value::BindingCell>>>> {
        // SAFETY: see `values`.
        unsafe { (&*self.bridges.get()).as_ref() }
    }

    #[allow(clippy::mut_from_ref)]
    fn bridges_mut(&self) -> &mut Vec<Option<Rc<crate::value::BindingCell>>> {
        // SAFETY: see `values_mut`.
        let bridges = unsafe { &mut *self.bridges.get() };
        bridges.get_or_insert_with(|| vec![None; self.values().len()])
    }

    fn ensure(&self, index: usize) {
        self.invariant();
        if self.values().len() <= index {
            self.values_mut().resize_undefined(index + 1);
        }
        if self.bridges().is_some() {
            while self.bridges().is_some_and(|bridges| bridges.len() <= index) {
                self.bridges_mut().push(None);
            }
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
            .and_then(|bridges| bridges.get(index))
            .and_then(Option::as_ref)
            .map_or_else(
                || self.values().read(index).unwrap_or(Value::Undefined),
                |cell| cell.load(),
            )
            .strong_function()
    }

    fn load_number(&self, index: usize) -> Option<f64> {
        self.ensure(index);
        if let Some(Some(cell)) = self.bridges().and_then(|bridges| bridges.get(index)) {
            let value = cell.borrow();
            return match &*value {
                Value::Number(number) => Some(*number),
                _ => None,
            };
        }
        self.values().read_number(index)
    }

    fn load_into(
        &self,
        registers: &mut crate::register_file::RegisterFile,
        dst: u16,
        index: usize,
    ) {
        self.ensure(index);
        self.load_existing_into(registers, dst, index);
    }

    #[inline(always)]
    fn load_existing_into(
        &self,
        registers: &mut crate::register_file::RegisterFile,
        dst: u16,
        index: usize,
    ) {
        let value = self
            .bridges()
            .and_then(|bridges| bridges.get(index))
            .and_then(Option::as_ref)
            .map(|cell| cell.borrow());
        if let Some(value) = value.as_deref() {
            crate::execute::write_value(registers, dst, value.clone().strong_function());
        } else {
            crate::execution_trace::event(crate::execution_trace::Event::LocalWordRead);
            let copied =
                registers.copy_strong_function_from(usize::from(dst), self.values(), index);
            debug_assert!(copied, "ensured lexical slot must own an execute word");
        }
    }

    fn load_into_fixed<const N: usize>(
        &self,
        registers: &mut crate::register_file::FixedWordFile<N>,
        dst: usize,
        index: usize,
    ) -> bool {
        self.ensure(index);
        if let Some(Some(cell)) = self.bridges().and_then(|bridges| bridges.get(index)) {
            return cell.with_word(|word| registers.write_owned(dst, word).is_some());
        }
        crate::execution_trace::event(crate::execution_trace::Event::LocalWordRead);
        registers.copy_from(dst, self.values(), index).is_some()
    }

    fn store(&self, index: usize, value: Value) {
        self.ensure(index);
        if let Some(Some(cell)) = self.bridges().and_then(|bridges| bridges.get(index)) {
            cell.store(value);
        } else {
            self.values_mut().write(index, value);
        }
        self.invariant();
    }

    fn copy_from_register(
        &self,
        index: usize,
        registers: &crate::register_file::RegisterFile,
        source: u16,
    ) -> bool {
        self.ensure(index);
        if let Some(Some(cell)) = self.bridges().and_then(|bridges| bridges.get(index)) {
            let Some(value) = registers.read(usize::from(source)) else {
                return false;
            };
            cell.store(value);
            return true;
        }
        self.values_mut()
            .copy_from(index, registers, usize::from(source))
    }

    #[inline(always)]
    fn immediate_word_ptr(&self, index: usize) -> Option<*mut crate::tagged_value::TaggedValue> {
        if self
            .bridges()
            .and_then(|bridges| bridges.get(index))
            .is_some_and(Option::is_some)
        {
            return None;
        }
        self.values_mut().immediate_word_ptr(index)
    }

    fn existing_cell(&self, index: usize) -> Option<Rc<crate::value::BindingCell>> {
        self.bridges()
            .and_then(|bridges| bridges.get(index))
            .and_then(Option::as_ref)
            .map(Rc::clone)
    }

    fn update_number(&self, index: usize, delta: f64) -> Option<(f64, f64)> {
        self.ensure(index);
        if let Some(Some(cell)) = self.bridges().and_then(|bridges| bridges.get(index)) {
            let mut value = cell.borrow_mut();
            let Value::Number(old) = &mut *value else {
                return None;
            };
            let before = *old;
            *old += delta;
            return Some((before, *old));
        } else {
            let before = self.values().read_number(index)?;
            let after = before + delta;
            self.values_mut().write_number(index, after);
            Some((before, after))
        }
    }

    fn bridge(&self, index: usize) -> Rc<crate::value::BindingCell> {
        self.ensure(index);
        if let Some(Some(cell)) = self.bridges().and_then(|bridges| bridges.get(index)) {
            return Rc::clone(cell);
        }
        let cell =
            crate::value::BindingCell::new(self.values().read(index).unwrap_or(Value::Undefined));
        self.bridges_mut()[index] = Some(Rc::clone(&cell));
        self.invariant();
        cell
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BindingRef {
    store: Rc<SlotStore>,
    index: usize,
}

/// A closure's captured binding references are immutable facts: their values
/// mutate through `SlotStore`, but the mapping from slot to store does not.
/// Child calls therefore share that prefix and allocate only their local tail.
#[derive(Debug, Default, PartialEq)]
struct SlotRefs {
    prefix_len: usize,
    prefix: Rc<[CapturedRef]>,
    suffix_store: Option<Rc<SlotStore>>,
    suffix_len: usize,
    suffix_overrides: Vec<CapturedRef>,
}

#[derive(Debug, Clone, PartialEq)]
struct CapturedRef {
    slot: usize,
    binding: BindingRef,
}

impl SlotRefs {
    fn from_prefix(prefix_len: usize, prefix: Rc<[CapturedRef]>) -> Self {
        Self {
            prefix_len,
            prefix,
            suffix_store: None,
            suffix_len: 0,
            suffix_overrides: Vec::new(),
        }
    }

    fn len(&self) -> usize {
        self.prefix_len + self.suffix_len
    }

    fn get(&self, index: usize) -> Option<BindingRef> {
        if index < self.prefix_len {
            return self
                .prefix
                .binary_search_by_key(&index, |capture| capture.slot)
                .ok()
                .and_then(|found| self.prefix.get(found))
                .map(|capture| capture.binding.clone());
        }
        if index >= self.len() {
            return None;
        }
        if let Ok(found) = self
            .suffix_overrides
            .binary_search_by_key(&index, |entry| entry.slot)
        {
            return self
                .suffix_overrides
                .get(found)
                .map(|entry| entry.binding.clone());
        }
        let store = Rc::clone(self.suffix_store.as_ref()?);
        Some(BindingRef {
            store,
            index: index - self.prefix_len,
        })
    }

    /// Visit the canonical slot without constructing another owning binding
    /// reference. The immutable slot map keeps both the store and index valid
    /// for the duration of the callback.
    #[inline(always)]
    fn with_binding<R>(
        &self,
        index: usize,
        use_binding: impl FnOnce(&SlotStore, usize) -> R,
    ) -> Option<R> {
        if index < self.prefix_len {
            let found = self
                .prefix
                .binary_search_by_key(&index, |capture| capture.slot)
                .ok()?;
            let binding = &self.prefix.get(found)?.binding;
            return Some(use_binding(binding.store.as_ref(), binding.index));
        }
        if index >= self.len() {
            return None;
        }
        if self.suffix_overrides.is_empty() {
            return Some(use_binding(
                self.suffix_store.as_deref()?,
                index - self.prefix_len,
            ));
        }
        if let Ok(found) = self
            .suffix_overrides
            .binary_search_by_key(&index, |entry| entry.slot)
        {
            let binding = &self.suffix_overrides.get(found)?.binding;
            return Some(use_binding(binding.store.as_ref(), binding.index));
        }
        Some(use_binding(
            self.suffix_store.as_deref()?,
            index - self.prefix_len,
        ))
    }

    fn shared_prefix(&self) -> Rc<[CapturedRef]> {
        if self.suffix_len == 0 {
            return Rc::clone(&self.prefix);
        }
        self.prefix
            .iter()
            .cloned()
            .chain(
                (self.prefix_len..self.len())
                    .filter_map(|slot| self.get(slot).map(|binding| CapturedRef { slot, binding })),
            )
            .collect::<Vec<_>>()
            .into()
    }

    fn push(&mut self, binding: BindingRef) {
        let slot = self.len();
        self.suffix_len += 1;
        self.suffix_overrides.push(CapturedRef { slot, binding });
    }

    fn replace(&mut self, index: usize, binding: BindingRef) {
        if index < self.prefix_len {
            let mut prefix = self.prefix.to_vec();
            match prefix.binary_search_by_key(&index, |capture| capture.slot) {
                Ok(found) => prefix[found].binding = binding,
                Err(insert) => prefix.insert(
                    insert,
                    CapturedRef {
                        slot: index,
                        binding,
                    },
                ),
            }
            self.prefix = prefix.into();
            return;
        }
        match self
            .suffix_overrides
            .binary_search_by_key(&index, |entry| entry.slot)
        {
            Ok(found) => self.suffix_overrides[found].binding = binding,
            Err(insert) => self.suffix_overrides.insert(
                insert,
                CapturedRef {
                    slot: index,
                    binding,
                },
            ),
        }
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

    fn copy_from_register(
        &self,
        registers: &crate::register_file::RegisterFile,
        source: u16,
    ) -> bool {
        self.store.copy_from_register(self.index, registers, source)
    }

    fn existing_cell(&self) -> Option<Rc<crate::value::BindingCell>> {
        self.store.existing_cell(self.index)
    }

    fn update_number(&self, delta: f64) -> Option<(f64, f64)> {
        self.store.update_number(self.index, delta)
    }

    fn cell(&self) -> Rc<crate::value::BindingCell> {
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
    tdz_parent: Option<Rc<Self>>,
}

// Call frames are short-lived and overwhelmingly non-escaping. Retain a
// small isolate-local pool of their containers so ordinary calls reuse the
// same slot storage instead of asking the allocator for a fresh Rc, RefCell,
// and SlotStore on every invocation. Escaping frames never re-enter this
// pool because recycle requires a unique Rc owner.
thread_local! {
    static FRAME_POOL: RefCell<Vec<Rc<Environment>>> = const { RefCell::new(Vec::new()) };
}

impl Drop for Environment {
    fn drop(&mut self) {
        crate::execution_trace::environment_lifecycle(false);
    }
}

fn immutable_prefix(source: &RefCell<Option<HashSet<u16>>>, limit: usize) -> Option<HashSet<u16>> {
    source.borrow().as_ref().map(|slots| {
        slots
            .iter()
            .copied()
            .filter(|slot| usize::from(*slot) < limit)
            .collect()
    })
}

impl Environment {
    pub(crate) fn binding_cells(&self) -> Vec<Rc<crate::value::BindingCell>> {
        fn collect(
            environment: &Environment,
            seen: &mut std::collections::HashSet<usize>,
            output: &mut Vec<Rc<crate::value::BindingCell>>,
        ) {
            for index in 0..environment.captured_len() {
                let Some(binding) = environment.slot(index as u16) else {
                    continue;
                };
                let cell = binding.cell();
                if seen.insert(Rc::as_ptr(&cell) as usize) {
                    output.push(cell);
                }
            }
            for names in [&environment.names, &environment.eval_names] {
                if let Some(names) = names.borrow().as_ref() {
                    for binding in names.values() {
                        let cell = binding.cell();
                        if seen.insert(Rc::as_ptr(&cell) as usize) {
                            output.push(cell);
                        }
                    }
                }
            }
            if let Some(caller) = &environment.caller {
                collect(caller, seen, output);
            }
        }
        let mut seen = std::collections::HashSet::new();
        let mut output = Vec::new();
        collect(self, &mut seen, &mut output);
        output
    }

    /// Borrow the immutable slot map without paying `RefCell`'s dynamic
    /// borrow check on every proven local read.  The VM is single-threaded;
    /// mutations still go through `slots.borrow_mut()` at the few semantic
    /// boundaries that can resize or replace a binding.
    #[inline(always)]
    fn slots_ref(&self) -> &SlotRefs {
        // SAFETY: all VM execution is confined to one thread. Callers using
        // this helper are read-only and do not overlap a `borrow_mut()` call.
        unsafe { &*self.slots.as_ptr() }
    }

    pub(crate) fn new() -> Rc<Self> {
        crate::execution_trace::environment_lifecycle(true);
        Rc::new(Self::default())
    }

    pub(crate) fn capture(environment: &Rc<Self>, count: u16) -> Rc<Self> {
        let count = usize::from(count);
        for index in 0..count {
            environment.ensure_slot(index as u16);
        }
        let refs: Rc<[CapturedRef]> = (0..count)
            .filter_map(|index| {
                environment.slot(index as u16).map(|binding| CapturedRef {
                    slot: index,
                    binding,
                })
            })
            .collect::<Vec<_>>()
            .into();
        crate::execution_trace::environment_lifecycle(true);
        Rc::new(Self {
            slots: RefCell::new(SlotRefs::from_prefix(count, refs)),
            names: RefCell::new(environment.names.borrow().clone()),
            eval_names: RefCell::new(environment.eval_names.borrow().clone()),
            immutable_names: RefCell::new(environment.immutable_names.borrow().clone()),
            immutable_slots: RefCell::new(immutable_prefix(&environment.immutable_slots, count)),
            uninitialized: RefCell::new(environment.uninitialized.borrow().clone()),
            deleted_cells: RefCell::new(environment.deleted_cells.borrow().clone()),
            caller: Some(Rc::clone(environment)),
            tdz_parent: Some(Rc::clone(environment)),
        })
    }

    pub(crate) fn capture_selected(
        environment: &Rc<Self>,
        count: u16,
        selected: &[u16],
    ) -> Rc<Self> {
        if selected.contains(&u16::MAX) {
            return Self::capture(environment, count);
        }
        let count_usize = usize::from(count);
        for slot in selected
            .iter()
            .copied()
            .filter(|slot| usize::from(*slot) < count_usize)
        {
            environment.ensure_slot(slot);
        }
        let refs = selected
            .iter()
            .copied()
            .filter(|slot| usize::from(*slot) < count_usize)
            .filter_map(|slot| {
                environment.slot(slot).map(|binding| CapturedRef {
                    slot: usize::from(slot),
                    binding,
                })
            })
            .collect::<Vec<_>>()
            .into();
        crate::execution_trace::environment_lifecycle(true);
        Rc::new(Self {
            slots: RefCell::new(SlotRefs::from_prefix(count_usize, refs)),
            names: RefCell::new(environment.names.borrow().clone()),
            eval_names: RefCell::new(environment.eval_names.borrow().clone()),
            immutable_names: RefCell::new(environment.immutable_names.borrow().clone()),
            immutable_slots: RefCell::new(Some(
                environment
                    .immutable_slots
                    .borrow()
                    .as_ref()
                    .map(|slots| {
                        slots
                            .iter()
                            .copied()
                            .filter(|slot| selected.contains(slot))
                            .collect()
                    })
                    .unwrap_or_default(),
            )),
            uninitialized: RefCell::new(environment.uninitialized.borrow().clone()),
            deleted_cells: RefCell::new(environment.deleted_cells.borrow().clone()),
            caller: None,
            tdz_parent: Some(Rc::clone(environment)),
        })
    }

    pub(crate) fn child(captures: &Rc<Self>, values: Vec<Value>) -> Rc<Self> {
        Self::child_registers(
            captures,
            crate::register_file::RegisterFile::from_values(values),
        )
    }

    pub(crate) fn child_registers(
        captures: &Rc<Self>,
        values: crate::register_file::RegisterFile,
    ) -> Rc<Self> {
        crate::execution_trace::environment_child(captures.len(), values.len());
        let store = SlotStore::from_registers(values);
        let prefix = captures.slots_ref().shared_prefix();
        let prefix_len = captures.len();
        let suffix_len = store.len();
        let state = Self {
            slots: RefCell::new(SlotRefs {
                prefix_len,
                prefix,
                suffix_store: Some(store),
                suffix_len,
                suffix_overrides: Vec::new(),
            }),
            names: RefCell::new(None),
            eval_names: RefCell::new(None),
            immutable_names: RefCell::new(None),
            immutable_slots: RefCell::new(None),
            uninitialized: RefCell::new(None),
            deleted_cells: RefCell::new(None),
            caller: Some(Rc::clone(captures)),
            tdz_parent: Some(Rc::clone(captures)),
        };
        let environment = FRAME_POOL.with(|pool| {
            let mut pool = pool.borrow_mut();
            let Some(mut environment) = pool.pop() else {
                crate::execution_trace::environment_lifecycle(true);
                return Rc::new(state);
            };
            let Some(slot) = Rc::get_mut(&mut environment) else {
                crate::execution_trace::environment_lifecycle(true);
                return Rc::new(state);
            };
            *slot = state;
            environment
        });
        environment
            .deleted_cells
            .replace(captures.deleted_cells.borrow().clone());
        environment
            .immutable_slots
            .replace(immutable_prefix(&captures.immutable_slots, prefix_len));
        environment
    }

    pub(crate) fn recycle_frame(environment: Rc<Self>) {
        if Rc::strong_count(&environment) != 1 {
            return;
        }
        FRAME_POOL.with(|pool| {
            let mut pool = pool.borrow_mut();
            if pool.len() < 64 {
                pool.push(environment);
            }
        });
    }

    pub(crate) fn in_place_child(captures: &Rc<Self>, values: Vec<Value>) -> Rc<Self> {
        Self::child(captures, values)
    }
    pub(crate) fn len(&self) -> usize {
        self.slots_ref().len()
    }

    pub(crate) fn captured_len(&self) -> usize {
        self.slots_ref().prefix_len
    }

    pub(crate) fn get(&self, slot: u16) -> Value {
        self.slot(slot).map_or(Value::Undefined, |slot| slot.load())
    }

    pub(crate) fn get_number(&self, slot: u16) -> Option<f64> {
        self.slot(slot)?.load_number()
    }

    pub(crate) fn load_into(
        &self,
        registers: &mut crate::register_file::RegisterFile,
        dst: u16,
        slot: u16,
    ) {
        let slots = self.slots_ref();
        if slots
            .with_binding(usize::from(slot), |store, index| {
                store.load_into(registers, dst, index)
            })
            .is_none()
        {
            crate::execute::write_value(registers, dst, Value::Undefined);
        }
    }

    /// Copy a proven lexical slot without cloning its `Rc<SlotStore>` map
    /// entry. A missing slot is reported so the caller can retain complete
    /// dynamic semantics through the ordinary path.
    #[inline(always)]
    pub(crate) fn load_proven_into(
        &self,
        registers: &mut crate::register_file::RegisterFile,
        dst: u16,
        slot: u16,
    ) -> bool {
        let slots = self.slots_ref();
        slots
            .with_binding(usize::from(slot), |store, index| {
                store.load_existing_into(registers, dst, index)
            })
            .is_some()
    }

    #[inline(always)]
    pub(crate) fn load_existing_proven_into(
        &self,
        registers: &mut crate::register_file::RegisterFile,
        dst: u16,
        slot: u16,
    ) -> bool {
        let slots = self.slots_ref();
        slots
            .with_binding(usize::from(slot), |store, index| {
                store.load_existing_into(registers, dst, index)
            })
            .is_some()
    }

    #[inline(always)]
    pub(crate) fn has_proven_slot(&self, slot: u16) -> bool {
        self.slots_ref()
            .with_binding(usize::from(slot), |_, _| ())
            .is_some()
    }

    #[inline(always)]
    pub(crate) fn copy_proven_from_register(
        &self,
        slot: u16,
        registers: &crate::register_file::RegisterFile,
        source: u16,
    ) -> bool {
        let slots = self.slots_ref();
        slots
            .with_binding(usize::from(slot), |store, index| {
                store.copy_from_register(index, registers, source)
            })
            .unwrap_or(false)
    }

    /// Resolve a move-only loop site's physical word operands once. The
    /// returned plan is valid while this Environment remains installed and no
    /// non-Move instruction can resize the involved stores.
    pub(crate) fn plan_immediate_move(
        &self,
        source: u16,
        target: u16,
    ) -> Option<crate::register_file::ImmediateCopyPlan> {
        let slots = self.slots_ref();
        let source = slots
            .with_binding(usize::from(source), SlotStore::immediate_word_ptr)
            .flatten()?;
        let target = slots
            .with_binding(usize::from(target), SlotStore::immediate_word_ptr)
            .flatten()?;
        Some(crate::register_file::ImmediateCopyPlan::new(source, target))
    }

    pub(crate) fn load_into_fixed<const N: usize>(
        &self,
        registers: &mut crate::register_file::FixedWordFile<N>,
        dst: usize,
        slot: u16,
    ) -> bool {
        let slots = self.slots_ref();
        slots
            .with_binding(usize::from(slot), |store, index| {
                store.load_into_fixed(registers, dst, index)
            })
            .unwrap_or(false)
    }

    pub(crate) fn set(&self, slot: u16, value: Value) {
        {
            let binding = self.ensure_slot(slot);
            binding.store(value);
        }
        self.initialize(slot);
    }

    pub(crate) fn copy_from_register(
        &self,
        slot: u16,
        registers: &crate::register_file::RegisterFile,
        source: u16,
    ) -> bool {
        let binding = self.ensure_slot(slot);
        let copied = binding.copy_from_register(registers, source);
        if copied {
            self.initialize(slot);
        }
        copied
    }

    pub(crate) fn update_number(&self, slot: u16, delta: f64) -> Option<(f64, f64)> {
        self.slot(slot)?.update_number(delta)
    }

    fn slot(&self, slot: u16) -> Option<BindingRef> {
        self.slots_ref().get(usize::from(slot))
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

    pub(crate) fn slot_cell(&self, slot: u16) -> Rc<crate::value::BindingCell> {
        self.ensure_slot(slot).cell()
    }

    pub(crate) fn install_slot_cell(&self, slot: u16, cell: Rc<crate::value::BindingCell>) {
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

    pub(crate) fn alias_binding(&self, name: &str, binding: Rc<crate::value::BindingCell>) {
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

    pub(crate) fn clear_immutable_slot(&self, slot: u16) {
        if let Some(slots) = self.immutable_slots.borrow_mut().as_mut() {
            slots.remove(&slot);
        }
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

    pub(crate) fn snapshot_eval_name_chain(&self) -> Vec<Option<HashMap<String, BindingRef>>> {
        let mut snapshots = vec![self.eval_names.borrow().clone()];
        if let Some(caller) = &self.caller {
            snapshots.extend(caller.snapshot_eval_name_chain());
        }
        snapshots
    }

    pub(crate) fn restore_eval_name_chain(
        &self,
        snapshots: &[Option<HashMap<String, BindingRef>>],
    ) {
        let Some((current, rest)) = snapshots.split_first() else {
            return;
        };
        self.eval_names.replace(current.clone());
        if let Some(caller) = &self.caller {
            caller.restore_eval_name_chain(rest);
        }
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
        self.caller
            .as_ref()
            .is_some_and(|caller| caller.eval_name_aliases_slot(name, slot))
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
    pub(crate) fn is_deleted(&self, cell: &Rc<crate::value::BindingCell>) -> bool {
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

    pub(crate) fn is_deleted_slot(&self, slot: u16) -> bool {
        if usize::from(slot) >= self.captured_len() && self.deleted_cells.borrow().is_none() {
            return false;
        }
        let Some(cell) = self.slot(slot).and_then(|binding| binding.existing_cell()) else {
            return false;
        };
        self.is_deleted(&cell)
    }

    fn mark_deleted_cell(&self, cell: Rc<crate::value::BindingCell>) {
        let mut state = self.deleted_cells.borrow_mut();
        let cells = state.get_or_insert_with(|| Rc::new(DeletedCells(UnsafeCell::new(Vec::new()))));
        // SAFETY: VM execution is single-threaded; no aliased mutable access
        // occurs while this method runs.
        unsafe { &mut *cells.0.get() }.push(cell);
    }

    pub(crate) fn mark_deleted_slot(&self, slot: u16) {
        if let Some(cell) = self.slot(slot).and_then(|binding| binding.existing_cell()) {
            self.mark_deleted_cell(cell);
        }
    }
    fn clear_deleted_cell(&self, cell: &Rc<crate::value::BindingCell>) {
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
        if usize::from(slot) >= self.captured_len() && self.uninitialized.borrow().is_none() {
            return false;
        }
        let local = self
            .uninitialized
            .borrow()
            .as_ref()
            .is_some_and(|slots| slots.contains(slot));
        local
            || (usize::from(slot) < self.captured_len()
                && self
                    .tdz_parent
                    .as_ref()
                    .is_some_and(|parent| parent.is_uninitialized(slot)))
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
        if index < slots.prefix_len {
            let store = SlotStore::from_values(vec![Value::Undefined]);
            let binding = BindingRef::new(store, 0);
            slots.replace(index, binding.clone());
            return binding;
        }
        while slots.len() <= index {
            let store = SlotStore::from_values(vec![Value::Undefined]);
            slots.push(BindingRef::new(store, 0));
        }
        slots.get(index).expect("ensured environment slot")
    }

    pub(crate) fn initialize(&self, slot: u16) {
        if let Some(slots) = self.uninitialized.borrow().as_ref() {
            slots.remove(slot);
        }
        if usize::from(slot) < self.captured_len() {
            if let Some(parent) = self.tdz_parent.as_ref() {
                parent.initialize(slot);
            }
        }
    }
    fn shared_tdz(&self) -> Rc<TdzCells> {
        let mut state = self.uninitialized.borrow_mut();
        let slots = state.get_or_insert_with(TdzCells::new);
        Rc::clone(slots)
    }

    fn initialize_binding(&self, binding: &BindingRef) {
        let slot = (0..self.slots_ref().len()).find(|slot| {
            self.slot(*slot as u16)
                .is_some_and(|candidate| candidate.same(binding))
        });
        if let Some(slot) = slot.and_then(|slot| u16::try_from(slot).ok()) {
            self.initialize(slot);
        }
    }

    pub(crate) fn replace_slot(&self, slot: u16, value: Value) -> Rc<crate::value::BindingCell> {
        let previous = self.ensure_slot(slot);
        let store = SlotStore::from_values(vec![value]);
        self.slots
            .borrow_mut()
            .replace(usize::from(slot), BindingRef::new(store, 0));
        self.initialize(slot);
        previous.cell()
    }

    pub(crate) fn restore_slot(&self, slot: u16, value: Rc<crate::value::BindingCell>) {
        let binding = BindingRef::new(SlotStore::from_cell(value), 0);
        self.slots.borrow_mut().replace(usize::from(slot), binding);
    }

    pub(crate) fn has_caller(&self) -> bool {
        self.caller.is_some()
    }

    pub(crate) fn replace_value(&self, old: &Value, new: &Value) {
        if let Some(caller) = &self.caller {
            caller.replace_value(old, new);
        }
        for index in 0..self.slots_ref().len() {
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
        let cell = crate::value::BindingCell::new(Value::Number(4.0));
        let store = SlotStore::from_cell(std::rc::Rc::clone(&cell));
        cell.store(Value::Number(9.0));
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
        std::hint::black_box((raw, slot, checked, current));
    }
}
