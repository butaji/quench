//! The typed IR's facts: semantic types, machine representations, binding times and
//! proofs, shared by every native lowering (lane bodies, OSR regions, the AOT residual).
//! One authority for each; `vm::specialize` holds the Futamura projections that use them.
//!
//! A representation is not a JavaScript type, and a profile observation is not
//! a proof. A region may specialize an observed representation only by keeping
//! the corresponding guard obligation in its verified entry/definition plan.

use crate::{Value, ValueType};

mod int_range;
mod slot_type;
pub(crate) use slot_type::SlotType;
pub(crate) use int_range::IntRange;

/// Whether residual execution can export ownership or enter another guest activation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reentry { Closed, Possible }

/// The Ershov division of a source fact. Location and Site fix an address or
/// feedback index, never the contents read from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BindingTime {
    Static,
    Location,
    Site,
    Dynamic,
}

/// One authority for a semantic type. Refined cases determine their language
/// bits; storing those bits alongside the payload would duplicate the fact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TypeKind {
    Language(u16),
    Int32,
    DirectObjectFamily,
    NullableDirectObjectFamily(u16),
    DirectObjectShape(u32),
    FunctionIdentity(Value),
    FunctionBody(u32),
}

/// A set of language types, with an optional semantic refinement.
/// Reserved bits in ANY also cover internal markers: a TDZ/hole is not Number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Type(TypeKind);

impl Type {
    pub(crate) const NEVER: Self = Self(TypeKind::Language(0));
    pub(crate) const ANY: Self = Self(TypeKind::Language(u16::MAX));
    pub(crate) const NULLISH: Self = Self(TypeKind::Language(
        (1 << ValueType::Null as u16) | (1 << ValueType::Undefined as u16)));
    pub(crate) const NUMBER: Self = Self::language(ValueType::Number);
    pub(crate) const INT32: Self = Self(TypeKind::Int32);
    /// The canonical direct-object tag only: no object-table range, base,
    /// shape or storage permission follows from this refinement.
    pub(crate) const DIRECT_OBJECT_FAMILY: Self = Self(TypeKind::DirectObjectFamily);
    /// Internal control discriminant emitted by the typeof-test rewrite.
    pub(crate) const TYPEOF_INDEX: Self = Self(TypeKind::Language(1 << (u16::BITS - 1)));

    pub(crate) const fn language(kind: ValueType) -> Self {
        Self(TypeKind::Language(1 << kind as u16))
    }

    const fn kinds(self) -> u16 {
        match self.0 {
            TypeKind::Language(kinds) => kinds,
            TypeKind::Int32 => 1 << ValueType::Number as u16,
            TypeKind::DirectObjectFamily | TypeKind::DirectObjectShape(_) => 1 << ValueType::Object as u16,
            TypeKind::NullableDirectObjectFamily(nullish) => (1 << ValueType::Object as u16) | nullish,
            TypeKind::FunctionIdentity(_) | TypeKind::FunctionBody(_) => 1 << ValueType::Function as u16,
        }
    }

    /// A direct native object record whose family, range and exact shape were
    /// proved on every incoming edge.
    /// This fact lasts only within a layout-preserving, no-safepoint region;
    /// feedback alone and canonical tagged bits cannot establish it.
    pub(crate) const fn direct_object(shape: u32) -> Self {
        Self(TypeKind::DirectObjectShape(shape))
    }

    pub(crate) const fn is_direct_object_family(self) -> bool {
        matches!(self.0, TypeKind::DirectObjectFamily | TypeKind::DirectObjectShape(_))
    }

    /// Nullish alternatives of a refined direct-object union. This never
    /// implies record range, heap ownership, shape or physical storage.
    fn direct_family_nullish(self) -> Option<u16> {
        match self.0 {
            TypeKind::DirectObjectFamily | TypeKind::DirectObjectShape(_) => Some(0),
            TypeKind::NullableDirectObjectFamily(nullish) => Some(nullish),
            _ => None,
        }
    }

    pub(crate) fn object_shape(self) -> Option<u32> {
        match self.0 {
            TypeKind::DirectObjectShape(shape) => Some(shape),
            _ => None,
        }
    }

    /// A function identity protected by a published weak binding and a
    /// dominating value check. It cannot cross collection or realm retirement.
    pub(crate) fn function(identity: Value) -> Option<Self> {
        (identity.direct_function_index().is_some() || identity.legacy_heap_index().is_some())
            .then_some(Self(TypeKind::FunctionIdentity(identity)))
    }

    pub(crate) fn function_identity(self) -> Option<Value> {
        match self.0 { TypeKind::FunctionIdentity(identity) => Some(identity), _ => None }
    }

