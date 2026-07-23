//! JavaScript runtime values - HIR (High-level IR)
//!
//! This module defines the value types used by the interpreter.
//! The key design decisions:
//! - Objects use prototype chain for inheritance
//! - Functions have interior mutability (RefCell) for prototype caching
//! - Values are immutable reference-counted handles

pub mod coerce;
pub mod compare;
pub mod convert;
pub mod error;
pub mod function;
pub mod generator;
pub mod generator_replay;
pub mod kind;
pub mod object;
pub mod primitive;
mod val;

pub use coerce::number_to_string;
pub use convert::{
    loose_eq, same_value, strict_eq, to_bool, to_js_string, to_number, to_number_unchecked,
    to_object, to_primitive, to_uint32, try_to_number, PrimitiveHint,
};
pub use error::{
    create_js_error, create_js_error_with_type, get_thrown_value, register_error_constructor,
    set_thrown_value, take_thrown_value, JsError,
};
pub use function::{NativeConstructor, NativeFunction, ValueFunction};
pub use generator::GeneratorObject;
pub use kind::ObjectKind;
pub use object::{
    Getter, GetterBody, GetterStorage, ObjData, Object, PropertyDescriptor, PropertyFlags, Setter,
    SetterBody, SetterStorage, TypedArrayName,
};
pub use val::{ClassValue, Symbol, Value};

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
