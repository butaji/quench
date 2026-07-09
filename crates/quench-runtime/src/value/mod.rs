//! JavaScript runtime values - HIR (High-level IR)
//!
//! This module defines the value types used by the interpreter.
//! The key design decisions:
//! - Objects use prototype chain for inheritance
//! - Functions have interior mutability (RefCell) for prototype caching
//! - Values are immutable reference-counted handles

pub mod convert;
pub mod error;
pub mod function;
pub mod kind;
pub mod object;
mod val;

pub use convert::{to_js_string, to_bool, to_number, strict_eq, loose_eq, to_primitive, PrimitiveHint};
pub use error::{
    JsError, set_thrown_value, take_thrown_value,
};
pub use function::{NativeFunction, NativeConstructor, ValueFunction};
pub use kind::ObjectKind;
pub use object::{Getter, GetterStorage, Object, Setter, SetterStorage};
pub use val::{ClassValue, Value};

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_object_keys_insertion_order() {
        let mut obj = Object::new(ObjectKind::Ordinary);
        obj.set("a", Value::Number(1.0));
        obj.set("b", Value::Number(2.0));
        obj.set("c", Value::Number(3.0));
        assert_eq!(obj.own_keys(), vec!["a", "b", "c"]);
    }

    #[test]
    fn test_object_keys_delete_readd_moves_to_end() {
        let mut obj = Object::new(ObjectKind::Ordinary);
        obj.set("a", Value::Number(1.0));
        obj.set("b", Value::Number(2.0));
        obj.set("c", Value::Number(3.0));
        obj.delete("b");
        obj.set("b", Value::Number(4.0));
        assert_eq!(obj.own_keys(), vec!["a", "c", "b"]);
    }

    #[test]
    fn test_object_keys_numeric_first() {
        let mut obj = Object::new(ObjectKind::Ordinary);
        obj.properties.insert("c".to_string(), Value::Number(1.0));
        obj.properties.insert("10".to_string(), Value::Number(2.0));
        obj.properties.insert("a".to_string(), Value::Number(3.0));
        obj.properties.insert("2".to_string(), Value::Number(4.0));
        obj.properties.insert("b".to_string(), Value::Number(5.0));
        assert_eq!(obj.own_keys(), vec!["2", "10", "c", "a", "b"]);
    }
}
