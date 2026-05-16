//! Generated from Rune source

use protocol::{AppState, Filter, Task};


#[derive(Clone, Debug)]
pub struct RawTask {
    pub id: f64,
    pub title: String,
    pub done: bool,
}


#[derive(Clone, Debug)]
pub struct JsonValue {

}


/// Function: parseJson
pub fn parse_json(data: String) -> () {
    // TODO: implement this function
}


/// Function: deserializeTasks
pub fn deserialize_tasks(data: String) -> () {
    // TODO: implement this function
}


/// Function: mergeTasks
pub fn merge_tasks(local: Vec<Task>, remote: Vec<Task>) -> () {
    // TODO: implement this function
}


/// Function: isString
pub fn is_string(val: JsonValue) -> () {
    // TODO: implement this function
}


/// Function: serializeTasks
pub fn serialize_tasks(tasks: Vec<Task>) -> () {
    // TODO: implement this function
}


/// Function: isNumber
pub fn is_number(val: JsonValue) -> () {
    // TODO: implement this function
}


/// Function: validateTask
pub fn validate_task(task: RawTask) -> () {
    // TODO: implement this function
}


/// Function: isBoolean
pub fn is_boolean(val: JsonValue) -> () {
    // TODO: implement this function
}


/// Function: isObject
pub fn is_object(val: JsonValue) -> () {
    // TODO: implement this function
}

// End of generated code
