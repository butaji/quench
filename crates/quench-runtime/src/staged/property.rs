//! The named-property family as staged rows: `GetField` in every form (own slot, prototype
//! slot, prototype constant, polymorphic arms, nested chains, a VM probe behind a fast plan),
//! `SetField` and `StoreField` (own slots in place by shape arms, a loop's projected slot, the
//! VM's own-field, bound-slot and adding stores), `Delete` and `in`. A site's proved plan is
//! static data (`FieldPlan`, `StorePlan`); each row runs it once over a stage, so the interpreter
//! (`Concrete`) and every specializer (`Emit`) read one definition of each read and store.
//! Whatever the plan cannot prove is an explicit `PropertyFrame` call-out to the VM's helper,
//! never a second copy of the lookup. What a tier proved of an object before the row runs (its
//! record, its shape, its layout) is read back through the frame, so a guard the frame holds
//! the proof of is not emitted twice.

#[path = "property_slots.rs"]
mod slots;
use slots::{formed_slot, kept_slot, word_formed_slot};
pub(super) use slots::guard_receiver_layout;
use super::ic::{self, IcOp, Reached};
use super::object::{object_record_at, Form, Layout};
use super::{value, Frame, NumericFrame, NumericStage, Operation, Stage, Step};
use crate::bytecode::{FieldBase, FieldLookup, Register};
use crate::typed_ir::Type;

/// Whose object a read starts from: what a frame keys its proofs of an object by.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Owner {
    /// The activation's bound `this`.
    This,
    Register(Register),
    /// A value no register names (a nested lookup's intermediate), which keeps no proof.
    Temporary,
}

impl Owner {
    /// The owner of a field base: `this` or a register; any other base is the tier's own.
    #[inline(always)]
    pub(crate) fn of(base: FieldBase) -> Option<Self> {
        if base == FieldBase::THIS { return Some(Self::This); }
        base.register_index().map(Self::Register)
    }
}

/// One shape family of a polymorphic read: the sorted exact shapes that take `ops`, which
/// read from the record the shapes already matched.
#[derive(Clone, Copy)]
pub struct Arm<'a> {
    pub shapes: &'a [u32],
    pub ops: &'a [IcOp<'a>],
    /// The exact shape of the prototypes whose values `ops` fold as constants, which a code
    /// dependency holds (`PropertyFrame::hold_prototype`).
    pub holds: Option<u32>,
}

/// Which of the VM's entry lookups a read takes where its plan has no op for the receiver: the
/// VM's polymorphic entries, its hashed entries, a prototype holder's entry, or an inherited
/// entry's chain walk. Each is one frame host call (`PropertyFrame::field_entry`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryKind {
    /// The site's inline megamorphic entries, own or of the immediate prototype.
    Polymorphic,
    /// The site's hashed entries past the inline ones (`Vm::polymorphic_field_hit`).
    Hashed,
    /// The site's primary holder entry, for a direct object or any other receiver.
    Holder,
    /// The site's inherited entry, walked past the immediate prototype.
    Inherited,
}

/// What holds an inherited read's receiver to the prototype its constants fold against.
#[derive(Clone, Copy)]
pub enum PrototypeProof {
    /// The holder is subscribed (`PropertyFrame::hold_prototype`): a change of its values retires the
    /// constant, and the read checks the link only through `IcOp::PrototypeConstants`.
    Watched { holder: u32 },
    /// The receiver's prototype link is `expected_link`, checked on each read (`entry::held_constant`)
    /// for a Stable fact a code's live check holds rather than a holder subscription.
    Linked { expected_link: u32 },
}

/// How a `GetField` site reads, as the proof its tier holds of the site.
#[derive(Clone, Copy)]
pub enum FieldPlan<'a> {
    /// The ops of `staged::ic` over the receiver's record: own slot, prototype slot, or a
    /// prototype constant.
    Ops(&'a [IcOp<'a>]),
    /// An own read of the receiver's record by its layout word and slot: the ops
    /// `[GuardLayout(layout), LoadSlot { slot, storage_len: slot + 1, rep: Tagged, .. }]` held by
    /// value, so a frame whose plan is run-time data (a lane record's hint) keeps it in registers
    /// rather than in a list it addresses.
    Own { layout: u32, slot: u16 },
    /// The VM's entry lookup of `kind` for the receiver (`PropertyFrame::field_entry`), with no
    /// op over its record.
    Entry(EntryKind),
    /// `ops` over the receiver's record (its hint) first; where they miss, the VM's entry lookup
    /// of `entry` (`PropertyFrame::entried`).
    Hinted { ops: &'a [IcOp<'a>], entry: EntryKind },
    /// The first arm whose shape set holds the receiver's exact shape reads; none leaves.
    Arms(&'a [Arm<'a>]),
    /// A receiver of exact `shape` whose prototype holds the values `constants` pair with the
    /// receiver's prototype link; `proof` says what holds that link to the folded holder.
    Inherited { shape: u32, constants: &'a [(u32, u64)], proof: PrototypeProof },
    /// The plan, whose receiver may be a function's record: its own fields answer too, and
    /// no direct object is proved of the receiver.
    Readable(&'a FieldPlan<'a>),
    /// The plan; where it misses, the VM's own read of `atom` through cache `site`.
    Probed { fast: &'a FieldPlan<'a>, atom: u32, site: u16 },
    /// The value an earlier read of this owner's field proved, which the frame holds
    /// (`PropertyFrame::known_value`).
    Known,
    /// A nested lookup `a.b.c`: each plan reads from the value the previous one produced.
    Chain(&'a [FieldPlan<'a>]),
    /// The VM's own cached read of `atom` through cache `site`: a dictionary object, a
    /// deeper holder, an accessor, a proxy.
    Host { atom: u32, site: u16 },
    /// The intrinsic geometry getter of `atom` on the receiver, where the frame's VM read proves
    /// it (`PropertyFrame::geometry_read`): a typed view's, DataView's, Map's or Set's geometry.
    /// Its value is a host proof (a cache of the getter's native), so it is a frame callout, not
    /// an op of `staged::ic`, which runs over records.
    Geometry { atom: u32 },
    /// The `length` of a receiver that is no array or string: an arguments object's or a user
    /// function's own data `length`, or a typed view's geometry (`PropertyFrame::length_read`).
    Length,
    /// A private name's read of the frame's record (`PropertyFrame::private_field`): the
    /// object's private field under the name's brand, which no field cache records.
    Private { atom: u32 },
}

