//! Facts as typed proofs (S1.5): the words a numeric row takes as an int32 or a binary64 operand.
//!
//! A numeric row computes only on an `Int32` or a `Float64`, never on a bare word. Each has a
//! private field, so a word becomes one in only these ways:
//! - `guard`: the single staged guard, the same in every projection: the tag test of a tagged
//!   word, leaving at the frame's miss edge (`checked` is its concrete form).
//! - `by_fact`: a fact token (`IntFact`, `NumberFact`) that only a fact makes: a type the
//!   analysis proved (`of_type`) or a representation the specializer fixed (`of_representation`).
//!   No test runs, and none is needed: the fact is the proof.
//! - a row's own result (`pub(crate)` constructors), and a constant the row folds (`constant`).

use super::{NumericStage, Stage};
use crate::typed_ir::{Representation, Type};

/// The low 32 bits of an int32, proved; the stage's word otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Int32<W>(W);

/// The IEEE bits of a binary64, proved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Float64<W>(W);

/// A fact that a word is an int32's low 32 bits. Only a fact makes one.
#[derive(Clone, Copy, Debug)]
pub struct IntFact(());

/// A fact that a word is a binary64's IEEE bits. Only a fact makes one.
#[derive(Clone, Copy, Debug)]
pub struct NumberFact(());

impl IntFact {
    /// `of_representation(Representation::Int32)` for a home the specializer fixed statically
    /// (a pinned int register, an int32 operand form): the same fact, with no match to fold.
    #[inline]
    pub const fn int32_home() -> Self { IntFact(()) }
    /// The analysis proved the value an int32 (its type is a subtype of `INT32`).
    #[inline]
    pub fn of_type(ty: Type) -> Option<Self> { ty.is_subtype(Type::INT32).then_some(IntFact(())) }
    /// The specializer holds the value unboxed as an int32 (`Representation::Int32`).
    #[inline]
    pub fn of_representation(representation: Representation) -> Option<Self> {
        (representation == Representation::Int32).then_some(IntFact(()))
    }
}

impl NumberFact {
    /// `of_representation(Representation::Float64)` for a home the specializer fixed statically
    /// (a pinned double register, a raw-double slot): the same fact, with no match to fold.
    #[inline]
    pub const fn float64_home() -> Self { NumberFact(()) }
    /// The analysis proved the value a Number (its type is a subtype of `NUMBER`).
    #[inline]
    pub fn of_type(ty: Type) -> Option<Self> { ty.is_subtype(Type::NUMBER).then_some(NumberFact(())) }
    /// The specializer holds the value unboxed as a binary64 (`Representation::Float64`).
    #[inline]
    pub fn of_representation(representation: Representation) -> Option<Self> {
        (representation == Representation::Float64).then_some(NumberFact(()))
    }
}

impl<W: Copy> Int32<W> {
    /// The int32 `word` holds by a fact; no test runs.
    #[inline]
    pub fn by_fact(word: W, _fact: IntFact) -> Self { Int32(word) }
    /// A word a staged row computed as an int32.
    #[inline(always)]
    pub(crate) fn of_row(word: W) -> Self { Int32(word) }
    /// The proved word.
    #[inline(always)]
    pub fn word(self) -> W { self.0 }
    /// The single int32 guard: `tagged` is an int-tagged word, else leave at `miss`; its low half.
    #[inline(always)]
    pub fn guard<S: NumericStage<Word = W>>(stage: &mut S, tagged: W, miss: S::Miss) -> Option<Self> {
        let int = super::value::is_int(stage, tagged);
        stage.guard(int, miss)?;
        Some(Int32(stage.narrow(tagged)))
    }
    /// The int32 constant `value`, folded into `stage`.
    #[inline(always)]
    pub fn constant<S: NumericStage<Word = W>>(stage: &mut S, value: i32) -> Self { Int32(stage.i32_constant(value)) }
    /// ToInt32 of a proved binary64 by the stage's own conversion (`NumericStage::to_int32`).
    #[inline(always)]
    pub fn truncating<S: NumericStage<Word = W>>(stage: &mut S, number: Float64<W>, miss: S::Miss) -> Option<Self> {
        stage.to_int32(number.0, miss).map(Int32)
    }
}