    /// A closure admitted against this region's program, function index and realm.
    /// Unlike identity, this admits distinct environments of the same body. It is
    /// local to the active program/realm and cannot cross a safepoint or reentry.
    pub(crate) const fn function_body(function: u32) -> Self {
        Self(TypeKind::FunctionBody(function))
    }

    pub(crate) fn body_index(self) -> Option<u32> {
        match self.0 { TypeKind::FunctionBody(function) => Some(function), _ => None }
    }

    /// Facts available from a canonical value alone. Heap language types need
    /// a semantic descriptor; an opaque heap locator remains explicitly ANY.
    pub(crate) fn immediate(value: Value) -> Self {
        if value.as_int().is_some() {
            Self::INT32
        } else if value.as_number().is_some() {
            Self::NUMBER
        } else if value.as_bool().is_some() {
            Self::language(ValueType::Boolean)
        } else if value.is_null() {
            Self::language(ValueType::Null)
        } else if value.is_undefined() {
            Self::language(ValueType::Undefined)
        } else if value.direct_object_index().is_some() {
            Self::DIRECT_OBJECT_FAMILY
        } else if value.is_record() {
            Self::language(ValueType::Object)
        } else {
            Self::ANY
        }
    }

    pub(crate) fn join(self, other: Self) -> Self {
        if self == other || other == Self::NEVER {
            return self;
        }
        if self == Self::NEVER {
            return other;
        }
        let nullish = |ty: Self| ty.direct_family_nullish()
            .or_else(|| ty.is_subtype(Self::NULLISH).then(|| ty.kinds()));
        if (self.direct_family_nullish().is_some() || other.direct_family_nullish().is_some())
            && let (Some(left), Some(right)) = (nullish(self), nullish(other)) {
            let kinds = left | right;
            return if kinds == 0 { Self::DIRECT_OBJECT_FAMILY }
                else { Self(TypeKind::NullableDirectObjectFamily(kinds)) };
        }
        Self(TypeKind::Language(self.kinds() | other.kinds()))
    }