/// One own data slot a store site proved: objects of exact `shape` keep the field at `slot`.
/// `holds` is `None` where the value stored is of a type the slot's type holds. Otherwise it
/// is the type the row tests the value for before it stores in place, so the slot's
/// representation never widens under the raw write; a value of any other type stores through
/// the VM, which joins the slot's type. `raw_double` marks a slot whose arena word is a raw
/// double (`Representation::Float64`): the store writes the value's binary64 word (`PropertyFrame::stored_number`),
/// never its tagged one, so a Number keeps no integer canonical form on its way in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StoreArm {
    pub(crate) shape: u32,
    pub(crate) slot: u16,
    pub holds: Option<Type>,
    pub(crate) raw_double: bool,
    /// Whether the site left the record's storage form open (`Form::Either`): a raw store
    /// then tests the inline flag at run time, which a proven arena run skips.
    pub(crate) unformed: bool,
    /// Whether `shape` is an ahead image's layout id of a plain arena run (`ImageWord::Layout`):
    /// its guard compares the record's own word, which proves the form, so the store tests none.
    pub layout: bool,
}

impl StoreArm {
    /// The arm for the slot `slot` of objects of `shape`, whose type is `slot_ty`, under a
    /// value of type `fact`. A slot that holds the value's type stores it as is. Otherwise
    /// the arm admits an int32, or for a slot that holds any direct object, that type; `None`
    /// where the slot's type holds no such value, so every store of it goes through the VM.
    pub fn of((shape, slot): (u32, u16), slot_ty: Type, fact: Type) -> Option<Self> {
        if fact.is_subtype(slot_ty) { return Some(Self { shape, slot, holds: None, raw_double: false, unformed: false, layout: false }); }
        let holds = if slot_ty.direct_object_part() == Some(Type::DIRECT_OBJECT_FAMILY) { slot_ty } else { Type::INT32 };
        holds.is_subtype(slot_ty).then_some(Self { shape, slot, holds: Some(holds), raw_double: false, unformed: false, layout: false })
    }

    /// `StoreArm::of` for a slot whose arena words are raw doubles where `raw_double`: only a
    /// value whose type the slot's holds, a Number, stores in place, as its binary64 word, and
    /// only into a shape whose values its `form` leaves an arena run (a packed slot holds narrow
    /// values, whatever its owner's type); an open form is tested when the store runs.
    pub fn of_rep(at: (u32, u16), slot_ty: Type, fact: Type, (raw_double, form): (bool, super::Form)) -> Option<Self> {
        let arm = Self::of(at, slot_ty, fact)?;
        if !raw_double { return Some(arm); }
        (arm.holds.is_none() && fact.is_subtype(Type::NUMBER) && form != super::Form::Packed)
            .then_some(Self { raw_double: true, unformed: form == super::Form::Either, ..arm })
    }

    /// This arm guarded by the layout id `shape` names (`layout`), where it stores a tagged word.
    pub fn as_layout(self) -> Self {
        Self { layout: !self.raw_double, ..self }
    }
}

/// How the VM completes a named store that an in-place arm does not: where no arm holds the
/// receiver's shape, or its value, its barrier or its storage.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StoreJoin {
    /// The VM's own-field store hit, which a failure leaves at the miss edge.
    Host,
    /// The same hit through the site's bound slot first, which the frame addressed before the
    /// run (`PropertyFrame::field_store_bound`).
    Bound,
    /// The VM's adding entries, which complete a store of any shape without leaving; a frame
    /// that uses it holds no object proof past the store.
    Add,
}

/// How a named store site writes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StorePlan<'a> {
    /// The first of `arms` whose exact shape the receiver has stores in place where the
    /// value, the barrier and the storage admit it; every other store runs through `join`.
    Own { arms: &'a [StoreArm], join: StoreJoin },
    /// A receiver whose layout id is `id` (a site's layout hint, `Object::hint_shape`) stores
    /// at `slot` in place: one compare of the record's own word proves its shape, form and
    /// flags, and the form the id's entry names picks the store with no test
    /// (`store::own_store`, hinted). Every other store runs through `join`. The generated
    /// own-store rows build it (`RecordPlan::own_store`).
    Layout { id: u32, slot: u16, join: StoreJoin },
    /// The slot a loop's entry projected for this store, which the frame addresses
    /// (`PropertyFrame::entry_slot_store`).
    Entry,
    /// A store that may add the property or change the shape: the VM's adding entry.
    Add,
    /// A setter, a read-only or dictionary property, a proxy: the VM's store.
    Host,
    /// The VM's store through the slot the entry bound for the site.
    Bound,
    /// A store the record's own hint (`id`, `slot`) proves for a tracked or narrow store
    /// (`narrow`), which `PropertyFrame::field_store_hinted` makes in place; a refusal leaves to
    /// `join` (the VM's own-field store).
    Hinted { id: u32, slot: u16, narrow: bool, join: StoreJoin },
    /// A tracked store through the record's own arena-form hint (`id`, `slot`) by a row installed for
    /// the slot's held type `held` (`slot_store::HELD_*`): the slot still holding that type and the
    /// value being of it, the store widens and observes nothing and is made in place
    /// (`slot_store::held_holds`); a refusal leaves at the miss edge.
    Held { id: u32, slot: u16, held: u8 },
    /// A store the record's own hint, the site's polymorphic entries or the proved add completes
    /// (`PropertyFrame::field_store_polymorphic`); a refusal leaves at the miss edge.
    Polymorphic { id: u32, slot: u16 },
}

