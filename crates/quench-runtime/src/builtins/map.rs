//! Map and Set built-ins

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{NativeFunction, Object, ObjectKind, Value};
use crate::Context;

// ============================================================================
// Map and Set
// ============================================================================

pub fn register_map_and_set(ctx: &mut Context) {
    let map_constructor = Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        let map_obj = Object::new(ObjectKind::Map);
        let map = Rc::new(RefCell::new(map_obj));
        let entries = Object::new_array(0);
        map.borrow_mut().set("_entries", Value::Object(Rc::new(RefCell::new(entries))));
        Ok(Value::Object(map))
    })));
    ctx.set_global("Map".to_string(), map_constructor);

    let set_constructor = Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        let set_obj = Object::new(ObjectKind::Set);
        Ok(Value::Object(Rc::new(RefCell::new(set_obj))))
    })));
    ctx.set_global("Set".to_string(), set_constructor);
}
