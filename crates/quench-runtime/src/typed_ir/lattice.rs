//! The semantic type lattice: a `Type` is a set of language types, refined by an integer range,
//! a direct-object shape, or a function identity or body. Pure order and data, with no heap or VM
//! behind it: the runtime's `typed_ir` re-exports it, and the b-jit stencils include this same file
//! so both tiers fold the same facts.
use crate::typed_ir::IntRange;
use crate::{Value, ValueType};

/// One authority for a semantic type. Refined cases determine their language
/// bits; storing those bits alongside the payload would duplicate the fact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TypeKind {
    Language(u16),
    Int(IntRange),
    DirectObjectFamily,
    NullableDirectObjectFamily(u16),
    DirectObjectShape { shape: u32, nullish: u16 },
    FunctionIdentity(Value),
    FunctionBody(u32),
}

/// A set of language types, with an optional semantic refinement.
/// Reserved bits in ANY also cover internal markers: a TDZ/hole is not Number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Type(pub(super) TypeKind);

impl Type {
    pub(crate) const NEVER: Self = Self(TypeKind::Language(0));
    pub(crate) const ANY: Self = Self(TypeKind::Language(u16::MAX));
    pub(crate) const NULLISH: Self = Self(TypeKind::Language(
        (1 << ValueType::Null as u16) | (1 << ValueType::Undefined as u16)));
    /// The types whose values name no heap record (a Number, a Boolean, `null`, `undefined`):
    /// the tag layout's non-reference words, which a store writes with no barrier.
    pub(crate) const IMMEDIATE: Self = Self(TypeKind::Language(
        (1 << ValueType::Number as u16) | (1 << ValueType::Boolean as u16)
            | (1 << ValueType::Null as u16) | (1 << ValueType::Undefined as u16)));
    pub(crate) const NUMBER: Self = Self::language(ValueType::Number);
    /// The integers of the whole int32 span, the widest range an int32-represented value holds.
    pub(crate) const INT32: Self = Self(TypeKind::Int(IntRange::FULL));
    /// The canonical direct-object tag only: no object-table range, base,
    /// shape or storage permission follows from this refinement.
    pub(crate) const DIRECT_OBJECT_FAMILY: Self = Self(TypeKind::DirectObjectFamily);
    /// Internal control discriminant emitted by the typeof-test rewrite.
    pub(crate) const TYPEOF_INDEX: Self = Self(TypeKind::Language(1 << (u16::BITS - 1)));

    pub(crate) const fn language(kind: ValueType) -> Self {
        Self(TypeKind::Language(1 << kind as u16))
    }

    /// The set of language types `bits` names (`language` bits, joined): a declared type
    /// (`Function::declared_parameters`).
    pub(crate) const fn languages(bits: u16) -> Self {
        Self(TypeKind::Language(bits))
    }

    // Inlined so a stage's constant fact (`Type::ANY` in the interpreter's own store,
    // `staged::store::own_store`) folds its gate tests away instead of calling here per store.
    #[inline(always)]
    pub(super) const fn kinds(self) -> u16 {
        match self.0 {
            TypeKind::Language(kinds) => kinds,
            TypeKind::Int(_) => 1 << ValueType::Number as u16,
            TypeKind::DirectObjectFamily => 1 << ValueType::Object as u16,
            TypeKind::DirectObjectShape { nullish, .. } => (1 << ValueType::Object as u16) | nullish,
            TypeKind::NullableDirectObjectFamily(nullish) => (1 << ValueType::Object as u16) | nullish,
            TypeKind::FunctionIdentity(_) | TypeKind::FunctionBody(_) => 1 << ValueType::Function as u16,
        }
    }

    /// A direct native object record whose family, range and exact shape were
    /// proved on every incoming edge.
    /// This fact lasts only within a layout-preserving, no-safepoint region;
    /// feedback alone and canonical tagged bits cannot establish it.
    pub(crate) const fn direct_object(shape: u32) -> Self {
        Self(TypeKind::DirectObjectShape { shape, nullish: 0 })
    }

    pub(crate) const fn is_direct_object_family(self) -> bool {
        matches!(self.0, TypeKind::DirectObjectFamily | TypeKind::DirectObjectShape { nullish: 0, .. })
    }