impl<'a> StorePlan<'a> {
    /// The plan that stores in place by `arms` and otherwise by `join`; by `join` alone with
    /// no arm.
    pub fn of(arms: &'a [StoreArm], join: StoreJoin) -> Self {
        match (arms.is_empty(), join) {
            (false, join) => Self::Own { arms, join },
            (true, StoreJoin::Host) => Self::Host,
            (true, StoreJoin::Bound) => Self::Bound,
            (true, StoreJoin::Add) => Self::Add,
        }
    }
}

/// The plans a frame's tier proved, by site. A frame with none answers `None`, and the
/// operation is left to the host path.
pub trait PlanSource {
    /// The read plan of `lookup`.
    fn field_plan(&self, lookup: FieldLookup) -> Option<FieldPlan<'_>>;
    /// The store plan of cache site `site`.
    fn store_plan(&self, site: u16) -> Option<StorePlan<'_>>;
    /// The two own hops `(guard, slot, form)` of a nested read `a.b.c` the source holds as data (a
    /// lane record's hint and its site's second entry): the guard an exact shape's compare
    /// (`IcOp::GuardShape`), or a layout id's that proves the form too (`IcOp::GuardFormed`); each form
    /// the storage form its site fixed (`Form::Either` where it fixed none); `None` where it holds none.
    #[inline(always)]
    fn nested_hops(&self) -> Option<[(IcOp<'static>, u16, Form); 2]> { None }
}

impl<P: PlanSource> PlanSource for &P {
    fn field_plan(&self, lookup: FieldLookup) -> Option<FieldPlan<'_>> { (**self).field_plan(lookup) }
    fn store_plan(&self, site: u16) -> Option<StorePlan<'_>> { (**self).store_plan(site) }
    #[inline(always)]
    fn nested_hops(&self) -> Option<[(IcOp<'static>, u16, Form); 2]> { (**self).nested_hops() }
}

/// The plans of a frame that proved none.
#[derive(Clone, Copy, Default)]
pub struct NoPlans;

impl PlanSource for NoPlans {
    fn field_plan(&self, _lookup: FieldLookup) -> Option<FieldPlan<'_>> { None }
    fn store_plan(&self, _site: u16) -> Option<StorePlan<'_>> { None }
}

/// A frame's property callouts. Every default declines, leaving the operation to the host
/// path; a miss edge of the stage is each callout's failure.
pub trait PropertyFrame<S: Stage>: Frame<S> {
    /// The handle to the plans, copied out so a row holds a plan while it calls the frame.
    type Plans: PlanSource + Copy;

    /// The plans this frame's tier proved.
    fn plans(&self) -> Self::Plans;

    /// What the frame proved of the type of `register`'s value: `Type::ANY` where it proved
    /// nothing, which a frame that knows no static facts (the interpreter) always answers. A
    /// store folds what the type decides (the barrier of an immediate, the narrow encoder of an
    /// int32) and tests the rest dynamically.
    #[inline(always)]
    fn value_fact(&self, _register: Register) -> Type { Type::ANY }

    /// The binary64 word of `register`, a Number by its fact, which a raw-double slot stores
    /// (`StoreArm::raw_double`); `None` where the frame keeps no such view, so the store leaves.
    #[inline(always)]
    fn stored_number(&mut self, _stage: &mut S, _register: Register, _miss: S::Miss) -> Option<S::Word> { None }

    /// The base of the object record table, which prototype reads and stores address.
    #[inline(always)]
    fn object_records(&mut self, _stage: &mut S) -> Option<S::Word> { None }

    /// The bound `this` a field access names; a frame whose entry proved it bound answers it
    /// without the test. An unbound `this` leaves at `miss`.
    #[inline(always)]
    fn field_this(&mut self, stage: &mut S, miss: S::Miss) -> Option<S::Word> where Self: Sized {
        super::register_frame::bound_this(stage, self, miss)
    }

    /// The record of `owner`'s `object`, with the words the frame already holds of it (its
    /// shape and inline flag). A direct object, else a leave at `miss`; where `readable`, a
    /// function's record answers too, for a frame that proves one.
    #[inline(always)]
    fn receiver_record(&mut self, stage: &mut S, _owner: Owner, object: S::Word, _readable: bool,
        miss: S::Miss) -> Option<Reached<S::Word, S::Cond>> where Self: Sized {
        receiver(stage, self, object, miss)
    }

    /// The base object of a nested lookup `a.b.c`, which the frame's record names: the first
    /// link reads from it. A frame that names none leaves.
    #[inline(always)]
    fn nested_base(&mut self, _stage: &mut S, _miss: S::Miss) -> Option<S::Word> { None }

    /// The exact shape `owner`'s object is proved to have, which makes a guard of it redundant.
    #[inline(always)]
    fn proven_shape(&mut self, _stage: &mut S, _owner: Owner) -> Option<(u32, super::ShapeFact)> { None }

    /// Records that `owner`'s object has `shape`, by the guard's proof that tested it; `direct`
    /// where the plan proves the receiver a direct object, not a function's record.
    #[inline(always)]
    fn prove_shape(&mut self, _stage: &mut S, _owner: Owner, _proof: (u32, super::Shaped<S::Word>), _direct: bool) {}

    /// The layout proved of `object` (the word of `owner`'s object) of exact `shape`, which a read reuses.
    #[inline(always)]
    fn kept_layout(&self, _owner: Owner, _object: S::Word, _shape: u32) -> Option<Layout<S::Word, S::Cond>> { None }

    /// Records the layout a read proved of `object` (the word of `owner`'s object) of exact `shape`.
    #[inline(always)]
    fn keep_layout(&mut self, _owner: Owner, _object: S::Word, _proof: (u32, super::ShapeFact), _layout: Layout<S::Word, S::Cond>) {}

    /// Holds the values of the prototypes of exact shape `shape` as constants, under a code
    /// dependency. A frame that takes none answers `None`.
    #[inline(always)]
    fn hold_prototype(&mut self, _stage: &mut S, _shape: u32, _miss: S::Miss) -> Option<()> { None }

    /// The word of local slot `slot` a field read's base names (`FieldBase::local`), which a
    /// frame that holds no readable local answers `None` (the read leaves at its miss).
    #[inline(always)]
    fn local_base(&mut self, _stage: &mut S, _slot: u16, _miss: S::Miss) -> Option<S::Word> { None }

    /// The value an earlier read of `owner`'s field proved, which no write has retired.
    #[inline(always)]
    fn known_value(&mut self, _stage: &mut S, _owner: Owner) -> Option<S::Word> { None }

    /// The value a loop's entry read of `lookup` holds, for an access the loop repeats unchanged.
    #[inline(always)]
    fn hoisted(&mut self, _stage: &mut S, _lookup: FieldLookup) -> Option<S::Word> { None }

    /// The value the field `lookup` named held in the entry record of an OSR region.
    #[inline(always)]
    fn entry_field(&mut self, _stage: &mut S, _lookup: FieldLookup) -> Option<S::Word> { None }

    /// The VM's read of `atom` of `owner`'s `object` through `site`, which a frame that bound
    /// the site's slot answers inline first.
    #[inline(always)]
    fn field_probe(&mut self, stage: &mut S, _owner: Owner, object: S::Word, atom: u32, site: u16,
        miss: S::Miss) -> Option<S::Word> {
        self.field_get_host(stage, object, atom, site, miss)
    }

    /// What `fast` reads, which leaves at the edge it is given; where it misses, the VM's
    /// probe instead (`field_probe`), and past the join only what both prove holds.
    #[inline(always)]
    fn probed(&mut self, stage: &mut S, owner: Owner, object: S::Word, (atom, site): (u32, u16), miss: S::Miss,
        fast: impl FnOnce(&mut S, &mut Self, S::Miss) -> Option<S::Word>) -> Option<S::Word> {
        match fast(stage, self, miss) {
            Some(word) => Some(word),
            None => self.field_probe(stage, owner, object, atom, site, miss),
        }
    }

    /// The intrinsic geometry getter of `atom` on the receiver `object` (`FieldPlan::Geometry`),
    /// where the VM's pure native read answers it; a receiver that shadows the name, or any
    /// other, leaves at `miss`.
    #[inline(always)]
    fn geometry_read(&mut self, _stage: &mut S, _object: S::Word, _atom: u32, _miss: S::Miss) -> Option<S::Word> { None }

    /// The `length` of a receiver that is no array or string (`FieldPlan::Length`): an
    /// arguments object's or a user function's own data `length`, or a typed view's geometry;
    /// any other receiver leaves at `miss`.
    #[inline(always)]
    fn length_read(&mut self, _stage: &mut S, _object: S::Word, _miss: S::Miss) -> Option<S::Word> { None }
    /// What `fast` reads, which leaves at the edge it is given; where it misses, the VM's entry
    /// lookup of `kind` (`field_entry`) instead.
    #[inline(always)]
    fn entried(&mut self, stage: &mut S, owner: Owner, object: S::Word, kind: EntryKind, miss: S::Miss,
        fast: impl FnOnce(&mut S, &mut Self, S::Miss) -> Option<S::Word>) -> Option<S::Word> {
        match fast(stage, self, miss) {
            Some(word) => Some(word),
            None => self.field_entry(stage, owner, object, kind, miss),
        }
    }

    /// The VM's entry lookup of `kind` for `owner`'s `object` (`EntryKind`), a host call whose
    /// answer the frame's record keeps where the VM keeps a hint for it; a miss leaves at `miss`.
    #[inline(always)]
    fn field_entry(&mut self, _stage: &mut S, _owner: Owner, _object: S::Word, _kind: EntryKind, _miss: S::Miss) -> Option<S::Word> { None }

    /// The VM's cached read of `atom` of `object` through `site` (`LaneHelper::FieldGet`); a
    /// miss leaves at `miss`.
    #[inline(always)]
    fn field_get_host(&mut self, _stage: &mut S, _object: S::Word, _atom: u32, _site: u16, _miss: S::Miss) -> Option<S::Word> { None }

    /// The private field `atom` of `object` at the frame's record, under the brand its
    /// environment finds (`FieldPlan::Private`); an object without the field, or a private
    /// method or accessor, leaves at `miss`.
    #[inline(always)]
    fn private_field(&mut self, _stage: &mut S, _object: S::Word, _atom: u32, _miss: S::Miss) -> Option<S::Word> { None }

    /// The VM's store of `value` to `atom` of `object` through `site`, which may add the
    /// property (`LaneHelper::FieldAdd`); a failure leaves at `miss`.
    #[inline(always)]
    fn field_add(&mut self, _stage: &mut S, _object: S::Word, _atom: u32, _site: u16, _value: S::Word, _miss: S::Miss) -> Option<()> { None }

    /// The VM's own-field store hit (`LaneHelper::FieldSet`); a failure leaves at `miss`.
    #[inline(always)]
    fn field_store_host(&mut self, _stage: &mut S, _object: S::Word, _atom: u32, _site: u16, _value: S::Word, _miss: S::Miss) -> Option<()> { None }

    /// The store of `value` to `atom` of `object` through the record's own hint `(id, slot)`, in a
    /// tracked store (`narrow` false) or a narrow one (`narrow`) (`StorePlan::Hinted`); `None`
    /// where the VM declines, which leaves to the plan's join.
    #[inline(always)]
    fn field_store_hinted(&mut self, _stage: &mut S, _object: S::Word, _atom: u32, _site: u16, _value: S::Word,
        _hint: (u32, u16), _narrow: bool) -> Option<()> { None }

    /// The store of `value` to `atom` of `object` through the record's own hint, then the site's
    /// polymorphic entries, then the proved add (`StorePlan::Polymorphic`); a refusal leaves at the
    /// miss edge.
    #[inline(always)]
    fn field_store_polymorphic(&mut self, _stage: &mut S, _object: S::Word, _atom: u32, _site: u16, _value: S::Word,
        _hint: (u32, u16)) -> Option<()> { None }
    /// The VM's Math call of `callee` on the first `count` of `arguments` (`CallOut`'s Math
    /// function of Numbers, `number::math_inline_call`), which runs no guest code: the value, or
    /// `None` where the call is not one the frame computes in place. The callee's identity is
    /// the VM's, read from the function record, so no row names the native.
    #[inline(always)]
    fn math_call(&mut self, _stage: &mut S, _callee: S::Word, _arguments: [S::Word; 2], _count: usize) -> Option<S::Word> { None }

    /// The VM's own-field store of `value` to `atom` on `this` through `site`, the store a read
    /// that stores its value to `this` makes after the read (`primary_own_field_store_hit`
    /// without widening); `None` leaves the store to the general path.
    #[inline(always)]
    fn sink_store(&mut self, _stage: &mut S, _atom: u32, _site: u16, _value: S::Word) -> Option<()> { None }

    /// The VM's own-field store hit of `value`, of type `fact`, through the slot the entry bound
    /// for `site`, which a frame that bound one stores into inline first (a value its writes
    /// admit); the plain hit otherwise.
    #[inline(always)]
    fn field_store_bound(&mut self, stage: &mut S, _owner: Owner, object: S::Word, (atom, site): (u32, u16),
        (value, _fact): (S::Word, Type), miss: S::Miss) -> Option<()> {
        self.field_store_host(stage, object, atom, site, value, miss)
    }

    /// Stores `value` through the slot a loop's entry projected for the access at `site`, which
    /// a frame that made the projection (`StorePlan::Entry`) addresses; a failure leaves at `miss`.
    #[inline(always)]
    fn entry_slot_store(&mut self, _stage: &mut S, _site: u16, _value: S::Word, _miss: S::Miss) -> Option<()> { None }

    /// Whether the slot a loop's entry projected for this store keeps raw doubles: the store
    /// then hands `entry_slot_store` the value's binary64 word, not its tagged one.
    #[inline(always)]
    fn entry_slot_raw(&self) -> bool { false }

    /// Continues where the run admits a direct store of the atom the access names (no method
    /// cache names it); a frame whose run changes that between accesses tests it here.
    #[inline(always)]
    fn store_admitted(&mut self, _stage: &mut S, _miss: S::Miss) -> Option<()> { Some(()) }

    /// `store_admitted` for an in-place store, split: what every store needs is tested here; the
    /// test that no method cache names the atom is answered instead, for the arm to take unless
    /// the store is quiet (`store::admit_named`). `None` inside where nothing is left to test.
    #[inline(always)]
    fn store_admission(&mut self, stage: &mut S, miss: S::Miss) -> Option<Option<S::Cond>> {
        self.store_admitted(stage, miss)?;
        Some(None)
    }

    /// What `fast` stores, which leaves at the edge it is given; where it misses, `slow` (the
    /// VM's store) instead, and past the join only what both prove holds.
    #[inline(always)]
    fn stored(&mut self, stage: &mut S, _owner: Owner, miss: S::Miss,
        fast: impl FnOnce(&mut S, &mut Self, S::Miss) -> Option<()>,
        slow: impl FnOnce(&mut S, &mut Self, S::Miss) -> Option<()>) -> Option<()> {
        match fast(stage, self, miss) {
            Some(()) => Some(()),
            None => slow(stage, self, miss),
        }
    }

    /// `delete base[key]` where the frame's own deletion answers it (an ordinary object's
    /// configurable property, which runs no guest code): the boolean word with its fact; `None`
    /// leaves the deletion to the VM's call-out (`CallOut::Delete`), which refuses by `strict`.
    #[inline(always)]
    fn delete_property(&mut self, _stage: &mut S, _base: S::Word, _key: S::Word, _strict: bool, _miss: S::Miss)
        -> Option<(S::Word, Self::Fact)> { None }

    /// `base[key]` where no dense or typed element read answers it: a hole read through the
    /// prototype chain, an index past a typed array, a string's character, a keyed own read.
    #[inline(always)]
    fn index_get_host(&mut self, _stage: &mut S, _base: S::Word, _key: S::Word, _miss: S::Miss) -> Option<S::Word> { None }
}

/// The record of the direct object `object`; anything else leaves at `miss`.
#[inline(always)]
pub fn receiver<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, object: S::Word,
    miss: S::Miss) -> Option<Reached<S::Word, S::Cond>> {
    let records = frame.object_records(stage)?;
    let direct = value::is_direct_object(stage, object);
    stage.guard(direct, miss)?;
    let id = stage.and_imm(object, u64::from(u32::MAX));
    Some(Reached::bare(object_record_at(stage, records, id)))
}

/// The base of a field access: the activation's bound `this`, or a register's tagged word;
/// a local or nested base is the tier's own.
#[inline(always)]
pub(super) fn base_word<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, base: FieldBase, miss: S::Miss) -> Option<S::Word> {
    if base == FieldBase::THIS {
        return frame.field_this(stage, miss);
    }
    Some(frame.tagged(stage, base.register_index()?))
}

/// The value `plan` reads from `owner`'s `object`, or a leave at `miss`; `readable` where the
/// plan admits a function's record as the receiver. Inlined, so a tier whose plan is static data
/// (a stencil row's) folds the match to its one arm; the recursive plans re-enter through
/// `read_nested`, so this function never calls itself and stays inlinable.
#[inline(always)]
pub(super) fn read_with<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, (owner, readable): (Owner, bool),
    object: S::Word, plan: FieldPlan<'_>, miss: S::Miss) -> Option<S::Word> {
    match plan {
        FieldPlan::Ops(ops) => {
            let at = frame.receiver_record(stage, owner, object, readable, miss)?;
            let records = frame.object_records(stage);
            run_owned(stage, frame, (owner, !readable), object, ops, (at, records), miss)
        }
        FieldPlan::Own { layout, slot } => {
            let at = frame.receiver_record(stage, owner, object, readable, miss)?;
            own_slot(stage, frame, (owner, !readable), object, &at, (layout, slot), miss)
        }
        FieldPlan::Arms(list) => {
            let at = frame.receiver_record(stage, owner, object, readable, miss)?;
            for held in list.iter().filter_map(|arm| arm.holds) { frame.hold_prototype(stage, held, miss)?; }
            let records = frame.object_records(stage);
            let proven = frame.proven_shape(stage, owner);
            if let Some((arm, fact)) = proven.and_then(|(shape, fact)| list.iter().find(|arm| arm.shapes.contains(&shape)).map(|arm| (arm, fact))) {
                return ic::run(stage, arm.ops, Reached { shaped: Some(super::Shaped::by_fact(at.record, fact)), ..at }, records, miss);
            }
            let shape = ic::shape_word(stage, &at);
            arms(stage, list, Reached { shape: Some(shape), ..at }, records, miss)
        }
        FieldPlan::Inherited { shape, constants, proof } =>
            inherited(stage, frame, (owner, readable), object, (shape, constants, proof), miss),
        FieldPlan::Readable(inner) => read_nested(stage, frame, (owner, true), object, *inner, miss),
        FieldPlan::Probed { fast, atom, site } => frame.probed(stage, owner, object, (atom, site), miss,
            #[inline(always)] |stage, frame, miss| read_nested(stage, frame, (owner, readable), object, *fast, miss)),
        FieldPlan::Known => frame.known_value(stage, owner),
        FieldPlan::Chain(links) => chain(stage, frame, (owner, readable), object, links, miss),
        FieldPlan::Host { atom, site } => frame.field_probe(stage, owner, object, atom, site, miss),
        FieldPlan::Geometry { atom } => frame.geometry_read(stage, object, atom, miss),
        FieldPlan::Length => frame.length_read(stage, object, miss),
        FieldPlan::Entry(kind) => frame.field_entry(stage, owner, object, kind, miss),
        FieldPlan::Hinted { ops, entry } => frame.entried(stage, owner, object, entry, miss, #[inline(always)] |stage, frame, miss| {
            let at = frame.receiver_record(stage, owner, object, readable, miss)?;
            let records = frame.object_records(stage);
            run_owned(stage, frame, (owner, !readable), object, ops, (at, records), miss)
        }),
        FieldPlan::Private { atom } => frame.private_field(stage, object, atom, miss),
    }
}

/// A prototype-held read (`FieldPlan::Inherited`) of `owner`'s `object`: the receiver's exact shape and the
/// holder its proof names, then the constant the holder held.
#[inline(always)]
fn inherited<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, (owner, readable): (Owner, bool), object: S::Word,
    (shape, constants, proof): (u32, &[(u32, u64)], PrototypeProof), miss: S::Miss) -> Option<S::Word> {
    let at = frame.receiver_record(stage, owner, object, readable, miss)?;
    match proof {
        PrototypeProof::Watched { holder } => {
            let shaped = guard_receiver_shape(stage, frame, (owner, !readable), &at, shape, miss)?;
            frame.hold_prototype(stage, holder, miss)?;
            ic::run(stage, &[IcOp::PrototypeConstants(constants)], Reached { shaped: Some(shaped), ..Reached::bare(at.record) }, None, miss)
        }
        PrototypeProof::Linked { expected_link } => {
            let &[(_, value)] = constants else { return None };
            super::entry::shape_link_matches(stage, at.record, (u64::from(shape), u64::from(expected_link)), miss).map(|linked| super::entry::held_constant(stage, linked, value))
        }
    }
}

