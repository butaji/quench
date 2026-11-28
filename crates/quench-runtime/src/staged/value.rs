//! Value-word reads every tier shares: which heap family a value names.

use super::machine::{Binary, Lane, MachineScalar, Relation, Unary};
use super::region::Region;
use super::Stage;
use crate::value::ValueLayout;

#[path = "value/tags.rs"]
mod tags;
#[path = "value/classes.rs"]
mod classes;
#[path = "value/numbers.rs"]
mod numbers;
#[path = "value/truthiness.rs"]
mod truthiness;
#[path = "value/typeof_ops.rs"]
mod typeof_ops;
pub use tags::*;
pub use classes::*;
pub use numbers::*;
pub use truthiness::*;
pub use typeof_ops::*;

/// The language bits of a declared parameter's mask (`quench_bjit::shared_words::language`, the
/// order of `ValueType`'s discriminants; the runtime's `vm/compiled/base_region/declared.rs` asserts
/// they are these types' bits).
pub mod language {
    pub const UNDEFINED: u16 = 1 << 0;
    pub const NULL: u16 = 1 << 1;
    pub const BOOLEAN: u16 = 1 << 2;
    pub const NUMBER: u16 = 1 << 3;
    pub const STRING: u16 = 1 << 4;
    pub const OBJECT: u16 = 1 << 7;
    pub const FUNCTION: u16 = 1 << 8;
    pub const ARRAY: u16 = 1 << 9;
    pub const OBJECT_PLAIN: u16 = 1 << 10;
}

#[cfg(not(stencil_build))]
use crate::bytecode::typeof_name_index as name_index;

// The stencil build mounts the same self-contained file as `crate::bytecode`'s `typeof_names`.
#[cfg(stencil_build)]
#[path = "../bytecode/typeof_names.rs"]
mod typeof_names;
#[cfg(stencil_build)]
use typeof_names::typeof_name_index as name_index;
#[cfg(test)]
mod typeof_tests {
    use super::*;
    use crate::Value;
    use crate::bytecode::TYPEOF_NAMES;

    /// The tag classes answer the index of their `typeof` name; a heap value or an internal marker has none.
    #[test]
    fn the_tag_classes_name_their_typeof_index() {
        let named = |value: Value| concrete_immediate_typeof_index(value).map(|index| TYPEOF_NAMES[index]);
        assert_eq!(named(Value::UNDEFINED), Some("undefined"));
        assert_eq!(named(Value::DELETED), Some("undefined"));
        assert_eq!(named(Value::NULL), Some("object"));
        assert_eq!([named(Value::TRUE), named(Value::FALSE)], [Some("boolean"); 2]);
        assert_eq!([named(Value::integer(-3)), named(Value::number(2.5)), named(Value::number(f64::NAN))], [Some("number"); 3]);
        assert_eq!(TYPEOF_NAMES[TYPEOF_UNDEFINED as usize], "undefined");
        assert_eq!(TYPEOF_NAMES[TYPEOF_OBJECT as usize], "object");
        assert_eq!(TYPEOF_NAMES[TYPEOF_BOOLEAN as usize], "boolean");
        assert_eq!(TYPEOF_NAMES[TYPEOF_NUMBER as usize], "number");
    }
}
