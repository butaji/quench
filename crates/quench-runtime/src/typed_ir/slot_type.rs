//! Compact observations of slot contents. These are not write-maintained proofs.
use super::{Type, TypeKind};
use crate::{Value, ValueType};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum SlotType {
    None,
    Int32,
    Number,
    Null,
    Undefined,
    Nullish,
    DirectObject,
    DirectObjectOrNull,
    DirectObjectOrUndefined,
    DirectObjectOrNullish,
    Tagged,
}

impl SlotType {
    pub(crate) const ALL: &'static [Self] = &[
        Self::None, Self::Int32, Self::Number, Self::Null, Self::Undefined,
        Self::Nullish, Self::DirectObject, Self::DirectObjectOrNull,
        Self::DirectObjectOrUndefined, Self::DirectObjectOrNullish, Self::Tagged,
    ];

    /// `from_type(Type::immediate(value))`, read straight off the value: writes call it.
    #[inline(always)]
    pub(crate) fn of(value: Value) -> Self {
        if value.as_int().is_some() { Self::Int32 }
        else if value.as_number().is_some() { Self::Number }
        else if value.is_null() { Self::Null }
        else if value.is_undefined() { Self::Undefined }
        else if value.direct_object_index().is_some() { Self::DirectObject }
        else { Self::Tagged }
    }

    pub(crate) fn from_type(ty: Type) -> Self {
        let ty = ty.without_object_shape();
        match ty.0 {
            TypeKind::Int32 => Self::Int32,
            TypeKind::DirectObjectFamily => Self::DirectObject,
            TypeKind::NullableDirectObjectFamily(bits) => {
                if bits == Type::language(ValueType::Null).kinds() { Self::DirectObjectOrNull }
                else if bits == Type::language(ValueType::Undefined).kinds() { Self::DirectObjectOrUndefined }
                else { Self::DirectObjectOrNullish }
            }
            _ if ty == Type::NEVER => Self::None,
            _ if ty == Type::NUMBER => Self::Number,
            _ if ty == Type::language(ValueType::Null) => Self::Null,
            _ if ty == Type::language(ValueType::Undefined) => Self::Undefined,
            _ if ty == Type::NULLISH => Self::Nullish,
            _ => Self::Tagged,
        }
    }

    pub(crate) fn ty(self) -> Type {
        match self {
            Self::None => Type::NEVER,
            Self::Int32 => Type::INT32,
            Self::Number => Type::NUMBER,
            Self::Null => Type::language(ValueType::Null),
            Self::Undefined => Type::language(ValueType::Undefined),
            Self::Nullish => Type::NULLISH,
            Self::DirectObject => Type::DIRECT_OBJECT_FAMILY,
            Self::DirectObjectOrNull => Type::DIRECT_OBJECT_FAMILY.join(Type::language(ValueType::Null)),
            Self::DirectObjectOrUndefined => Type::DIRECT_OBJECT_FAMILY.join(Type::language(ValueType::Undefined)),
            Self::DirectObjectOrNullish => Type::DIRECT_OBJECT_FAMILY.join(Type::NULLISH),
            Self::Tagged => Type::ANY,
        }
    }

    /// The join of `ty()`s, as a union of value classes (`CLASSES`): numbers mix with
    /// nothing else.
    #[inline(always)]
    pub(crate) fn join(self, other: Self) -> Self {
        if self == other { return self; }
        let classes = CLASSES[self as usize] | CLASSES[other as usize];
        match (classes & NUMBERS, classes >> 2) {
            (0, rest @ 0..=7) => NON_NUMBERS[usize::from(rest)],
            (INT32, 0) => Self::Int32,
            (_, 0) => Self::Number,
            _ => Self::Tagged,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::None => "no_live_object", Self::Int32 => "int32", Self::Number => "number",
            Self::Null => "null", Self::Undefined => "undefined", Self::Nullish => "nullish",
            Self::DirectObject => "direct_object", Self::DirectObjectOrNull => "direct_object_or_null",
            Self::DirectObjectOrUndefined => "direct_object_or_undefined",
            Self::DirectObjectOrNullish => "direct_object_or_nullish", Self::Tagged => "tagged",
        }
    }
}

/// Each type's value classes, by discriminant: int32 and other numbers (the low two bits),
/// then null, undefined, direct objects and any other value.
const INT32: u8 = 1;
const NUMBERS: u8 = 3;
const CLASSES: [u8; 11] = [0, INT32, NUMBERS, 4, 8, 4 | 8, 16, 16 | 4, 16 | 8, 16 | 4 | 8, 32];
/// The type of a union of null (1), undefined (2) and direct objects (4), indexed by it.
const NON_NUMBERS: [SlotType; 8] = [
    SlotType::None, SlotType::Null, SlotType::Undefined, SlotType::Nullish, SlotType::DirectObject,
    SlotType::DirectObjectOrNull, SlotType::DirectObjectOrUndefined, SlotType::DirectObjectOrNullish,
];

const _: () = assert!(std::mem::size_of::<SlotType>() == std::mem::size_of::<u8>());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_observations_join_by_semantics_not_discriminant_order() {
        for (index, &left) in SlotType::ALL.iter().enumerate() {
            assert_eq!(left as usize, index);
            assert_eq!(SlotType::from_type(left.ty()), left);
            for &right in SlotType::ALL {
                let joined = left.join(right);
                assert_eq!(joined, SlotType::from_type(left.ty().join(right.ty())));
                assert_eq!(joined, right.join(left));
                assert!(left.ty().is_subtype(joined.ty()) && right.ty().is_subtype(joined.ty()));
                for &third in SlotType::ALL {
                    assert_eq!(joined.join(third), left.join(right.join(third)));
                }
            }
        }
        assert_eq!(SlotType::DirectObject.join(SlotType::Null), SlotType::DirectObjectOrNull);
        assert_eq!(SlotType::DirectObjectOrNull.join(SlotType::Undefined), SlotType::DirectObjectOrNullish);
        assert_eq!(SlotType::Int32.join(SlotType::DirectObject), SlotType::Tagged);
        for value in [Value::direct_array(1), Value::direct_function(1), Value::heap(1), Value::DELETED] {
            assert_eq!(SlotType::of(value), SlotType::Tagged);
        }
        for value in [Value::integer(3), Value::number(0.5), Value::NULL, Value::UNDEFINED, Value::TRUE,
            Value::direct_array(1), Value::heap(1), Value::DELETED] {
            assert_eq!(SlotType::of(value), SlotType::from_type(Type::immediate(value)));
        }
    }
}