/// `read_with` of a plan nested in another (`Readable`, `Probed`, a `Chain` link): the one
/// out-of-line entry of the recursion.
fn read_nested<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, owner: (Owner, bool),
    object: S::Word, plan: FieldPlan<'_>, miss: S::Miss) -> Option<S::Word> {
    read_with(stage, frame, owner, object, plan, miss)
}

/// Continues where `owner`'s object has exact shape `shape`: a proof the frame holds emits
/// nothing, else the guard is made and recorded. `direct` where the receiver is a direct object.
#[inline(always)]
pub(super) fn guard_receiver_shape<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, (owner, direct): (Owner, bool),
    at: &Reached<S::Word, S::Cond>, shape: u32, miss: S::Miss) -> Option<super::Shaped<S::Word>> {
    if let Some((proven, fact)) = frame.proven_shape(stage, owner) && proven == shape { return Some(super::Shaped::by_fact(at.record, fact)); }
    let shaped = ic::guard_shape(stage, at, shape, miss)?;
    frame.prove_shape(stage, owner, (shape, shaped), direct);
    Some(shaped)
}

/// `run_owned` of `FieldPlan::Own`: the layout guard and the slot it proves.
#[inline(always)]
fn own_slot<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, owned: (Owner, bool), object: S::Word,
    at: &Reached<S::Word, S::Cond>, (layout, slot): (u32, u16), miss: S::Miss) -> Option<S::Word> {
    // `run_owned`'s arm of the same two ops.
    word_formed_slot(stage, frame, owned, object, at, layout, (slot, crate::typed_ir::Representation::Tagged, usize::from(slot) + 1), miss)
}