impl<W: Copy> Float64<W> {
    /// The binary64 `word` holds by a fact; no test runs.
    #[inline]
    pub fn by_fact(word: W, _fact: NumberFact) -> Self { Float64(word) }
    /// A word a staged row computed as a binary64.
    #[inline(always)]
    pub(crate) fn of_row(word: W) -> Self { Float64(word) }
    /// The proved word.
    #[inline(always)]
    pub fn word(self) -> W { self.0 }
    /// The single Number guard: a double word is its own bits, an int-tagged word widens, and any
    /// other word leaves at `miss`. The double test comes first, as `Value::as_number` orders it:
    /// by the shifted tag field, or (`NumericStage::ORDERED_FIRST`) by the word read as a binary64
    /// being ordered, which only a double's bits are, the NaN space's words then sorted by tag.
    #[inline(always)]
    pub fn guard<S: NumericStage<Word = W>>(stage: &mut S, tagged: W, miss: S::Miss) -> Option<Self> {
        if S::ORDERED_FIRST {
            let number = stage.f64_from_bits(tagged);
            let ordered = stage.float_eq(number, number);
            return stage.choose_leaving(ordered, #[inline(always)] move |_| Some(number), #[inline(always)] move |stage| {
                let (double, int) = super::value::number_classes(stage, tagged);
                stage.choose_leaving(int, #[inline(always)] move |stage| { let low = stage.narrow(tagged); Some(stage.i32_to_f64(low)) },
                    #[inline(always)] move |stage| { stage.guard(double, miss)?; Some(number) })
            }).map(Float64);
        }
        let (double, int) = super::value::number_classes(stage, tagged);
        stage.choose_leaving(double, #[inline(always)] |stage| Some(stage.f64_from_bits(tagged)), #[inline(always)] |stage| {
            stage.guard(int, miss)?;
            let low = stage.narrow(tagged);
            Some(stage.i32_to_f64(low))
        }).map(Float64)
    }
    /// The binary64 constant `value`, folded into `stage`.
    #[inline(always)]
    pub fn constant<S: NumericStage<Word = W>>(stage: &mut S, value: f64) -> Self { Float64(stage.f64_constant(value)) }
}

#[cfg(not(stencil_build))]
impl Int32<u64> {
    /// `guard` at the interpreter's stage: a tagged value's int32; `None` where it holds none.
    #[inline(always)]
    pub fn checked(value: crate::Value) -> Option<Self> {
        Self::guard(&mut super::Concrete::new(core::ptr::null()), value.to_bits(), ())
    }
}

#[cfg(not(stencil_build))]
impl Int32<u64> {
    /// `guard` lowered to a value's two dwords, for a frame that reads them in memory: the tag
    /// dword compared alone (`cmp dword [..+4], imm`), the value dword loaded only past it. An int
    /// word's high dword is exactly `INT_TAG_HIGH` (its payload is 32 bits), so this is `guard`.
    #[inline]
    pub fn guard_halves(high: u32, low: impl FnOnce() -> u32) -> Option<Self> {
        (high == crate::value::INT_TAG_HIGH).then(|| Int32(u64::from(low())))
    }
}

/// The guards lowered to the machine word, for a frame that holds plain `u64`s (a stencil's places).
impl Int32<u64> {
    /// `guard` of two tagged words in one test: their high dwords are both exactly the int tag's
    /// (an int word's payload is 32 bits), tested as `(x ^ tag) | (y ^ tag)` shifted, for a row
    /// that takes both operands as int32s.
    #[inline(always)]
    pub fn guard_pair(x: u64, y: u64) -> Option<(Self, Self)> {
        let tag = crate::value::ValueLayout::INT_TAG;
        (((x ^ tag) | (y ^ tag)) >> u32::BITS == 0).then(|| (Int32(u64::from(x as u32)), Int32(u64::from(y as u32))))
    }
    /// The int32 a proved binary64 is exactly, by the projection's own truncation `truncate` (a
    /// stencil's `cvttsd2si`): its value converted back has the same bits (no fraction, no -0, in
    /// range). The round trip is the test, so any `truncate` gives a sound answer.
    #[inline(always)]
    pub fn exact_by(number: Float64<u64>, truncate: impl FnOnce(f64) -> i32) -> Option<Self> {
        let int = truncate(f64::from_bits(number.0));
        (f64::from(int).to_bits() == number.0).then_some(Int32(u64::from(int as u32)))
    }
    /// ToInt32 of a proved binary64 whose truncation is exact (`value::exact_uint32`), else `None`.
    #[inline(always)]
    pub fn exact_truncation(number: Float64<u64>) -> Option<Self> {
        crate::value::exact_uint32(f64::from_bits(number.0)).map(|uint32| Int32(u64::from(uint32)))
    }
}

