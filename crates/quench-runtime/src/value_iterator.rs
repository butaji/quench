#[derive(Debug, PartialEq)]
pub struct IteratorData {
    pub state: RefCell<IteratorState>,
    pub executing: RefCell<bool>,
    pub in_return: RefCell<bool>,
}

impl IteratorData {
    pub fn new(state: IteratorState) -> Self {
        Self {
            state: RefCell::new(state),
            executing: RefCell::new(false),
            in_return: RefCell::new(false),
        }
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
        mode: u8,
        padding: Value,
        padding_values: Vec<Value>,
        started: bool,
        done: bool,
    },
    ZipKeyed {
        keys: Vec<String>,
        iterators: Vec<Value>,
        mode: u8,
        padding: Value,
        padding_values: Vec<Value>,
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
        limit_value: Option<Value>,
        done: bool,
    },
    Take {
        inner: Value,
        remaining: u64,
        limit_value: Option<Value>,
        done: bool,
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
        input: String,
        global: bool,
        unicode: bool,
        done: bool,
    },
}