/// `ops` over the record `at` reached, where a leading shape guard is the owner's proof: its
/// own-slot load reads through the layout the frame kept, or proves and keeps it.
#[inline(always)]
fn run_owned<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, (owner, direct): (Owner, bool), object: S::Word,
    ops: &[IcOp<'_>], (at, records): (Reached<S::Word, S::Cond>, Option<S::Word>), miss: S::Miss) -> Option<S::Word> {
    if let [IcOp::GuardLayout(word), IcOp::LoadSlot { slot, storage_len, rep, .. }] = ops {
        return word_formed_slot(stage, frame, (owner, direct), object, &at, *word, (*slot, *rep, *storage_len), miss);
    }
    if let [IcOp::GuardImageLayout(key), IcOp::LoadSlot { slot, storage_len, rep, .. }] = ops {
        return slots::image_layout_slot(stage, frame, (owner, true), object, &at, *key, (*slot, *rep, *storage_len), miss);
    }
    let [IcOp::GuardShape(shape), rest @ ..] = ops else { return ic::run(stage, ops, at, records, miss) };
    if direct && let [IcOp::LoadSlot { slot, form: Form::Arena, rep, storage_len }] = rest
        && frame.proven_shape(stage, owner).map(|(proven, _)| proven) != Some(*shape)
        && frame.kept_layout(owner, object, *shape).is_none() {
        return formed_slot(stage, frame, (owner, direct, *shape), object, &at, (*slot, *rep, *storage_len), miss);
    }
    let at = Reached { shaped: Some(guard_receiver_shape(stage, frame, (owner, direct), &at, *shape, miss)?), ..at };
    match rest {
        [IcOp::LoadSlot { slot, form, storage_len, rep }] =>
            kept_slot(stage, frame, (owner, *shape), object, &at, (*slot, *form, *storage_len, *rep), miss),
        _ => ic::run_unrolled(stage, rest, at, records, miss),
    }
}

