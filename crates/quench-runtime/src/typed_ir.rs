//! The typed IR's facts: semantic types, machine representations, binding times and
//! proofs, shared by every native lowering (lane bodies, OSR regions, the AOT residual).
//! One authority for each; `vm::specialize` holds the Futamura projections that use them.
//!
//! A representation is not a JavaScript type, and a profile observation is not
//! a proof. A region may specialize an observed representation only by keeping
//! the corresponding guard obligation in its verified entry/definition plan.

use crate::{Value, ValueType};

/// The Ershov division of a source fact. Location and Site fix an address or
/// feedback index, never the contents read from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BindingTime {
    Static,
    Location,
    Site,
    Dynamic,
}

/// A set of language types, with an independent exact signed-int32 refinement.
/// Reserved bits in ANY also cover internal markers: a TDZ/hole is not Number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Type {
    kinds: u16,
    int32: bool,
}

impl Type {
    pub(crate) const NEVER: Self = Self {
        kinds: 0,
        int32: false,
    };
    pub(crate) const ANY: Self = Self {
        kinds: u16::MAX,
        int32: false,
    };
    pub(crate) const NUMBER: Self = Self::language(ValueType::Number);
    pub(crate) const INT32: Self = Self {
        kinds: Self::NUMBER.kinds,
        int32: true,
    };
    /// Internal control discriminant emitted by the typeof-test rewrite.
    pub(crate) const TYPEOF_INDEX: Self = Self { kinds: 1 << (u16::BITS - 1), int32: false };

    pub(crate) const fn language(kind: ValueType) -> Self {
        Self {
            kinds: 1 << kind as u16,
            int32: false,
        }
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
        } else if value.is_record() {
            Self::language(ValueType::Object)
        } else {
            Self::ANY
        }
    }

    pub(crate) fn join(self, other: Self) -> Self {
        if self == Self::NEVER {
            return other;
        }
        if other == Self::NEVER {
            return self;
        }
        let kinds = self.kinds | other.kinds;
        Self {
            kinds,
            int32: kinds == Self::NUMBER.kinds && self.int32 && other.int32,
        }
    }

    pub(crate) fn is_subtype(self, expected: Self) -> bool {
        self == Self::NEVER
            || (self.kinds & !expected.kinds == 0 && (!expected.int32 || self.int32))
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

impl Fact {
    pub(crate) fn verify(self) -> bool {
        (self.representation.accepts(self.ty)
            || (self.projection == Projection::CheckedDefinitions
                && self.representation == Representation::Int32
                && self.ty.is_subtype(Type::NUMBER) && self.ty != Type::NEVER))
            && match self.binding {
                BindingTime::Static => self.proof == Proof::Static,
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
    fn joins_do_not_turn_bigint_or_unknown_into_number() {
        let bigint = Type::language(ValueType::BigInt);
        assert!(!Type::INT32.join(bigint).is_subtype(Type::NUMBER));
        assert!(!Type::NUMBER.join(Type::ANY).is_subtype(Type::NUMBER));
        assert_eq!(Type::INT32.join(Type::NUMBER), Type::NUMBER);
        assert_eq!(Type::NEVER.join(Type::INT32), Type::INT32);
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
}
