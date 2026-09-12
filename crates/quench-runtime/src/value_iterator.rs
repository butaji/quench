#[derive(Debug, PartialEq)]
pub struct IteratorData {
    pub state: RefCell<IteratorState>,
    pub(crate) properties: RefCell<Vec<(String, Value)>>,
    pub(crate) descriptors: RefCell<Vec<(String, Value)>>,
    pub executing: RefCell<bool>,
    pub in_return: RefCell<bool>,
}

impl IteratorData {
    pub fn new(state: IteratorState) -> Self {
        Self {
            state: RefCell::new(state),
            properties: RefCell::new(Vec::new()),
            descriptors: RefCell::new(Vec::new()),
            executing: RefCell::new(false),
            in_return: RefCell::new(false),
        }
    }

    pub(crate) fn property(&self, key: &str) -> Option<Value> {
        self.properties
            .borrow()
            .iter()
            .rev()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.clone())
    }

    pub(crate) fn set_property(&self, key: &str, value: Value) {
        let mut properties = self.properties.borrow_mut();
        if let Some((_, current)) = properties.iter_mut().rev().find(|(name, _)| name == key) {
            *current = value;
        } else {
            properties.push((key.to_string(), value));
        }
    }

    pub(crate) fn set_descriptor(&self, key: &str, descriptor: Value) {
        let mut descriptors = self.descriptors.borrow_mut();
        if let Some((_, current)) = descriptors.iter_mut().rev().find(|(name, _)| name == key) {
            *current = descriptor;
        } else {
            descriptors.push((key.to_string(), descriptor));
        }
    }

    pub(crate) fn descriptor(&self, key: &str) -> Option<Value> {
        self.descriptors
            .borrow()
            .iter()
            .rev()
            .find(|(name, _)| name == key)
            .map(|(_, descriptor)| descriptor.clone())
    }
}

#[derive(Debug, PartialEq)]
pub enum IteratorState {
    Concat {
        items: Vec<(Value, Value)>,
        opened: Vec<Option<Value>>,
        index: usize,
        current: Option<Value>,
        done: bool,
    },
    Zip {
        iterators: Vec<Value>,
        padding: Vec<Value>,
        mode: u8,
        keys: Option<Vec<String>>,
        started: bool,
        done: bool,
    },
    Mapped {
        iterator: Value,
        mapper: Value,
        index: usize,
        done: bool,
    },
    FlatMapped {
        inner: Value,
        mapper: Value,
        index: usize,
        current: Option<Value>,
        done: bool,
    },
    Filtered {
        iterator: Value,
        predicate: Value,
        index: usize,
        done: bool,
    },
    Dropped {
        inner: Value,
        skipped: usize,
        limit: usize,
        done: bool,
    },
    Take {
        inner: Value,
        remaining: u64,
    },
    Native {
        values: Vec<Value>,
        receiver: Option<Rc<crate::value::ArrayData>>,
        typed_receiver: Option<Value>,
        typed_keys: bool,
        entries: bool,
        keys: bool,
        index: usize,
        done: bool,
    },
    String {
        input: Vec<u16>,
        index: usize,
        done: bool,
    },
    Set {
        data: Rc<SetData>,
        index: usize,
        kind: u8,
        done: bool,
    },
    Map {
        data: Rc<MapData>,
        index: usize,
        kind: u8,
        done: bool,
    },
    Protocol {
        iterator: Value,
        next: Value,
        done: bool,
        await_value: bool,
    },
    RegExpString {
        regexp: Value,
        input: Value,
        global: bool,
        unicode: bool,
        done: bool,
    },
}
