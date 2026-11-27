//! The typed IR's facts: semantic types, machine representations, binding times and
//! proofs, shared by every native lowering (lane bodies, OSR regions, the AOT residual).
//! One authority for each; `vm::specialize` holds the Futamura projections that use them.
//!
//! A representation is not a JavaScript type, and a profile observation is not
//! a proof. A region may specialize an observed representation only by keeping
//! the corresponding guard obligation in its verified entry/definition plan.

use crate::{Value, ValueType};

mod int_range;
mod int_type;
mod lattice;
mod representation;
pub use lattice::{ANY_LANGUAGES, Languages, Type, TypeKind, language_bit};
pub use representation::Representation;
mod slot_type;
pub use slot_type::SlotType;
#[allow(unused_imports)]
pub use int_range::{IntRange, WIDENING_ROUNDS};
mod cell;
pub use cell::{Cell, Classes, Elements, FactKey, Length, MAX_CLASSES};

/// An element index is below its array's length, which every layout stores in 32 bits: it
/// lies within `IntRange::EXACT`, so an index word and the Number it came from agree on it.
const _: () = assert!((crate::bytecode::MAX_ARRAY_LENGTH as u64) <= (1 << int_range::EXACT_BITS));

/// Whether residual execution can export ownership or enter another guest activation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Reentry { Closed, Possible }

mod binding;
pub use binding::{BindingTime, Division, Proof};

impl Type {
    /// A function identity protected by a published weak binding and a
    /// dominating value check. It cannot cross collection or realm retirement.
    pub fn function(identity: Value) -> Option<Self> {
        (identity.direct_function_index().is_some() || identity.legacy_heap_index().is_some())
            .then_some(Self(TypeKind::FunctionIdentity(identity)))
    }

    /// Facts available from a canonical value alone. Heap language types need
    /// a semantic descriptor; an opaque heap locator remains explicitly ANY.
    pub fn immediate(value: Value) -> Self {
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
        } else if value.direct_string_index().is_some() {
            Self::language(ValueType::String)
        } else if value.is_record() {
            Self::language(ValueType::Object)
        } else {
            Self::ANY
        }
    }
}

/// A checked machine projection is an obligation for the backend, not an
/// int32 semantic refinement. Publication must discharge its defining checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Projection {
    Exact,
    #[cfg(test)]
    CheckedDefinitions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fact {
    pub ty: Type,
    pub representation: Representation,
    pub binding: BindingTime,
    pub proof: Proof,
    pub projection: Projection,
}

/// Immutable, consistent semantic type and representation evidence. Entry guards
/// and checked-definition obligations must still be discharged before publication.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerifiedFact(Fact);

impl std::ops::Deref for VerifiedFact {
    type Target = Fact;
    fn deref(&self) -> &Fact { &self.0 }
}

impl Fact {
    pub fn verified(self) -> Option<VerifiedFact> {
        self.verify().then_some(VerifiedFact(self))
    }
    pub(crate) fn verify(self) -> bool {
        self.projection_holds() && self.binding_proved()
    }

    fn projection_holds(self) -> bool {
        match self.projection {
            Projection::Exact => self.representation.accepts(self.ty),
            #[cfg(test)]
            Projection::CheckedDefinitions => {
                self.binding == BindingTime::Dynamic
                    && self.proof == Proof::Definition
                    && self.representation == Representation::Int32
                    && self.ty.is_subtype(Type::NUMBER) && self.ty != Type::NEVER
            }
        }
    }

    fn binding_proved(self) -> bool {
        match self.binding {
            BindingTime::Static => self.proof == Proof::Static && self.ty.without_object_shape() == self.ty
                && self.ty.function_identity().is_none() && self.ty.body_index().is_none(),
            BindingTime::Location | BindingTime::Site => {
                self.proof == Proof::EntryGuard
                    || (self.proof == Proof::CanonicalValue
                        && self.ty == Type::ANY
                        && self.representation == Representation::Tagged)
            }
            BindingTime::Stable => self.proof == Proof::Dependency,
            BindingTime::Dynamic => self.proof == Proof::Definition,
        }
    }

    #[cfg(test)]
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

    pub fn tagged(binding: BindingTime, proof: Proof) -> Self {
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
mod tests;