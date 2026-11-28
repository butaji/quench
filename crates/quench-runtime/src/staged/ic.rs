//! Inline caches as data (CacheIR): a property read a site proved, as a short list of ops
//! that every tier runs from this one definition, the interpreter through `Concrete` and the
//! native tiers through `Emit`. A tier records a site's state as its op list instead of
//! writing the read again.

use super::Stage;
use super::region::Region;
use super::object::{Form, PackedAccess, formed_own_layout, object_record_at, recorded_shape, proto_link, proto_object_record_id, read_slot};

pub use super::ic_read::*;
use crate::heap::NATIVE_OBJECT_LAYOUT as OBJECT;
use crate::typed_ir::Representation;

/// One step of a proved property read, over the record the read has reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IcOp<'a> {
    /// The record's exact shape (`Object::exact_shape`) is this one, else the read misses.
    GuardShape(u32),
    /// The record's layout id is this one (`Object::hint_shape`, a lane record's own-field
    /// hint), else the read misses: one compare of the record's own word proves the exact
    /// shape, the storage form, the prototype link and the flags, and the `LoadSlot` after it
    /// reads the form's layout, which the id's table entry names, with no test.
    GuardLayout(u32),
    /// `GuardLayout` of a layout whose storage form the site fixed (a generated row's site
    /// fact): the form is a constant of the row, so the load after it reads no table entry.
    GuardFormed { id: u32, form: u32 },
    /// The record's whole auxiliary word, its exact shape and its storage-form bits, is this shape's with no
    /// form bit set, else the read misses: one compare proves the shape and that the values are a plain arena
    /// run (no extras, not inline, not narrow), so the `LoadSlot` after it reads the run with no form test. A
    /// site whose observed form is that run (`Form::Arena`) guards this way: a record in another form missed
    /// the run's form tests anyway (`object::plain_slots`), so only the tests go.
    GuardShapeFormed(u32),
    /// The reached cell's class (`object::cell_class`) is this one, else the read misses. The reached word is a
    /// class cell (`object::cell_of`), so the guard is one compare of its header's low half: a departed object's
    /// header holds `DEPARTED_CLASS`, which no class guard names, so it misses here and takes the record path.
    GuardClass(u32),
    /// `GuardClass` of the class an ahead image's loader resolves for field guard key `key` (`ImageWord::Layout`,
    /// the class id it names on this heap), for the image's construction and its fields.
    GuardImageClass(u32),
    /// The value at `slot` of the class cell the guard before it proved (`object::cell_fields`): tagged, at
    /// `cell + 8 + 8 * slot`. It ends the read.
    LoadField { slot: u16, rep: Representation },
    /// The tagged value at `slot` of the live class cell at granule `holder`, the holder of a class-keyed held
    /// entry (`entry::class_held_value`). The class guard before it fixes the holder's identity; a departed holder
    /// resets the field caches before any read (`Vm::drop_departed_holder_entries`), so it tests none. It ends the read.
    LoadHeldField { holder: u32, slot: u16 },
    /// The read of the one object whose record is at `record` as `value`, while `watch` holds
    /// (`own_constant_read`): an own constant, as an enum member or a namespace export. A plan
    /// that starts from the receiver's word compares the word with the object's own instead.
    OwnConstant { record: u64, watch: u64, value: u64 },
    /// A direct array's `length` from the receiver's word, its record among the array records
    /// at `records` (`elements::array_length`); any other receiver or form misses.
    ArrayLength { records: u64 },
    /// A flat string's `length` from the receiver's word, its record among the string records
    /// at `records` (`string_length::string_length`); any other receiver or form misses.
    StringLength { records: u64 },
    /// The read moves to the record's prototype, an object record, else misses.
    Prototype,
    /// The value at `slot` of the record, whose guarded shapes keep at least `storage_len`
    /// values in `form`, an arena word of which encodes its value in `rep` (`read_slot`); it
    /// ends the read.
    LoadSlot { slot: u16, form: Form, storage_len: usize, rep: Representation },
    /// The value at `slot` of a prototype record, which keeps its values in the arena past its
    /// extras (`Object::extras_storage_start`), else the read misses; it ends the read.
    LoadPrototypeSlot(u16),
    /// The record's prototype link selects its value from these (link, value bits) pairs, else
    /// the read misses; a dependency holds each prototype's value. It ends the read.
    PrototypeConstants(&'a [(u32, u64)]),
    /// An ahead image's prototype method read, by its holder guard key: the value the holder
    /// held when the image linked, for a receiver whose prototype link is the holder's, both
    /// words the loader resolves (`stage::ImageWord`). The fact is proven at link and retired by
    /// the holder's prototype-value watch, so the read loads neither the holder nor its slot.
    HolderConstant(u32),
    /// The value a prototype hint recorded (`holder::holder_read`): the record's layout is
    /// `guard` (its exact shape, storage form and prototype link at once) and the watch at
    /// `watch` on the prototype's values still holds, else the read misses. It ends the read.
    Held { guard: u64, watch: u64, value: u64 },
    /// The value `String.prototype` answered for a string receiver's read (`string_read`): the
    /// reached word is a direct string, whose one prototype stands for the shape and link
    /// guards, and the watch at `watch` on the prototype's values still holds, else the read
    /// misses. The read starts from the receiver's word, not an object record. It ends the read.
    HeldString { watch: u64, value: u64 },
    /// The value `Number.prototype` answered for a number primitive's read (`number_read`): the
    /// reached word is a Number (an int32 or a double), whose one prototype stands for the shape
    /// and link guards, and the watch at `watch` on the prototype's values still holds, else the
    /// read misses. The read starts from the receiver's word, not an object record. It ends the read.
    HeldNumber { watch: u64, value: u64 },
    /// The own field at `slot` of the record, which a getter's hint returned for a receiver of
    /// layout `guard` (`getter_own_read`): the record's layout is `guard` and the watch at `watch`
    /// on its prototype's values still holds, else the read misses. It ends the read.
    GetterOwn { guard: u64, watch: u64, slot: u16 },
}

