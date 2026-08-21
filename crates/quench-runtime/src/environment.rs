use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

use crate::module_bindings::ModuleBindingCell;
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
    eval_names: RefCell<Option<HashMap<String, BindingRef>>>,
    immutable_names: RefCell<Option<HashSet<String>>>,
    immutable_slots: RefCell<Option<HashSet<u16>>>,
    uninitialized: RefCell<Option<Rc<RefCell<HashSet<u16>>>>>,
    caller: Option<Rc<Self>>,
}

include!("environment_tail.rs");
