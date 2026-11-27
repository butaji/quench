//! The value classes `SlotType::admits` tests, and the mask of classes each slot type admits. Plain
//! data, so the stencil build includes this file unchanged (`quench-bjit/stencils/facade.rs`);
//! `slot_type.rs` asserts that the discriminants below are `SlotType`'s.

/// The value classes a `SLOT_ADMITS` mask names, one bit each (`staged::slot_store::value_class`):
/// an int32, a Number that is a double, `null`, `undefined`, a direct object, a direct string, a
/// boolean, and any other value.
pub(crate) const CLASS_INT: u8 = 1 << 0;
pub(crate) const CLASS_DOUBLE: u8 = 1 << 1;
pub(crate) const CLASS_NULL: u8 = 1 << 2;
pub(crate) const CLASS_UNDEFINED: u8 = 1 << 3;
pub(crate) const CLASS_OBJECT: u8 = 1 << 4;
pub(crate) const CLASS_STRING: u8 = 1 << 5;
pub(crate) const CLASS_BOOLEAN: u8 = 1 << 6;
pub(crate) const CLASS_OTHER: u8 = 1 << 7;

/// The classes each slot type admits, by discriminant (`SlotType::admits`). A static, so a staged
/// row reads it at an address the runtime fixes (`Table::SlotAdmits`).
pub(crate) static SLOT_ADMITS: [u8; 13] = [
    0,
    CLASS_INT,
    CLASS_INT | CLASS_DOUBLE,
    CLASS_NULL,
    CLASS_UNDEFINED,
    CLASS_NULL | CLASS_UNDEFINED,
    CLASS_OBJECT,
    CLASS_OBJECT | CLASS_NULL,
    CLASS_OBJECT | CLASS_UNDEFINED,
    CLASS_OBJECT | CLASS_NULL | CLASS_UNDEFINED,
    CLASS_STRING,
    CLASS_BOOLEAN,
    u8::MAX,
];

/// `SlotType::Number`'s discriminant: the one slot type a tracked store refuses in place, since its
/// arena encoding of an int32 is a double (`encode_slot`).
pub(crate) const SLOT_NUMBER: u8 = 2;