    pub(crate) fn is_subtype(self, expected: Self) -> bool {
        self == Self::NEVER || self == expected
            || (expected == Self::DIRECT_OBJECT_FAMILY && self.is_direct_object_family())
            || matches!(expected.0, TypeKind::NullableDirectObjectFamily(nullish)
                if self.direct_family_nullish().is_some_and(|actual| actual & !nullish == 0)
                    || matches!(self.0, TypeKind::Language(kinds) if kinds & !nullish == 0))
            || matches!(expected.0, TypeKind::Language(kinds) if self.kinds() & !kinds == 0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Representation {
    Int32,
    Float64,
    Tagged,
}

impl Representation {
    /// Implicit views supported by the shared emitter. Narrowing and unboxing
    /// require explicit checked operations, never a register assignment.
    pub(crate) fn can_receive(self, source: Self) -> bool {
        self == source || self == Self::Tagged || (self == Self::Float64 && source == Self::Int32)
    }
    pub(crate) fn accepts(self, ty: Type) -> bool {
        ty != Type::NEVER
            && match self {
                Self::Int32 => ty.is_subtype(Type::INT32),
                Self::Float64 => ty.is_subtype(Type::NUMBER),
                Self::Tagged => true,
            }
    }
}

/// Proof obligations are part of IR, not inferred from the selected encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Proof {
    Static,
    CanonicalValue,
    EntryGuard,
    Definition,
}

/// A checked machine projection is an obligation for the backend, not an
/// int32 semantic refinement. Publication must discharge its defining checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Projection {
    Exact,
    CheckedDefinitions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Fact {
    pub(crate) ty: Type,
    pub(crate) representation: Representation,
    pub(crate) binding: BindingTime,
    pub(crate) proof: Proof,
    pub(crate) projection: Projection,
}

/// Immutable, consistent semantic type and representation evidence. Entry guards
/// and checked-definition obligations must still be discharged before publication.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VerifiedFact(Fact);

impl std::ops::Deref for VerifiedFact {
    type Target = Fact;
    fn deref(&self) -> &Fact { &self.0 }
}

impl Fact {
    pub(crate) fn verified(self) -> Option<VerifiedFact> {
        self.verify().then_some(VerifiedFact(self))
    }
    pub(crate) fn verify(self) -> bool {
        (match self.projection {
            Projection::Exact => self.representation.accepts(self.ty),
            Projection::CheckedDefinitions => {
                self.binding == BindingTime::Dynamic
                    && self.proof == Proof::Definition
                    && self.representation == Representation::Int32
                    && self.ty.is_subtype(Type::NUMBER) && self.ty != Type::NEVER
            }
        })
            && match self.binding {
                BindingTime::Static => self.proof == Proof::Static && self.ty.object_shape().is_none()
                    && self.ty.function_identity().is_none() && self.ty.body_index().is_none(),
                BindingTime::Location | BindingTime::Site => {
                    self.proof == Proof::EntryGuard
                        || (self.proof == Proof::CanonicalValue
                            && self.ty == Type::ANY
                            && self.representation == Representation::Tagged)
                }
                BindingTime::Dynamic => self.proof == Proof::Definition,
            }
    }

    pub(crate) fn number(
        representation: Representation,
        binding: BindingTime,
        proof: Proof,
    ) -> Self {
        Self {
            ty: if representation == Representation::Int32 && proof == Proof::EntryGuard {
                Type::INT32
            } else {
                Type::NUMBER
            },
            representation,
            binding,
            proof,
            projection: if representation == Representation::Int32 && proof != Proof::EntryGuard {
                Projection::CheckedDefinitions
            } else { Projection::Exact },
        }
    }

    pub(crate) fn tagged(binding: BindingTime, proof: Proof) -> Self {
        Self {
            ty: Type::ANY,
            representation: Representation::Tagged,
            binding,
            proof,
            projection: Projection::Exact,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_body_admission_is_not_identity_or_static_knowledge() {
        let body = Type::function_body(7);
        let other = Type::function_body(9);
        let function = Type::language(ValueType::Function);
        assert_eq!(body.body_index(), Some(7));
        assert_eq!(body.join(body), body);
        assert_eq!(body.join(other), function);
        assert_eq!(body.join(Type::function(Value::direct_function(7)).unwrap()), function);
        assert!(body.is_subtype(function));
        assert!(!function.is_subtype(body));
        assert!(!body.is_subtype(other));
        let fact = Fact { ty: body, representation: Representation::Tagged,
            binding: BindingTime::Location, proof: Proof::EntryGuard, projection: Projection::Exact };
        assert!(fact.verify());
        assert!(!Fact { proof: Proof::CanonicalValue, ..fact }.verify());
        assert!(!Fact { binding: BindingTime::Static, proof: Proof::Static, ..fact }.verify());
    }

    #[test]
    fn function_identities_join_without_conflating_closures_or_static_proofs() {
        let first = Type::function(Value::direct_function(7)).unwrap();
        let second = Type::function(Value::direct_function(9)).unwrap();
        let function = Type::language(ValueType::Function);
        assert_eq!(first.function_identity(), Some(Value::direct_function(7)));
        assert_eq!(first.join(first), first);
        assert_eq!(first.join(second), function);
        assert_eq!(first.join(function), function);
        assert!(first.is_subtype(function));
        assert!(!function.is_subtype(first));
        assert!(!first.is_subtype(second));
        let fact = Fact { ty: first, representation: Representation::Tagged,
            binding: BindingTime::Location, proof: Proof::EntryGuard, projection: Projection::Exact };
        assert!(fact.verify());
        assert!(!Fact { binding: BindingTime::Static, proof: Proof::Static, ..fact }.verify());
        assert!(Type::function(Value::integer(7)).is_none());
        assert_eq!(Type::immediate(Value::direct_function(7)), Type::ANY,
            "tag bits and source annotations cannot manufacture a function proof");
    }

    #[test]
    fn joins_do_not_turn_bigint_or_unknown_into_number() {
        let bigint = Type::language(ValueType::BigInt);
        assert!(!Type::INT32.join(bigint).is_subtype(Type::NUMBER));
        assert!(!Type::NUMBER.join(Type::ANY).is_subtype(Type::NUMBER));
        assert_eq!(Type::INT32.join(Type::NUMBER), Type::NUMBER);
        assert_eq!(Type::NEVER.join(Type::INT32), Type::INT32);
    }

    #[test]
    fn object_shapes_survive_only_compatible_joins_and_proof_lifetimes() {
        let object = Type::language(ValueType::Object);
        let shape = Type::direct_object(3);
        assert_eq!(shape.join(shape), shape);
        assert_eq!(shape.join(Type::NEVER), shape);
        assert_eq!(shape.join(Type::direct_object(4)), Type::DIRECT_OBJECT_FAMILY);
        assert_eq!(shape.join(object), object);
        assert!(shape.is_subtype(object));
        assert!(!object.is_subtype(shape));
        assert!(!Type::direct_object(4).is_subtype(shape));
        assert_eq!(shape.join(Type::language(ValueType::Null)).object_shape(), None);
        let fact = Fact { ty: shape, representation: Representation::Tagged,
            binding: BindingTime::Dynamic, proof: Proof::Definition, projection: Projection::Exact };
        assert!(fact.verified().is_some());
        assert!(Fact { binding: BindingTime::Location, proof: Proof::EntryGuard, ..fact }.verified().is_some());
        for (binding, proof) in [(BindingTime::Static, Proof::Static),
            (BindingTime::Location, Proof::CanonicalValue)] {
            assert!(Fact { binding, proof, ..fact }.verified().is_none());
        }
    }

    #[test]
    fn direct_object_family_is_weaker_than_shape_and_stronger_than_generic_object() {
        let family = Type::DIRECT_OBJECT_FAMILY;
        let object = Type::language(ValueType::Object);
        let shape = Type::direct_object(3);
        for direct in [family, shape] {
            assert!(direct.is_direct_object_family());
            assert!(direct.is_subtype(family));
            assert!(direct.is_subtype(object));
            assert_eq!(direct.join(family), family);
            assert_eq!(family.join(direct), family);
            assert_eq!(direct.join(object), object);
        }
        assert_eq!(family.object_shape(), None);
        assert!(!family.is_subtype(shape));
        for wider in [object, Type::ANY, object.join(Type::language(ValueType::Null))] {
            assert!(!wider.is_direct_object_family());
            assert!(!wider.is_subtype(family));
            assert_eq!(family.join(wider), wider);
        }
        let fact = Fact { ty: family, representation: Representation::Tagged,
            binding: BindingTime::Dynamic, proof: Proof::Definition, projection: Projection::Exact };
        assert!(fact.verified().is_some());
        assert!(Fact { binding: BindingTime::Location, proof: Proof::EntryGuard, ..fact }.verified().is_some());
        assert!(Fact { binding: BindingTime::Location, proof: Proof::CanonicalValue, ..fact }.verified().is_none());
        assert!(Fact { representation: Representation::Int32, ..fact }.verified().is_none());
    }

    #[test]
    fn int32_representation_needs_a_range_proof() {
        for value in [-0.0, f64::NAN, f64::INFINITY, 2147483648.0, 0.5] {
            assert!(!Representation::Int32.accepts(Type::immediate(Value::number(value))));
            assert!(Representation::Float64.accepts(Type::immediate(Value::number(value))));
        }
        assert!(Representation::Tagged.accepts(Type::INT32));
        assert!(!Representation::Float64.accepts(Type::ANY));
        assert!(!Representation::Int32.accepts(Type::NEVER));
    }

    #[test]
    fn a_fixed_location_or_observed_site_does_not_prove_its_value() {
        for binding in [BindingTime::Location, BindingTime::Site] {
            assert!(!Fact::number(Representation::Float64, binding, Proof::Static).verify());
            assert!(!Fact::number(Representation::Float64, binding, Proof::Definition).verify());
            assert!(Fact::number(Representation::Float64, binding, Proof::EntryGuard).verify());
        }
    }

    #[test]
    fn selecting_an_int32_projection_does_not_prove_an_int32_result() {
        let fact = Fact::number(Representation::Int32, BindingTime::Dynamic, Proof::Definition);
        assert_eq!(fact.ty, Type::NUMBER);
        assert_eq!(fact.projection, Projection::CheckedDefinitions);
        assert!(!Representation::Int32.accepts(fact.ty));
        assert!(!Representation::Int32.can_receive(Representation::Float64));
        assert!(!Representation::Float64.can_receive(Representation::Tagged));
        assert!(Representation::Float64.can_receive(Representation::Int32));
    }

    #[test]
    fn checked_projection_admission_requires_a_numeric_int32_destination() {
        let definition = Fact::number(Representation::Int32, BindingTime::Dynamic, Proof::Definition);
        assert!(definition.verified().is_some());
        for representation in [Representation::Float64, Representation::Tagged] {
            assert!(Fact { representation, ..definition }.verified().is_none());
        }
        for ty in [Type::NEVER, Type::ANY, Type::language(ValueType::Boolean),
            Type::NUMBER.join(Type::language(ValueType::BigInt))] {
            assert!(Fact { ty, ..definition }.verified().is_none());
        }
        assert!(Fact { ty: Type::INT32, ..definition }.verified().is_some());
    }

    #[test]
    fn checked_definitions_cannot_stand_in_for_static_or_entry_proofs() {
        let definition = Fact::number(Representation::Int32, BindingTime::Dynamic, Proof::Definition);
        for (binding, proof) in [
            (BindingTime::Static, Proof::Static),
            (BindingTime::Location, Proof::EntryGuard),
            (BindingTime::Site, Proof::EntryGuard),
        ] {
            let fact = Fact { binding, proof, ..definition };
            assert!(fact.verified().is_none(), "{fact:?}");
        }
        assert!(definition.verified().is_some());
        assert!(Fact::number(Representation::Int32, BindingTime::Location, Proof::EntryGuard)
            .verified().is_some());
    }

    #[test]
    fn nullable_direct_object_observations_do_not_authorize_record_access() {
        let family = Type::DIRECT_OBJECT_FAMILY;
        let null = Type::language(ValueType::Null);
        let undefined = Type::language(ValueType::Undefined);
        let nullable = family.join(null);
        let nullish = family.join(Type::NULLISH);
        assert_eq!(Type::immediate(Value::direct_object(7)), family);
        assert_eq!(nullable.join(undefined), nullish);
        assert_ne!(nullable, Type::language(ValueType::Object).join(null));
        assert!(!nullable.is_direct_object_family());
        assert!(nullable.object_shape().is_none());
        assert!(!undefined.is_subtype(nullable));
        for ty in [family, null, undefined] { assert!(ty.is_subtype(nullish)); }
        for value in [Value::direct_array(7), Value::direct_function(7), Value::integer(7), Value::DELETED] {
            assert!(!Type::immediate(value).is_subtype(nullish));
        }
        let observed = Fact { ty: nullish, representation: Representation::Tagged,
            binding: BindingTime::Site, proof: Proof::CanonicalValue, projection: Projection::Exact };
        assert!(observed.verified().is_none());
        assert!(Fact { proof: Proof::EntryGuard, ..observed }.verified().is_some());
        assert!(Fact { representation: Representation::Int32, proof: Proof::EntryGuard, ..observed }.verified().is_none());
    }

    #[test]
    fn joins_preserve_each_semantic_input_and_never_invent_a_numeric_proof() {
        let types = [Type::NEVER, Type::INT32, Type::NUMBER, Type::ANY, Type::TYPEOF_INDEX,
            Type::DIRECT_OBJECT_FAMILY, Type::direct_object(3), Type::direct_object(4),
            Type::NULLISH, Type::DIRECT_OBJECT_FAMILY.join(Type::language(ValueType::Null)),
            Type::DIRECT_OBJECT_FAMILY.join(Type::language(ValueType::Undefined)),
            Type::DIRECT_OBJECT_FAMILY.join(Type::NULLISH),
            Type::language(ValueType::Undefined), Type::language(ValueType::Null),
            Type::language(ValueType::Boolean), Type::language(ValueType::String),
            Type::language(ValueType::Symbol), Type::language(ValueType::BigInt),
            Type::language(ValueType::Object), Type::language(ValueType::Function)];
        for left in types {
            for right in types {
                let joined = left.join(right);
                assert_eq!(joined, right.join(left));
                for third in types {
                    assert_eq!(joined.join(third), left.join(right.join(third)),
                        "join associativity: {left:?}, {right:?}, {third:?}");
                }
                assert!(left.is_subtype(joined) && right.is_subtype(joined));
                for expected in [Type::INT32, Type::NUMBER, Type::language(ValueType::Object),
                    Type::DIRECT_OBJECT_FAMILY, Type::direct_object(3), Type::NULLISH,
                    Type::DIRECT_OBJECT_FAMILY.join(Type::NULLISH)] {
                    assert_eq!(joined.is_subtype(expected),
                        left.is_subtype(expected) && right.is_subtype(expected));
                }
            }
        }
    }

    #[test]
    fn admission_rejects_missing_types_and_unproved_machine_views() {
        let canonical = Fact::tagged(BindingTime::Location, Proof::CanonicalValue);
        for malformed in [
            Fact { ty: Type::NEVER, ..canonical },
            Fact { representation: Representation::Float64, ..canonical },
            Fact { ty: Type::NUMBER, ..canonical },
            Fact { proof: Proof::Static, ..canonical },
            Fact { binding: BindingTime::Static, ..canonical },
        ] {
            assert!(malformed.verified().is_none(), "{malformed:?}");
        }
        let union = Type::NUMBER.join(Type::language(ValueType::BigInt));
        let fact = Fact { ty: union, binding: BindingTime::Dynamic,
            proof: Proof::Definition, ..canonical };
        assert_eq!(fact.verified().unwrap().ty, union);
        assert!(Fact { representation: Representation::Float64, ..fact }.verified().is_none());
        assert_eq!(canonical.verified().unwrap().ty, Type::ANY);
    }
}
