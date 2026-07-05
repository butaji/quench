//! Built-in JavaScript objects and functions

// Re-export submodules
pub mod console;
pub mod json;
pub mod math;
pub mod object;
pub mod array;
pub mod map;
pub mod date;
pub mod error;
pub mod function;
pub mod number;
pub mod symbol;

// Re-export the public items from submodules
pub use array::get_array_prototype;
pub use object::get_object_prototype;
pub use function::get_function_prototype;

// Re-export get_native_this for use by submodules
pub(crate) use crate::interpreter::get_native_this;
use serde::ser::{SerializeMap, SerializeSeq};

// ============================================================================
// JsValueProxy — serde serializer for JS values
// ============================================================================

pub(crate) struct JsValueProxy<'a>(&'a crate::value::Value);

impl serde::Serialize for JsValueProxy<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: serde::Serializer {
        use crate::value::Value;
        match self.0 {
            Value::Undefined => serializer.serialize_unit(),
            Value::Null => serializer.serialize_unit(),
            Value::Boolean(b) => serializer.serialize_bool(*b),
            Value::Number(n) => serializer.serialize_f64(*n),
            Value::String(s) => serializer.serialize_str(s),
            Value::Object(obj_rc) => {
                let obj = obj_rc.borrow();

                // Check if it's an array (has numeric indices and length)
                if obj.kind == ObjectKind::Array || !obj.elements.is_empty() {
                    // Serialize as array
                    let mut seq = serializer.serialize_seq(Some(obj.elements.len()))?;
                    for val in &obj.elements {
                        seq.serialize_element(&JsValueProxy(val))?;
                    }
                    seq.end()
                } else {
                    // Serialize as object - collect own properties only
                    let mut map = serializer.serialize_map(Some(obj.properties.len()))?;
                    for (key, val) in &obj.properties {
                        // Skip internal properties
                        if key.starts_with("__") || key == "constructor" || key == "prototype" {
                            continue;
                        }
                        map.serialize_entry(key, &JsValueProxy(val))?;
                    }
                    map.end()
                }
            }
            #[allow(unused_variables)] Value::Function(_) => serializer.serialize_str("[Function]"),
            Value::NativeFunction(_) => serializer.serialize_str("[Function]"),
            Value::NativeConstructor(_) => serializer.serialize_str("[Function]"),
            Value::Symbol(s) => serializer.serialize_str(&format!("Symbol({})", s)),
        }
    }
}

// ============================================================================
// Object helper: new_array_from
// ============================================================================

impl Object {
    /// Create a new array from a list of values
    pub(crate) fn new_array_from(items: Vec<Value>) -> Self {
        let mut obj = Object::new(ObjectKind::Array);
        obj.elements = items.clone();
        obj.properties.insert("length".to_string(), Value::Number(items.len() as f64));
        obj
    }
}

// ============================================================================
// Register all built-ins
// ============================================================================

/// Register all built-in globals into the context
pub fn register_builtins(ctx: &mut Context) {
    console::register_console(ctx);
    json::register_json(ctx);
    math::register_math(ctx);
    object::register_object(ctx);
    array::register_array(ctx);
    map::register_map_and_set(ctx);
    // Number must be registered before Date (for timestamp conversion)
    number::register_number(ctx);
    date::register_global_functions(ctx);
    function::register_function(ctx);
    error::register_error(ctx);
    // Date needs to be registered after global functions (for Number, String, etc.)
    date::register_date(ctx);
    // Symbol needs to be registered for symbol support
    symbol::register_symbol(ctx);
}

use crate::Context;
use crate::value::{Object, ObjectKind, Value};
