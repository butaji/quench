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

    pub(crate) fn of(value: Value) -> Self { Self::from_type(Type::immediate(value)) }

    fn from_type(ty: Type) -> Self {
        match ty.0 {
            TypeKind::Int32 => Self::Int32,
            TypeKind::DirectObjectFamily | TypeKind::DirectObjectShape(_) => Self::DirectObject,
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

    pub(crate) fn join(self, other: Self) -> Self { Self::from_type(self.ty().join(other.ty())) }

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
    }
}