/// The first of `plans` whose shape set holds the record's shape reads it; the last is the guard of the
/// whole. The member test is the arm's shape guard (`Shaped`): its shapes share the slots it reads.
#[inline(always)]
fn arms<S: Stage>(stage: &mut S, plans: &[Arm<'_>], at: Reached<S::Word, S::Cond>, records: Option<S::Word>,
    miss: S::Miss) -> Option<S::Word> {
    let ((arm, rest), shaped) = (plans.split_first()?, Reached { shaped: Some(super::Shaped::of_guard(at.record)), ..at });
    let exact = ic::shape_word(stage, &at);
    let member = stage.shape_in_site(exact, arm.shapes);
    if rest.is_empty() {
        stage.guard(member, miss)?;
        return ic::run(stage, arm.ops, shaped, records, miss);
    }
    stage.choose_leaving(member, #[inline(always)] |stage| ic::run(stage, arm.ops, shaped, records, miss),
        #[inline(always)] |stage| arms(stage, rest, at, records, miss))
}

/// A nested lookup: each link reads from the previous link's value, the first from `owner`'s
/// object and the rest from objects no register names.
#[inline(always)]
fn chain<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, first: (Owner, bool), base: S::Word,
    links: &[FieldPlan<'_>], miss: S::Miss) -> Option<S::Word> {
    let mut object = base;
    let mut from = first;
    for link in links {
        object = read_nested(stage, frame, from, object, *link, miss)?;
        from = (Owner::Temporary, true);
    }
    Some(object)
}

/// One own hop of a nested read: `object`'s record, under `guard`, its `slot` in `form`.
#[inline(always)]
fn nested_hop<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, object: S::Word,
    (guard, slot, form): (IcOp<'static>, u16, Form), miss: S::Miss) -> Option<S::Word> {
    let at = frame.receiver_record(stage, Owner::Temporary, object, false, miss)?;
    let ops = [guard, IcOp::LoadSlot { slot, form, storage_len: usize::from(slot) + 1, rep: crate::typed_ir::Representation::Tagged }];
    ic::run(stage, &ops, at, None, miss)
}

/// `GetField`: `base`'s value, as the loop's entry or the record of an OSR region already
/// holds it, else read through `lookup`'s plan.
#[inline(always)]
pub(super) fn get_field<S: Stage, F: PropertyFrame<S>>(stage: &mut S, frame: &mut F, base: FieldBase, lookup: FieldLookup,
    miss: S::Miss) -> Option<S::Word> {
    if let Some(word) = frame.hoisted(stage, lookup) { return Some(word); }
    if let Some(word) = frame.entry_field(stage, lookup) { return Some(word); }
    if base.0 == FieldBase::NESTED {
        // A nested lookup's base is the site's, which the frame names; its links keep no proof.
        // Two own hops inline, each one exact-shape compare and one slot load, as the hand read.
        // The two hops written out, not looped, so each hop's guard folds to its own compare.
        let [first, second] = frame.plans().nested_hops()?;
        let base = frame.nested_base(stage, miss)?;
        let middle = nested_hop(stage, frame, base, first, miss)?;
        return nested_hop(stage, frame, middle, second, miss);
    }
    // A local base is read through the frame's local (`PropertyFrame::local_base`), which keeps no
    // proof of its object, as a temporary's.
    let local = base.local_index();
    let owner = match local {
        Some(_) => Owner::Temporary,
        None => Owner::of(base)?,
    };
    let plans = frame.plans();
    let plan = plans.field_plan(lookup)?;
    let object = match local {
        Some(slot) => frame.local_base(stage, slot, miss)?,
        None => base_word(stage, frame, base, miss)?,
    };
    // A plan of ops over the receiver's own record, the one a lane record's hint holds, inline;
    // every other plan through `read_with`, which recurses through nested plans.
    if let FieldPlan::Ops(ops) = plan {
        let at = frame.receiver_record(stage, owner, object, false, miss)?;
        let records = frame.object_records(stage);
        return run_owned(stage, frame, (owner, true), object, ops, (at, records), miss);
    }
    if let FieldPlan::Own { layout, slot } = plan {
        let at = frame.receiver_record(stage, owner, object, false, miss)?;
        return own_slot(stage, frame, (owner, true), object, &at, (layout, slot), miss);
    }
    // A prototype-held read, the other plan a row's holes name, inline too: only nested plans recurse.
    if let FieldPlan::Inherited { shape, constants, proof } = plan {
        return inherited(stage, frame, (owner, false), object, (shape, constants, proof), miss);
    }
    // The polymorphic read (the record's hint, then the VM's entries) and the entry reads inline as
    // well: through `read_with`, out of line, the row built the plan in memory (a 1.5 KB frame,
    // ~165 Ir) at every hint miss.
    if let FieldPlan::Hinted { ops, entry } = plan {
        return frame.entried(stage, owner, object, entry, miss, #[inline(always)] |stage, frame, miss| {
            let at = frame.receiver_record(stage, owner, object, false, miss)?;
            let records = frame.object_records(stage);
            run_owned(stage, frame, (owner, true), object, ops, (at, records), miss)
        });
    }
    if let FieldPlan::Entry(kind) = plan {
        return frame.field_entry(stage, owner, object, kind, miss);
    }
    read_with(stage, frame, (owner, false), object, plan, miss)
}

/// The property operations of `step`: the word each produces, or `Unresolved` where the
/// frame holds no plan or callout for it.
#[inline(always)]
pub(super) fn property_step_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let word = match operation {
        // A read its plan leaves is the VM's named read, where the frame takes it (`CallOut::LoadField`).
        Operation::GetField { base, lookup, destination } => if const { super::op_kind::admits(KIND, &[super::op_kind::GET_FIELD]) } { match get_field(stage, frame, base, lookup, miss) {
            Some(word) => Some(word),
            None => return super::callout::loading(stage, frame, super::CallOut::LoadField { lookup }, destination),
        } } else { unreachable!() },
        // A store its plan leaves is the VM's named store, where the frame takes it (`CallOut::StoreField`). A store
        // to `this` has no base register: the VM takes `this` where the frame holds it (`Frame::this_value`).
        Operation::SetField { value, base, atom, site, strict } => if const { super::op_kind::admits(KIND, &[super::op_kind::SET_FIELD]) } { match super::property_write::set_field(stage, frame, (value, base), (atom, site), miss) {
            Some(word) => Some(word),
            None => match base {
                Some(object) => super::callout::storing(stage, frame,
                    super::CallOut::StoreField { object: Some(object), value, atom, site, strict }),
                None => frame.this_value(stage).and_then(|_| super::callout::storing(stage, frame,
                    super::CallOut::StoreField { object: None, value, atom, site, strict })),
            },
        } } else { unreachable!() },
        Operation::StoreField { base, value, fact, atom, site } =>
            if const { super::op_kind::admits(KIND, &[super::op_kind::STORE_FIELD]) } { super::property_write::store_word(stage, frame, base, (value, fact), (atom, site), miss) } else { unreachable!() },
        Operation::FieldGetHost { object, atom, site } => if const { super::op_kind::admits(KIND, &[super::op_kind::FIELD_GET_HOST]) } { frame.field_probe(stage, Owner::Temporary, object, atom, site, miss) } else { unreachable!() },
        Operation::FieldAdd { object, atom, site, value } => if const { super::op_kind::admits(KIND, &[super::op_kind::FIELD_ADD]) } { frame.field_add(stage, object, atom, site, value, miss).map(|()| value) } else { unreachable!() },
        Operation::IndexGetHost { base, key } => if const { super::op_kind::admits(KIND, &[super::op_kind::INDEX_GET_HOST]) } { frame.index_get_host(stage, base, key, miss) } else { unreachable!() },
        _ => if const { super::op_kind::outside(KIND, &[super::op_kind::GET_FIELD, super::op_kind::SET_FIELD, super::op_kind::STORE_FIELD, super::op_kind::FIELD_GET_HOST, super::op_kind::FIELD_ADD, super::op_kind::INDEX_GET_HOST]) } { None } else { unreachable!() },
    };
    word.map_or(Step::Unresolved, Step::Value)
}

/// `property_step_of` for any operation.
#[inline(always)]
pub(super) fn property_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F,
    operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    property_step_of::<{ super::op_kind::ANY }, S, F>(stage, frame, operation)
}
