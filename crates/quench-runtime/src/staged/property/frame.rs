//! The `PropertyFrame` trait: the per-frame proofs and call-outs a named-property row uses.

use super::*;

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

    /// The base of the object record table, which prototype reads and stores address.
    #[inline(always)]
    fn object_records(&mut self, _stage: &mut S) -> Option<S::Word> { None }

    /// The bound `this` a field access names; a frame whose entry proved it bound answers it
    /// without the test. An unbound `this` leaves at `miss`.
    #[inline(always)]
    fn field_this(&mut self, stage: &mut S, miss: S::Miss) -> Option<S::Word> where Self: Sized {
        super::super::register_frame::bound_this(stage, self, miss)
    }

    /// The record of `owner`'s `object`, with the words the frame already holds of it (its
    /// shape and inline flag). A direct object, else a leave at `miss`; where `readable`, a
    /// function's record answers too, for a frame that proves one.
    #[inline(always)]
    fn receiver_record(&mut self, stage: &mut S, _owner: Owner, _object: S::Word, _readable: bool,
        miss: S::Miss) -> Option<Reached<S::Word, S::Cond>> where Self: Sized {
        // A direct object is a live class cell with no record (`receiver_cell`), so the default has no record to name.
        super::guard::no_record(stage, miss)?;
        None
    }

    /// The live class cell of `owner`'s `object` (`object::cell_of`), where a class-keyed plan reads it: a direct
    /// object, else a leave at `miss`. The plan's class guard after it proves the cell live and of the key's class.
    #[inline(always)]
    fn receiver_cell(&mut self, stage: &mut S, object: S::Word, miss: S::Miss) -> Option<Reached<S::Word, S::Cond>> where Self: Sized {
        let direct = super::super::value::is_direct_object(stage, object);
        stage.guard(direct, miss)?;
        Some(Reached::bare(super::super::object::cell_of(stage, object)))
    }

    /// The base object of a nested lookup `a.b.c`, which the frame's record names: the first
    /// link reads from it. A frame that names none leaves.
    #[inline(always)]
    fn nested_base(&mut self, _stage: &mut S, _miss: S::Miss) -> Option<S::Word> { None }

    /// The exact shape `owner`'s object is proved to have, which makes a guard of it redundant.
    #[inline(always)]
    fn proven_shape(&mut self, _stage: &mut S, _owner: Owner) -> Option<(u32, super::super::ShapeFact)> { None }

    /// Records that `owner`'s object has `shape`, by the guard's proof that tested it; `direct`
    /// where the plan proves the receiver a direct object, not a function's record.
    #[inline(always)]
    fn prove_shape(&mut self, _stage: &mut S, _owner: Owner, _proof: (u32, super::super::Shaped<S::Word>), _direct: bool) {}

    /// The layout proved of `object` (the word of `owner`'s object) of exact `shape`, which a read reuses.
    #[inline(always)]
    fn kept_layout(&self, _owner: Owner, _object: S::Word, _shape: u32) -> Option<Layout<S::Word, S::Cond>> { None }

    /// Records the layout a read proved of `object` (the word of `owner`'s object) of exact `shape`.
    #[inline(always)]
    fn keep_layout(&mut self, _owner: Owner, _object: S::Word, _proof: (u32, super::super::ShapeFact), _layout: Layout<S::Word, S::Cond>) {}

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
