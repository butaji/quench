#[derive(Debug, PartialEq)]
pub struct IteratorData {
    pub state: RefCell<IteratorState>,
}

#[derive(Debug, PartialEq)]
pub enum IteratorState {
    Native { values: Vec<Value>, index: usize, done: bool },
    Set { data: Rc<SetData>, index: usize, kind: u8, done: bool },
    Map { data: Rc<MapData>, index: usize, kind: u8, done: bool },
    Protocol { iterator: Value, next: Value, done: bool, executing: bool },
    Concat { items: Vec<(Value, Value)>, index: usize, current: Option<Value>, done: bool, executing: bool },
    Drop { iterator: Value, remaining: u64, done: bool },
    MapHelper { iterator: Value, callback: Value, index: u64, done: bool, executing: bool },
    FilterHelper { iterator: Value, callback: Value, index: u64, done: bool, executing: bool },
    Take { iterator: Value, remaining: u64, done: bool },
    RegExpString { regexp: Value, input: String, global: bool, unicode: bool, done: bool },
}