/// The record a read starts from, with the facts its caller already holds for it: its
/// shape word and inline flag, which the read's first guards and load reuse instead of
/// loading again. A prototype the read reaches starts with none.
#[derive(Clone, Copy)]
pub struct Reached<W, C> {
    /// The object part the read starts from: a record's address, or for a class-fixed object the class cell
    /// (`object::cell_of`), which the class ops (`GuardClass`, `LoadField`) read.
    pub record: W,
    pub shape: Option<W>,
    pub inline: Option<C>,
    /// The record's layout id, where the caller loaded it.
    pub word: Option<W>,
    /// The storage-form bits a `GuardLayout` proved: a static input of the slot's layout.
    pub formed: Option<u32>,
    /// The record's auxiliary word (exact shape and storage-form bits), where the caller loaded it.
    pub auxiliary: Option<W>,
    /// The proof of the record's exact shape, once a guard of the read (or the caller's) ran: a
    /// slot read (`slot_layout`) takes it, so no slot is read past no guard.
    pub shaped: Option<super::Shaped<W>>,
}

impl<W, C> Reached<W, C> {
    /// A record the read reached with no facts of it held.
    #[inline(always)]
    pub fn bare(record: W) -> Self {
        Self { record, shape: None, inline: None, word: None, formed: None, auxiliary: None, shaped: None }
    }
}

