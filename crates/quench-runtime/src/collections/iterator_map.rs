use crate::value::{MapData, Value};

pub(super) fn step(data: &MapData, index: &mut usize, kind: &u8, done: &mut bool) -> Option<Value> {
    if *done {
        return None;
    }
    let key = data.keys.borrow().get(*index).cloned();
    let value = data.values.borrow().get(*index).cloned();
    let result = match (key, value, kind) {
        (Some(key), Some(value), 0) => Some(Value::array(vec![key, value])),
        (Some(key), Some(_), 1) => Some(key),
        (Some(_), Some(value), 2) => Some(value),
        _ => None,
    };
    if result.is_some() {
        *index += 1;
    } else {
        *done = true;
    }
    result
}