#[cfg(not(stencil_build))]
impl Float64<u64> {
    /// `guard` at the interpreter's stage: a tagged value's Number; `None` where it is none.
    #[inline(always)]
    pub fn checked(value: crate::Value) -> Option<Self> {
        Self::guard(&mut super::Concrete::new(core::ptr::null()), value.to_bits(), ())
    }
}

/// The guards a machine-word stage (a specializer's) runs on numbers it holds unboxed.
impl<W: Copy> Int32<W> {
    /// The int32 a proved binary64 is exactly (`value::exact_int32`: no fraction, no -0, in
    /// range), else leave at `miss`.
    #[inline]
    pub fn exact<S>(stage: &mut S, number: Float64<W>, miss: <S as Stage>::Miss) -> Option<Self>
    where S: NumericStage<Word = W, Cond = W> + super::machine::MachineScalar<Word = W> {
        let (int, exact) = super::value::exact_int32(stage, number.0);
        stage.guard(exact, miss)?;
        Some(Int32(int))
    }
    /// The int32 a signed 64-bit integer word is, where the word fits one, else leave at `miss`.
    #[inline]
    pub fn of_long<S>(stage: &mut S, word: W, miss: <S as Stage>::Miss) -> Option<Self>
    where S: NumericStage<Word = W, Cond = W> + super::machine::MachineScalar<Word = W> {
        use super::machine::{Lane, Relation, Unary};
        let int = stage.raw_unary(Unary::Narrow(Lane::I32), word);
        let widened = stage.raw_unary(Unary::SignExtend(Lane::I64), int);
        let fits = stage.raw_compare(Relation::Equal, widened, word);
        stage.guard(fits, miss)?;
        Some(Int32(int))
    }
}

impl<W: Copy> Int32<W> {
    /// ToInt32 of a tagged word, as a bitwise operator reads it: an int-tagged word is its int32,
    /// a Boolean its 0 or 1 (`(a < b) | 0`: `true`'s word is `false`'s with the low bit set), `undefined` and
    /// `null` 0, a Number truncates (`NumericStage::to_int32`), and any other word leaves at `miss`.
    #[inline(always)]
    pub fn guard_truncating<S: NumericStage<Word = W>>(stage: &mut S, tagged: W, miss: S::Miss) -> Option<Self>
    where S::Miss: Copy {
        let int = super::value::is_int(stage, tagged);
        stage.choose_leaving(int, #[inline(always)] |stage| Some(stage.narrow(tagged)), #[inline(always)] |stage| {
            let boolean = super::value::is_boolean(stage, tagged);
            stage.choose_leaving(boolean, #[inline(always)] |stage| { let bit = stage.and_imm(tagged, 1u64); Some(stage.narrow(bit)) },
                #[inline(always)] |stage| {
                    // `undefined` and `null` are 0 (ToNumber gives NaN and +0): `x | 0` of a hole.
                    let (undefined, null) = (super::value::is_undefined(stage, tagged), super::value::is_null(stage, tagged));
                    let nullish = stage.any(undefined, null);
                    stage.choose_leaving(nullish, #[inline(always)] |stage| { let zero = stage.constant(0); Some(stage.narrow(zero)) },
                        #[inline(always)] |stage| {
                            let number = Float64::guard(stage, tagged, miss)?;
                            stage.to_int32(number.0, miss)
                        })
                })
        }).map(Int32)
    }
}

impl<W: Copy> Float64<W> {
    /// The binary64 an int32 is.
    #[inline]
    pub fn of_int<S: NumericStage<Word = W>>(stage: &mut S, int: Int32<W>) -> Self { Float64(stage.i32_to_f64(int.0)) }
    /// The binary64 a signed 64-bit integer word converts to (every such word is a Number).
    #[inline]
    pub fn of_long<S: super::machine::MachineScalar<Word = W>>(stage: &mut S, word: W) -> Self {
        Float64(stage.raw_unary(super::machine::Unary::FromSigned(super::machine::Lane::F64), word))
    }
}

