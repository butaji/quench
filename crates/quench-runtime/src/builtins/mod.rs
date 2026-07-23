//! Built-in JavaScript objects and functions

// Re-export submodules
pub mod array;
pub mod array_buffer;
pub mod bigint;
pub mod console;
pub mod date;
pub mod error;
pub mod function;
pub mod json;
pub mod map;
pub mod math;
pub mod number;
pub mod object;
pub mod object_static;
pub mod promise;
pub mod reflect;
pub mod regex; // regex module includes string_methods submodule
pub mod string;
pub mod symbol;
pub mod typed_array;
pub mod uri;
pub mod weak;

// Re-export the public items from submodules
pub use array::get_array_prototype;
pub use function::{get_function_prototype, get_restricted_prop_error, is_function_prototype};
pub use object::get_object_prototype;
pub use promise::execute_pending_microtasks;
pub use typed_array::get_typed_array_prototype;

// Re-export get_native_this for use by submodules
pub(crate) use crate::interpreter::get_native_this;
pub(crate) use crate::interpreter::get_this_value;

use serde::ser::{SerializeMap, SerializeSeq};

// ============================================================================
// JsValueProxy — serde serializer for JS values
// ============================================================================

#[allow(dead_code)]
pub(crate) struct JsValueProxy<'a>(&'a crate::value::Value);

impl serde::Serialize for JsValueProxy<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
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
            #[allow(unused_variables)]
            Value::Function(_) => serializer.serialize_str("[Function]"),
            Value::NativeFunction(_) => serializer.serialize_str("[Function]"),
            Value::NativeConstructor(_) => serializer.serialize_str("[Function]"),
            Value::Symbol(s) => {
                serializer.serialize_str(&format!("Symbol({})", s.desc.as_deref().unwrap_or("")))
            }
            #[allow(unused_variables)]
            Value::Class(_) => serializer.serialize_str("[Function]"),
            Value::BigInt(bi) => serializer.serialize_str(&format!("{}n", bi)),
            Value::Generator(ref gen) => {
                let state = gen.borrow().state.clone();
                let label = match state {
                    crate::value::generator::GeneratorState::Suspended => "Generator (suspended)",
                    crate::value::generator::GeneratorState::Running => "Generator (running)",
                    crate::value::generator::GeneratorState::Completed => "Generator (completed)",
                };
                serializer.serialize_str(label)
            }
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
        obj.properties
            .insert("length".to_string(), Value::Number(items.len() as f64));
        if let Some(proto) = crate::builtins::array::get_array_prototype() {
            obj.prototype = Some(proto);
        }
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
    // Symbol must be registered before Map/Set so their prototypes can carry
    // the Symbol.iterator method.
    symbol::register_symbol(ctx);
    map::register_map_and_set(ctx);
    // WeakMap and WeakSet
    weak::register_weak_collections(ctx);
    // String must be registered for string support
    string::register_string(ctx);
    // Number must be registered before Date (for timestamp conversion)
    number::register_number(ctx);
    bigint::register_bigint(ctx);
    date::register_global_functions(ctx);
    function::register_function(ctx);
    error::register_error(ctx);
    // Date needs to be registered after global functions (for Number, String, etc.)
    date::register_date(ctx);
    // Promise needs to be registered for async support
    promise::register_promise(ctx);
    // RegExp needs to be registered for regex support
    regex::register_regexp(ctx);
    // String regex methods need to be registered after RegExp
    regex::register_string_regex_methods(ctx);
    // Minimal Reflect (ownKeys) — needed by the test262 harness
    reflect::register_reflect(ctx);
    // ArrayBuffer and typed-array constructors are needed by harness utilities.
    array_buffer::register_array_buffer(ctx);
    typed_array::register_typed_arrays(ctx);
    // Global URI / parseInt / parseFloat / isNaN / isFinite functions
    uri::register_uri(ctx);
    // Array.prototype[Symbol.iterator] requires Symbol to be registered first.
    array::register_array_iterator();
}

use crate::value::{Object, ObjectKind, Value};
use crate::Context;
