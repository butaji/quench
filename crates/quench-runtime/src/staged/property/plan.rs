//! The proved plans a field access runs: whose object it starts from, its arms and its prototype proof.

use super::*;

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

/// One class arm of a polymorphic read whose sites hold live class cells: the cells of class `class` take `ops`,
/// which read from the cell the class already matched (`Shaped::of_cell`).
#[derive(Clone, Copy)]
pub struct ClassArm<'a> {
    pub class: u32,
    pub ops: &'a [IcOp<'a>],
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
    /// A site whose receivers are live class cells as well as records: a direct object's cell class is dispatched
    /// over `classes` (the first arm of its class reads), and where no arm takes it, or the receiver is no direct
    /// object, `rest` reads (the shape plan, `Arms` or `Probed` for the records).
    Classed { classes: &'a [ClassArm<'a>], rest: &'a FieldPlan<'a> },
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