/// The value `ops` read from `start`, prototypes found in the record table at `records`, or
/// `miss` where a guard fails. `None` where the stage leaves at `miss`, or the list ends
/// before a load.
#[inline(always)]
pub fn run<S: Stage>(stage: &mut S, ops: &[IcOp<'_>], mut at: Reached<S::Word, S::Cond>, records: Option<S::Word>,
    miss: S::Miss) -> Option<S::Word> {
    for op in ops {
        if let core::ops::ControlFlow::Break(read) = run_op(stage, op, &mut at, records, miss) { return read; }
    }
    None
}

/// `run` with its first ops written out one by one, for the read after a receiver's shape guard
/// (`property::run_owned`), whose list a stencil row holds as a constant of a few ops. Each step
/// folds to its op's arm, so the row makes no call and counts its holes at specialization; a loop
/// there kept every arm and counted holes at run time. Only that site unrolls: inlined at every
/// `run`, the copies tripled the compile time of a heap shard.
#[inline(always)]
pub fn run_unrolled<S: Stage>(stage: &mut S, ops: &[IcOp<'_>], mut at: Reached<S::Word, S::Cond>, records: Option<S::Word>,
    miss: S::Miss) -> Option<S::Word> {
    // The first ops one by one, then any rest in a loop: a stencil row's list is a constant of at most
    // `UNROLLED` ops, so each step folds to its op's arm and the row makes no call (a loop over the list
    // would keep every arm, and the holes each arm takes would be counted at run time).
    const UNROLLED: usize = 4;
    macro_rules! step {
        ($index:expr) => {
            if let Some(op) = ops.get($index) {
                if let core::ops::ControlFlow::Break(read) = run_op(stage, op, &mut at, records, miss) { return read; }
            } else {
                return None;
            }
        };
    }
    step!(0); step!(1); step!(2); step!(3);
    for op in ops.get(UNROLLED..).unwrap_or(&[]) {
        if let core::ops::ControlFlow::Break(read) = run_op(stage, op, &mut at, records, miss) { return read; }
    }
    None
}

/// One op of `run` over the record `at` reached: `Continue` to the next op, or `Break` with the read
/// (`None` where a guard left at `miss`).
#[inline(always)]
fn run_op<S: Stage>(stage: &mut S, op: &IcOp<'_>, at: &mut Reached<S::Word, S::Cond>, records: Option<S::Word>,
    miss: S::Miss) -> core::ops::ControlFlow<Option<S::Word>> {
    use core::ops::ControlFlow::{Break, Continue};
    macro_rules! guarded { ($e:expr) => { match $e { Some(held) => held, None => return Break(None) } }; }
    match *op {
        IcOp::GuardShape(shape) => at.shaped = Some(guarded!(guard_shape(stage, at, shape, miss))),
        IcOp::GuardLayout(id) => {
            at.shaped = Some(guarded!(guard_layout(stage, at, id, miss)));
            at.formed = Some(layout_form(id));
        }
        IcOp::GuardShapeFormed(shape) => {
            at.shaped = Some(guarded!(guard_shape_formed(stage, at, shape, miss)));
            at.formed = Some(0);
        }
        IcOp::GuardClass(class) => at.shaped = Some(guarded!(guard_class(stage, at, class, miss))),
        IcOp::GuardImageClass(key) => at.shaped = Some(guarded!(guard_image_class(stage, at, key, miss))),
        IcOp::LoadField { slot, rep } => return Break(Some(load_field(stage, at, (slot, rep)))),
        IcOp::LoadHeldField { holder, slot } => return Break(Some(load_held_field(stage, holder, slot))),
        IcOp::GuardFormed { id, form } => {
            at.shaped = Some(guarded!(guard_layout(stage, at, id, miss)));
            debug_assert_eq!(layout_form(id), form, "a fixed form's layout names it");
            at.formed = Some(form);
        }
        IcOp::Prototype => {
            let link = proto_link(stage, at.record);
            let Some(id) = proto_object_record_id(stage, link, miss) else { return Break(None) };
            let records = records.expect("a read through a prototype has the record table");
            *at = Reached::bare(object_record_at(stage, records, id));
        }
        IcOp::LoadSlot { slot, form, storage_len, rep } => return Break(load_slot(stage, at, (slot, rep), form, storage_len, miss)),
        IcOp::LoadPrototypeSlot(slot) => return Break(prototype_slot(stage, at.shaped.expect("a prototype slot read follows the guard of the holder's shape"), slot, miss)),
        IcOp::PrototypeConstants(values) => return Break(prototype_constants(stage, at.shaped.expect("a prototype's constants follow the guard of the receiver's shape"), values, miss)),
        IcOp::HolderConstant(key) => return Break(holder_constant(stage, at.shaped.expect("a holder's constant follows the guard of the receiver's shape"), key, miss)),
        IcOp::Held { guard, watch, value } => return Break(holder_read(stage, at.record, guard, watch, value, miss)),
        IcOp::OwnConstant { record, watch, value } => return Break(own_constant_read(stage, at.record, record, watch, value, miss)),
        IcOp::ArrayLength { records } => {
            let records = site_word(stage, records);
            return Break(super::elements::array_length(stage, records, at.record, miss));
        }
        IcOp::StringLength { records } => {
            let records = site_word(stage, records);
            return Break(super::string_length::string_length(stage, records, at.record, miss));
        }
        IcOp::HeldString { watch, value } => return Break(string_read(stage, at.record, watch, value, miss)),
        IcOp::HeldNumber { watch, value } => return Break(number_read(stage, at.record, watch, value, miss)),
        IcOp::GetterOwn { guard, watch, slot } => return Break(getter_own_read(stage, at.record, guard, watch, slot, miss)),
    }
    Continue(())
}

/// The own field of the object record at `record` where its exact shape is `shape`: the interpreter's
/// `GetField` hit (`concrete_own_field_hit`) and a stencil's inline-cache read (`quench-bjit/stencils/ic_rows.rs`)
/// alike. The auxiliary word proves the shape and, by its storage-form bits, where the values live, so a flat
/// run is one test away; `None`, leaving at `miss`, for another shape.
#[inline(always)]
pub fn own_field<S: Stage>(stage: &mut S, record: S::Word, shape: u32, slot: u16, miss: S::Miss) -> Option<S::Word> {
    let word = super::object::header_u32(stage, Region::ShapeWord.mutable(), record, OBJECT.auxiliary_offset);
    let expected = stage.shape_site(shape);
    // The word is the shape with no storage-form bit: that shape, its values a wide flat run, on one compare.
    let exact = stage.eq_imm(word, expected);
    stage.choose_leaving(exact, #[inline(always)] |stage| {
        let payload = stage.load_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32);
        let run = super::object::arena_run(stage, payload);
        let layout = super::object::Layout { slots: super::object::Slots::Flat(run), storage_len: usize::from(slot) + 1 };
        Some(read_slot(stage, layout, slot, Representation::Tagged))
    }, |stage| own_field_miss(stage, record, word, expected, slot, miss))
}

