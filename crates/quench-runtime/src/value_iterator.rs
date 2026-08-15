#[derive(Debug, PartialEq)]
pub struct IteratorData {
    pub state: RefCell<IteratorState>,
}

#[derive(Debug, PartialEq)]
pub enum IteratorState {
    Native {
        values: Vec<Value>,
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
    },
    RegExpString {
        regexp: Value,
        input: String,
        global: bool,
        unicode: bool,
        done: bool,
    },
}