/// The proved operand types a stage's numeric rows take.
pub type IntOf<S> = Int32<<S as Stage>::Word>;
/// The proved binary64 operand type a stage's numeric rows take.
pub type NumberOf<S> = Float64<<S as Stage>::Word>;

/// An object record whose exact shape a guard proved (`ic::guard_shape`, `ic::guard_layout`,
/// `guard::shape`, `guard::prototype`), or a fact holds (`by_fact`): its slots are read at the
/// shape's offsets with no test of their own. The field is private, so a slot read
/// (`ic::slot_layout`, `object::proved_own_layout`) runs only past a guard or a fact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Shaped<W>(W);

/// A fact that a record has the exact shape a read was planned for. Only a guard
/// (`Shaped::fact`), a type (`of_type`), a region's entry guard (`entry_guard`) or a held code
/// dependency (`HeldShapes::hold`, `HeldLayouts::hold`) makes one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShapeFact(());

impl ShapeFact {
    /// The analysis proved the value a direct object of one exact shape (`Type::object_shape`):
    /// that shape, and the fact.
    #[inline]
    pub fn of_type(ty: Type) -> Option<(u32, Self)> { ty.object_shape().map(|shape| (shape, ShapeFact(()))) }
    /// The region's entry guarded the receiver's exact shape and flat form (`guard::shape` at
    /// `Form::Arena`, `base_region::proofs`), and a row inside the region reads under it.
    #[inline]
    /// Only a stencil row makes one (and the tests): the region's entry guard runs in another
    /// compilation unit, so no runtime code can name the fact without its guard.
    #[cfg(any(stencil_build, test))]
    pub(crate) const fn entry_guard() -> Self { ShapeFact(()) }
}

/// The exact shapes a compiled unit holds under code dependencies (`StableFacts::shapes`), which
/// its publication subscribes: holding one is the only way a dependency makes a `ShapeFact`, so
/// a read under a held shape always retires with the code the moment an object leaves it. The
/// guard became the invalidation hook: the code tests nothing, the departure retires it.
#[cfg(not(stencil_build))]
#[derive(Default)]
pub struct HeldShapes(crate::bitset::BitSet);

#[cfg(not(stencil_build))]
impl HeldShapes {
    /// Holds `shape`: the code depends on it from now on, and the fact is the proof.
    #[inline]
    pub fn hold(&mut self, shape: u32) -> ShapeFact { self.0.insert_growing(shape as usize); ShapeFact(()) }
    /// The held shapes, leaving none.
    pub fn take(&mut self) -> Box<[u32]> { std::mem::take(&mut self.0).iter().map(|shape| shape as u32).collect() }
}

/// The global objects' layouts a compiled unit holds under code dependencies
/// (`StableFacts::layouts`, `subscribe_global_layout`): holding one is the only way a global
/// object's slot read gets its `ShapeFact`, as `HeldShapes::hold` is for an object's shape.
#[cfg(not(stencil_build))]
#[derive(Default)]
pub struct HeldLayouts(std::collections::BTreeSet<crate::heap::GlobalLayoutFact>);

#[cfg(not(stencil_build))]
impl HeldLayouts {
    /// Holds `layout`: the code depends on it from now on, and the fact is the proof.
    #[inline]
    pub fn hold(&mut self, layout: crate::heap::GlobalLayoutFact) -> ShapeFact { self.0.insert(layout); ShapeFact(()) }
    /// The held layouts, leaving none.
    pub fn take(&mut self) -> Box<[crate::heap::GlobalLayoutFact]> { std::mem::take(&mut self.0).into_iter().collect() }
}

impl<W: Copy> Shaped<W> {
    /// The record `record` of the shape a fact proved; no test runs.
    #[inline]
    pub fn by_fact(record: W, _fact: ShapeFact) -> Self { Shaped(record) }
    /// A record a staged guard in this module tree just tested.
    #[inline]
    pub(super) fn of_guard(record: W) -> Self { Shaped(record) }
    /// The proved record.
    #[inline]
    pub fn record(self) -> W { self.0 }
    /// The fact this proof stands for, which a frame keeps for the later reads of the same record.
    #[inline]
    pub fn fact(self) -> ShapeFact { ShapeFact(()) }
}