/// The own field of a record whose shape is `expected` but whose storage form is not the exact-shape word's, read through its formed layout.
#[cfg_attr(not(stencil_build), cold, inline(never))]
#[cfg_attr(stencil_build, inline(always))]
fn own_field_miss<S: Stage>(stage: &mut S, record: S::Word, word: S::Word, expected: S::Imm, slot: u16, miss: S::Miss) -> Option<S::Word> {
    let recorded = recorded_shape(stage, word);
    let same = stage.eq_imm(recorded, expected);
    stage.guard(same, miss)?;
    let layout = formed_own_layout(stage, record, word, Form::Either, usize::from(slot) + 1, PackedAccess::Reload, miss)?;
    Some(read_slot(stage, layout, slot, Representation::Tagged))
}

/// `concrete_hinted_own_field_hit`'s form: the one the hint's bits name at run time.
pub const HINTED_FORM: u32 = u32::MAX;

/// The tag of a shape id that names an ahead image's shape recipe rather than a shape of this
/// heap (`Stage::shape_site`): only an image's lowering makes one (`ahead_images::fields`), and the
/// shape table stops short of it (`vm::push_child`).
pub const SYMBOLIC_SHAPE: u32 = 1 << 30;

#[cfg(not(stencil_build))]
mod glue;
#[cfg(not(stencil_build))]
pub use glue::*;

#[cfg(test)]
mod tests;