    /// Nullish alternatives of a refined direct-object union. This never
    /// implies record range, heap ownership, shape or physical storage.
    fn direct_family_nullish(self) -> Option<u16> {
        match self.0 {
            TypeKind::DirectObjectFamily => Some(0),
            TypeKind::DirectObjectShape { nullish, .. } => Some(nullish),
            TypeKind::NullableDirectObjectFamily(nullish) => Some(nullish),
            _ => None,
        }
    }

    pub(crate) fn object_shape(self) -> Option<u32> {
        match self.0 {
            TypeKind::DirectObjectShape { shape, nullish: 0 } => Some(shape),
            _ => None,
        }
    }

    /// The object alternative, usable only after an edge proves this value is a
    /// direct object. A nullable shape alone never authorizes a record access.
    pub(crate) fn direct_object_part(self) -> Option<Self> {
        match self.0 {
            TypeKind::DirectObjectShape { shape, .. } => Some(Self::direct_object(shape)),
            TypeKind::DirectObjectFamily | TypeKind::NullableDirectObjectFamily(_) => Some(Self::DIRECT_OBJECT_FAMILY),
            _ => None,
        }
    }

    /// Retire a region-local shape proof while retaining its language alternatives.
    pub(crate) fn without_object_shape(self) -> Self {
        match self.0 {
            TypeKind::DirectObjectShape { nullish: 0, .. } => Self::DIRECT_OBJECT_FAMILY,
            TypeKind::DirectObjectShape { nullish, .. } => Self(TypeKind::NullableDirectObjectFamily(nullish)),
            _ => self,
        }
    }

    /// Whether nothing narrower than a Number is known: what an int32 contract of a loop may narrow.
    pub(crate) fn is_any_or_number(self) -> bool { matches!(self, Self::ANY | Self::NUMBER) }

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

    pub(crate) fn join(self, other: Self) -> Self {
        if let Some(joined) = self.trivial_join(other) {
            return joined;
        }
        let shaped = match (self.0, other.0) {
            (TypeKind::DirectObjectShape { shape, nullish: left }, TypeKind::DirectObjectShape { shape: right, nullish })
                if shape == right => Some((shape, left | nullish)),
            (TypeKind::DirectObjectShape { shape, nullish }, _) if other.is_subtype(Self::NULLISH) =>
                Some((shape, nullish | other.kinds())),
            (_, TypeKind::DirectObjectShape { shape, nullish }) if self.is_subtype(Self::NULLISH) =>
                Some((shape, nullish | self.kinds())),
            _ => None,
        };
        if let Some((shape, nullish)) = shaped { return Self(TypeKind::DirectObjectShape { shape, nullish }); }
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

    /// Whether this is `ANY`, by pattern rather than the derived `==`, so a frame's constant
    /// fact folds even where the comparison is not inlined.
    #[inline(always)]
    pub(crate) const fn is_any(self) -> bool { matches!(self.0, TypeKind::Language(u16::MAX)) }

    #[inline(always)]
    pub(crate) fn is_subtype(self, expected: Self) -> bool {
        // Nothing but `ANY` holds every value: the common unproved fact answers with no compare.
        if self.is_any() { return expected.is_any(); }
        self == Self::NEVER || self == expected
            || matches!((self.0, expected.0), (TypeKind::Int(known), TypeKind::Int(allowed)) if allowed.contains(known))
            || matches!(expected.0, TypeKind::DirectObjectShape { shape, nullish }
                if matches!(self.0, TypeKind::DirectObjectShape { shape: actual, nullish: alternatives }
                    if actual == shape && alternatives & !nullish == 0)
                    || matches!(self.0, TypeKind::Language(kinds) if kinds & !nullish == 0))
            || (expected == Self::DIRECT_OBJECT_FAMILY && self.is_direct_object_family())
            || matches!(expected.0, TypeKind::NullableDirectObjectFamily(nullish)
                if self.direct_family_nullish().is_some_and(|actual| actual & !nullish == 0)
                    || matches!(self.0, TypeKind::Language(kinds) if kinds & !nullish == 0))
            || matches!(expected.0, TypeKind::Language(kinds) if self.kinds() & !kinds == 0)
    }
}
