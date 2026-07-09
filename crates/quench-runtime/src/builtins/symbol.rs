//! Symbol built-in

use std::rc::Rc;

use crate::value::{NativeFunction, Value};
use crate::Context;

/// Symbol counter for unique IDs
static mut SYMBOL_COUNTER: usize = 0;

/// Create a unique symbol description
fn next_symbol_desc() -> usize {
    unsafe {
        SYMBOL_COUNTER += 1;
        SYMBOL_COUNTER
    }
}

/// Register Symbol constructor and static methods
pub fn register_symbol(ctx: &mut Context) {
    // Symbol constructor function
    let symbol_constructor = NativeFunction::new(move |args| {
        let desc = args.first()
            .map(crate::value::to_js_string)
            .unwrap_or_default();
        let symbol_id = next_symbol_desc();
        let symbol_val = Value::Symbol(format!("Symbol({}):{}", desc, symbol_id));
        Ok(symbol_val)
    });

    ctx.set_global("Symbol".to_string(), Value::NativeFunction(Rc::new(symbol_constructor)));
}

/// Check if a value is a symbol
pub fn is_symbol(val: &Value) -> bool {
    matches!(val, Value::Symbol(_))
}