/// An object record whose exact shape and prototype link one guard proved together
/// (`entry::shape_link_matches`): its holder is the one a prototype-held read was folded
/// against, so the read's constant answers for it (`entry::held_constant`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Linked<W>(W);

impl<W: Copy> Linked<W> {
    /// A record the shape-and-link guard just tested.
    #[inline]
    pub(super) fn of_guard(record: W) -> Self { Linked(record) }
    /// The record's shape proof, which the link proof includes.
    #[inline]
    pub fn shaped(self) -> Shaped<W> { Shaped(self.0) }
}

/// A typed array's elements proved for one native kind: their address, their count and the kind
/// whose element they are. A row reads or stores one at an index with the bounds test alone
/// (`Viewed::view`, then `typed_elements`' rows): the record's kind, its stamp, its attachment
/// and (to store) its unshared bytes were tested once, by the view guard (`prove_viewed`), or are
/// a fact a code dependency holds (`HeldViews::hold`). The fields are private, so a typed access
/// runs only past that guard or that fact.
#[derive(Clone, Copy)]
pub struct Viewed<W> {
    values: W,
    length: W,
    kind: super::typed_elements::NativeKind,
}

/// A fact that a typed array's view proved earlier still holds: its bytes have not moved, its
/// length has not changed, its buffer is attached and not shared. Only a view guard
/// (`Viewed::fact`) or a held code dependency (`HeldViews::hold`) makes one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypedViewFact(());

impl<W: Copy> Viewed<W> {
    /// The view the guard in this module tree (`typed_view::prove_viewed`) just tested.
    #[inline]
    pub(super) fn of_guard(values: W, length: W, kind: super::typed_elements::NativeKind) -> Self { Viewed { values, length, kind } }
    /// The view `(values, length)` of kind `kind` an earlier guard proved and a fact keeps; no test runs.
    #[inline]
    pub fn by_fact(values: W, length: W, kind: super::typed_elements::NativeKind, _fact: TypedViewFact) -> Self { Viewed { values, length, kind } }
    /// The elements' address.
    #[inline]
    pub fn values(self) -> W { self.values }
    /// The elements' count.
    #[inline]
    pub fn length(self) -> W { self.length }
    /// The kind of the elements.
    #[inline]
    pub fn kind(self) -> super::typed_elements::NativeKind { self.kind }
    /// The fact this proof stands for, which a frame keeps for the later accesses of the same array.
    #[inline]
    pub fn fact(self) -> TypedViewFact { TypedViewFact(()) }
    /// The access form the element rows take: this kind, the index still to bound.
    #[inline]
    pub fn view(self) -> super::typed_elements::TypedView<W> {
        super::typed_elements::TypedView { values: self.values, length: self.length,
            kind: super::typed_elements::ViewKind::Static(self.kind), within: false }
    }
}

impl TypedViewFact {
    /// The loop's entry proved the view once (`entry::entry_typed` reads what it recorded), and a
    /// row inside the loop reads under it. Only a stencil row makes one (and the tests).
    #[cfg(any(stencil_build, test))]
    pub(crate) const fn entry_guard() -> Self { TypedViewFact(()) }
}

/// Whether a compiled unit holds the views it proved under a code dependency
/// (`StableFacts::views`, `heap::subscribe_typed_views`): the first change of a live view's bytes
/// (detached, resized, moved, shared) retires it. Holding is the only way a dependency makes a
/// `TypedViewFact`: a view proved before a call stays proved after it with no test, the call that
/// changed one having retired the code (its invalidation word, read after the call).
#[cfg(not(stencil_build))]
#[derive(Default)]
pub struct HeldViews(bool);

#[cfg(not(stencil_build))]
impl HeldViews {
    /// Holds every view the code proved: the code depends on them from now on, and the fact is the proof.
    #[inline]
    pub fn hold(&mut self) -> TypedViewFact { self.0 = true; TypedViewFact(()) }
    /// Whether the code holds them, leaving it not.
    pub fn take(&mut self) -> bool { std::mem::take(&mut self.0) }
}
