//! Inline caches as data (CacheIR): a property read a site proved, as a short list of ops
//! that every tier runs from this one definition, the interpreter through `Concrete` and the
//! native tiers through `Emit`. A tier records a site's state as its op list instead of
//! writing the read again.

use super::Stage;
use super::region::Region;
use super::object::{Form, Layout, PackedAccess, arena_run, extras_pointer, formed_own_layout, has_extras, inline_storage, object_record_at, own_layout,
    recorded_shape,
    proto_link, proto_object_record_id, read_slot, read_slot_value};
use crate::heap::NATIVE_OBJECT_LAYOUT as OBJECT;
use crate::typed_ir::Representation;

/// One step of a proved property read, over the record the read has reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IcOp<'a> {
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
pub(crate) struct Reached<W, C> {
    pub(crate) record: W,
    pub(crate) shape: Option<W>,
    pub(crate) inline: Option<C>,
    /// The record's layout id, where the caller loaded it.
    pub(crate) word: Option<W>,
    /// The storage-form bits a `GuardLayout` proved: a static input of the slot's layout.
    pub(crate) formed: Option<u32>,
    /// The record's auxiliary word (exact shape and storage-form bits), where the caller loaded it.
    pub(crate) auxiliary: Option<W>,
}

impl<W, C> Reached<W, C> {
    /// A record the read reached with no facts of it held.
    #[inline(always)]
    pub(crate) fn bare(record: W) -> Self {
        Self { record, shape: None, inline: None, word: None, formed: None, auxiliary: None }
    }
}

/// The value `ops` read from `start`, prototypes found in the record table at `records`, or
/// `miss` where a guard fails. `None` where the stage leaves at `miss`, or the list ends
/// before a load.
#[inline(always)]
pub(crate) fn run<S: Stage>(stage: &mut S, ops: &[IcOp<'_>], mut at: Reached<S::Word, S::Cond>, records: Option<S::Word>,
    miss: S::Miss) -> Option<S::Word> {
    for op in ops {
        match *op {
            IcOp::GuardShape(shape) => guard_shape(stage, &at, shape, miss)?,
            IcOp::GuardLayout(id) => {
                guard_layout(stage, &at, id, miss)?;
                at.formed = Some(layout_form(id));
            }
            IcOp::GuardFormed { id, form } => {
                guard_layout(stage, &at, id, miss)?;
                debug_assert_eq!(layout_form(id), form, "a fixed form's layout names it");
                at.formed = Some(form);
            }
            IcOp::Prototype => {
                let link = proto_link(stage, at.record);
                let id = proto_object_record_id(stage, link, miss)?;
                // A read through a prototype has the record table; a plan without it leaves, with
                // no panic path, so a stencil's loop over the ops stays call-free and unrolls.
                debug_assert!(records.is_some(), "a read through a prototype has the record table");
                let records = records?;
                at = Reached::bare(object_record_at(stage, records, id));
            }
            IcOp::LoadSlot { slot, form, storage_len, rep } => return load_slot(stage, &at, (slot, rep), form, storage_len, miss),
            IcOp::LoadPrototypeSlot(slot) => return prototype_slot(stage, at.record, slot, miss),
            IcOp::PrototypeConstants(values) => return prototype_constants(stage, at.record, values, miss),
            IcOp::Held { guard, watch, value } => return holder_read(stage, at.record, guard, watch, value, miss),
            IcOp::OwnConstant { record, watch, value } => return own_constant_read(stage, at.record, record, watch, value, miss),
            IcOp::ArrayLength { records } => {
                let records = site_word(stage, records);
                return super::elements::array_length(stage, records, at.record, miss);
            }
            IcOp::StringLength { records } => {
                let records = site_word(stage, records);
                return super::string_length::string_length(stage, records, at.record, miss);
            }
            IcOp::HeldString { watch, value } => return string_read(stage, at.record, watch, value, miss),
            IcOp::HeldNumber { watch, value } => return number_read(stage, at.record, watch, value, miss),
            IcOp::GetterOwn { guard, watch, slot } => return getter_own_read(stage, at.record, guard, watch, slot, miss),
        }
    }
    None
}

/// The value the hint `(guard, watch, value)` records for the object part at `record`: `None`
/// (leaving at `miss`) where the record's layout (`Object::guard_word`: the interned pair of
/// its whole auxiliary word, the exact shape with its storage form, and its prototype link and
/// flags) differs, or the watch no longer holds. One compare of the layout id proves the shape
/// and the prototype together.
#[inline(always)]
pub(crate) fn holder_read<S: Stage>(stage: &mut S, record: S::Word, guard: u64, watch: u64, value: u64,
    miss: S::Miss) -> Option<S::Word> {
    let words = stage.load_u32(Region::ShapeWord.mutable(), record,
        crate::heap::NATIVE_OBJECT_LAYOUT.layout_offset as i32);
    let guard = stage.site(guard);
    let same = stage.eq_imm(words, guard);
    stage.guard(same, miss)?;
    let watch = stage.site(watch);
    let holds = stage.assumption(watch);
    stage.guard(holds, miss)?;
    let zero = stage.constant(0);
    let value = stage.site(value);
    Some(stage.add_imm(zero, value))
}

/// The own field the getter hint `(guard, watch, slot)` records for the object part at `record`:
/// `None` (leaving at `miss`) where the record's layout word differs from `guard` (`holder_read`'s
/// compare), or the watch on its prototype's values no longer holds. The field is then the tagged
/// slot `LoadSlot` reads, as a getter's own-field return is.
#[inline(always)]
pub(crate) fn getter_own_read<S: Stage>(stage: &mut S, record: S::Word, guard: u64, watch: u64, slot: u16,
    miss: S::Miss) -> Option<S::Word> {
    let words = stage.load_u32(Region::ShapeWord.mutable(), record,
        crate::heap::NATIVE_OBJECT_LAYOUT.layout_offset as i32);
    let guard = stage.site(guard);
    let same = stage.eq_imm(words, guard);
    stage.guard(same, miss)?;
    let watch = stage.site(watch);
    let holds = stage.assumption(watch);
    stage.guard(holds, miss)?;
    let at = Reached::bare(record);
    load_slot(stage, &at, (slot, Representation::Tagged), Form::Either, usize::from(slot) + 1, miss)
}

/// The value the own-constant hint `(record, watch, value)` records for the object part at
/// `at`: `None` (leaving at `miss`) for any record but the hinted one, or once the watch on its
/// values no longer holds. A receiver that is always one object, whose slot the code reads and
/// nothing rewrites (an enum, a namespace, a module's exports, a configuration object), reads
/// as its value: one compare of the record's address and the watch's flag. Two objects of one
/// layout (two enums with the same members) share a layout but never a record.
#[inline(always)]
pub(crate) fn own_constant_read<S: Stage>(stage: &mut S, at: S::Word, record: u64, watch: u64, value: u64,
    miss: S::Miss) -> Option<S::Word> {
    let record = stage.site(record);
    let same = stage.eq_imm(at, record);
    stage.guard(same, miss)?;
    let watch = stage.site(watch);
    let holds = stage.assumption(watch);
    stage.guard(holds, miss)?;
    let zero = stage.constant(0);
    let value = stage.site(value);
    Some(stage.add_imm(zero, value))
}

/// The per-site operand `bits` as a word (a record table's base the row names).
#[inline(always)]
fn site_word<S: Stage>(stage: &mut S, bits: u64) -> S::Word {
    let zero = stage.constant(0);
    let bits = stage.site(bits);
    stage.add_imm(zero, bits)
}

/// The value the string hint `(watch, value)` records for the receiver word `word`: `None`
/// (leaving at `miss`) for any receiver but a direct string, or once the watch on
/// `String.prototype`'s values no longer holds.
#[inline(always)]
pub(crate) fn string_read<S: Stage>(stage: &mut S, word: S::Word, watch: u64, value: u64, miss: S::Miss) -> Option<S::Word> {
    let string = super::value::is_direct_string(stage, word);
    stage.guard(string, miss)?;
    let watch = stage.site(watch);
    let holds = stage.assumption(watch);
    stage.guard(holds, miss)?;
    let zero = stage.constant(0);
    let value = stage.site(value);
    Some(stage.add_imm(zero, value))
}

/// The value the number hint `(watch, value)` records for the receiver word `word`: `None`
/// (leaving at `miss`) for any receiver but a Number, an int32 or a binary64, or once the watch
/// on `Number.prototype`'s values no longer holds.
#[inline(always)]
pub(crate) fn number_read<S: Stage>(stage: &mut S, word: S::Word, watch: u64, value: u64, miss: S::Miss) -> Option<S::Word> {
    let integer = super::value::is_int(stage, word);
    let double = super::value::is_double(stage, word);
    let number = stage.any(integer, double);
    stage.guard(number, miss)?;
    let watch = stage.site(watch);
    let holds = stage.assumption(watch);
    stage.guard(holds, miss)?;
    let zero = stage.constant(0);
    let value = stage.site(value);
    Some(stage.add_imm(zero, value))
}

/// `IcOp::GuardShape`: continues where the reached record's exact shape is `shape`.
#[inline(always)]
pub(super) fn guard_shape<S: Stage>(stage: &mut S, at: &Reached<S::Word, S::Cond>, shape: u32, miss: S::Miss) -> Option<()> {
    let exact = shape_word(stage, at);
    let same = { let shape = stage.site(u64::from(shape)); stage.eq_imm(exact, shape) };
    stage.guard(same, miss)
}

/// `IcOp::GuardLayout`: continues where the reached record's layout id is `id`.
#[inline(always)]
pub(super) fn guard_layout<S: Stage>(stage: &mut S, at: &Reached<S::Word, S::Cond>, id: u32, miss: S::Miss) -> Option<()> {
    let loaded = at.word.unwrap_or_else(|| layout_id(stage, at.record));
    let same = { let id = stage.site(u64::from(id)); stage.eq_imm(loaded, id) };
    stage.guard(same, miss)
}

/// The record's layout id (`Object::layout_id`), the one header word it keeps.
#[inline(always)]
pub(crate) fn layout_id<S: Stage>(stage: &mut S, record: S::Word) -> S::Word {
    stage.load_u32(Region::ShapeWord.mutable(), record, OBJECT.layout_offset as i32)
}

/// The storage-form bits of layout `id`, read from its interned entry: a fact of the id, so a
/// specializer folds it at compile time. `id` is one a record's layout matched (an interned
/// id, its entry readable with no bound test).
#[inline(always)]
pub(crate) fn layout_form(id: u32) -> u32 {
    crate::heap::layouts::word(id) as u32 & FORM_BITS
}

/// The exact shape of layout `id` (`Object::exact_shape`), any word (`layouts::auxiliary`).
#[inline(always)]
pub(crate) fn layout_shape(id: u32) -> u32 {
    crate::heap::layouts::auxiliary(id) & !FORM_BITS
}

/// Every storage-form bit of an auxiliary word.
pub(crate) const FORM_BITS: u32 = OBJECT.inline_form | OBJECT.extras_form | OBJECT.narrow_form;

/// The `Form` layout `id` names: a fact of the id (its auxiliary word's form bits), so a caller
/// holding the id at compile time passes it and the stage emits no run-time form test. Plain
/// inline values are `Packed`, a plain arena run is `Arena`. A narrow run and values with extras
/// have no `Form` of their own: a formed read takes them from the id (`proved_own_layout`), and
/// the fallback reads them as `Either`. An id past the table names no layout and reads as `Arena`,
/// which no guard of a cleared hint admits. Reads the auxiliary word, which bounds its id.
#[inline(always)]
pub(crate) fn form_of_layout(id: u32) -> Form {
    match crate::heap::layouts::auxiliary(id) & FORM_BITS {
        0 => Form::Arena,
        bits if bits == OBJECT.inline_form => Form::Packed,
        _ => Form::Either,
    }
}

/// `IcOp::LoadSlot`: the value at `slot` of the reached record.
#[inline(always)]
fn load_slot<S: Stage>(stage: &mut S, at: &Reached<S::Word, S::Cond>, (slot, rep): (u16, Representation), form: Form, storage_len: usize,
    miss: S::Miss) -> Option<S::Word> {
    let layout = slot_layout(stage, at, form, storage_len, miss)?;
    Some(read_slot_value(stage, layout, slot, rep))
}

/// The layout `IcOp::LoadSlot` reads through: the reached record's, whose shape is guarded
/// and keeps `storage_len` values in `form`. A caller that proved it before reads through it
/// again instead of proving it twice (`property::kept_slot`).
#[inline(always)]
pub(super) fn slot_layout<S: Stage>(stage: &mut S, at: &Reached<S::Word, S::Cond>, form: Form, storage_len: usize,
    miss: S::Miss) -> Option<Layout<S::Word, S::Cond>> {
    if let Some(layout) = at.formed.and_then(|bits| super::object::proved_own_layout(stage, at.record, bits, storage_len)) {
        return Some(layout);
    }
    let inline = at.inline.unwrap_or_else(|| inline_storage(stage, at.record));
    own_layout(stage, at.record, inline, form, storage_len, PackedAccess::Reload, miss)
}

/// The reached record's exact shape word: the caller's where it holds one, else loaded.
#[inline(always)]
pub(super) fn shape_word<S: Stage>(stage: &mut S, at: &Reached<S::Word, S::Cond>) -> S::Word {
    at.shape.unwrap_or_else(|| super::object::exact_shape(stage, at.record))
}

/// `IcOp::PrototypeConstants`: the value `values` pair with the link of `record`'s prototype.
#[inline(always)]
fn prototype_constants<S: Stage>(stage: &mut S, record: S::Word, values: &[(u32, u64)], miss: S::Miss) -> Option<S::Word> {
    let link = proto_link(stage, record);
    let ((first, bits), rest) = values.split_first()?;
    let mut matched = stage.eq_imm(link, u64::from(*first));
    let mut read = stage.constant(*bits);
    for (prototype, bits) in rest {
        let same = stage.eq_imm(link, u64::from(*prototype));
        let candidate = stage.constant(*bits);
        read = stage.select(same, candidate, read);
        matched = stage.any(matched, same);
    }
    stage.guard(matched, miss)?;
    Some(read)
}

/// `IcOp::LoadPrototypeSlot` of the record at `record`.
#[inline(always)]
fn prototype_slot<S: Stage>(stage: &mut S, record: S::Word, slot: u16, miss: S::Miss) -> Option<S::Word> {
    let inline = inline_storage(stage, record);
    let in_arena = stage.not(inline);
    stage.guard(in_arena, miss)?;
    let payload = stage.load_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32);
    let extras = has_extras(stage, payload);
    stage.guard(extras, miss)?;
    let pointer = extras_pointer(stage, payload);
    let present = stage.ne_imm(pointer, 0u32);
    stage.guard(present, miss)?;
    let start = stage.load_u32(Region::Payload.mutable(), pointer, OBJECT.extras_arena_start_offset as i32);
    let run = arena_run(stage, start);
    { let offset = stage.site(u64::from(slot) * OBJECT.property_value_size as u64); Some(stage.load_site(Region::ObjectSlots.mutable(), run, offset)) }
}

/// The own field a site's hint proved: the value at `slot` of `object` where its exact shape
/// is `shape`, else `None` (the interpreter's `GetField` hit: `GuardShape` then `LoadSlot`, the
/// ops `run` interprets, through `Concrete`), through the arena whose values start at `property_values`.
///
/// # Safety
/// `object` is a live record, and `property_values` is the base of the arena that holds its
/// run; a hint's slot is one its shape covers.
#[inline(always)]
pub(crate) unsafe fn concrete_own_field_hit(object: &crate::heap::Object, property_values: *const crate::Value,
    shape: u32, slot: u16) -> Option<crate::Value> {
    let mut stage = super::Concrete::new(property_values);
    let record = object as *const crate::heap::Object as u64;
    own_field(&mut stage, record, shape, slot, ()).map(crate::value_vec::decode_staged_word)
}

/// The own field of the object record at `record` where its exact shape is `shape`: the interpreter's
/// `GetField` hit (`concrete_own_field_hit`) and a stencil's inline-cache read (`quench-bjit/stencils/ic_rows.rs`)
/// alike. The auxiliary word proves the shape and, by its storage-form bits, where the values live, so a flat
/// run is one test away; `None`, leaving at `miss`, for another shape.
#[inline(always)]
pub(crate) fn own_field<S: Stage>(stage: &mut S, record: S::Word, shape: u32, slot: u16, miss: S::Miss) -> Option<S::Word> {
    let word = super::object::header_u32(stage, Region::ShapeWord.mutable(), record, OBJECT.auxiliary_offset);
    let expected = stage.site(u64::from(shape));
    // The word is the shape with no storage-form bit: that shape, its values a wide flat run, on one compare.
    let exact = stage.eq_imm(word, expected);
    stage.choose_leaving(exact, #[inline(always)] |stage| {
        let payload = stage.load_u64(Region::Payload.mutable(), record, OBJECT.payload_offset as i32);
        let run = super::object::arena_run(stage, payload);
        let layout = super::object::Layout { slots: super::object::Slots::Flat(run), storage_len: usize::from(slot) + 1 };
        Some(read_slot(stage, layout, slot, Representation::Tagged))
    }, #[inline(always)] |stage| {
        let recorded = recorded_shape(stage, word);
        let same = stage.eq_imm(recorded, expected);
        stage.guard(same, miss)?;
        let layout = formed_own_layout(stage, record, word, Form::Either, usize::from(slot) + 1, PackedAccess::Reload, miss)?;
        Some(read_slot(stage, layout, slot, Representation::Tagged))
    })
}

/// `concrete_hinted_own_field_hit`'s form: the one the hint's bits name at run time.
pub(crate) const HINTED_FORM: u32 = u32::MAX;

/// A fixed form's flag (`concrete_hinted_own_field_hit`): every slot word of the process is a
/// tagged `Value` (`switches::float_slots` off, a per-process decision), so the read decodes
/// nothing. The handler is chosen with the flag when its hint is installed, so the switch is
/// read once per installation, not per read. Below every storage-form bit.
pub(crate) const TAGGED_SLOTS: u32 = 1 << 26;
const _: () = assert!(TAGGED_SLOTS & (OBJECT.inline_form | OBJECT.extras_form | OBJECT.narrow_form) == 0);

/// `concrete_own_field_hit` through a lane record's own-field hint, whose `hint` is a layout
/// id (`Object::hint_shape`): one compare of the record's own word proves the exact shape and
/// the storage form, which the id's entry then tells.
///
/// A fixed `FORM` is the form of every hint a record of it holds: its handler is chosen from
/// the hint it is installed with (`own_formed_handler`), and a later hint either re-chooses it
/// or names no object's layout (a cleared hint), so equality proves `FORM` too.
///
/// # Safety
/// As for `concrete_own_field_hit`; for a fixed `FORM`, a hint some object's layout equals
/// names `FORM`.
#[inline(always)]
pub(crate) unsafe fn concrete_hinted_own_field_hit<const FORM: u32>(object: &crate::heap::Object,
    property_values: *const crate::Value, hint: u32, slot: u16) -> Option<crate::Value> {
    let mut stage = super::Concrete::new(property_values);
    let record = object as *const crate::heap::Object as u64;
    let word = layout_id(&mut stage, record);
    // A fixed form's hint names it (the caller's contract), so equality proves that form
    // and its layout is read without testing the bits; no narrow run's layout matches it.
    let tagged = FORM != HINTED_FORM && FORM & TAGGED_SLOTS != 0;
    let form = if FORM == HINTED_FORM { FORM } else { FORM & !TAGGED_SLOTS };
    // The same row a specializer runs (`IcOp::GuardLayout`, then the proved form's layout):
    // one compare proves shape and form, and the layout tests no form bit.
    let at = Reached { word: Some(word), ..Reached::bare(record) };
    guard_layout(&mut stage, &at, hint, ())?;
    debug_assert!(form == HINTED_FORM || layout_form(hint) == form, "a fixed form's matched hint names it");
    let bits = if form == HINTED_FORM { layout_form(hint) } else { form };
    let layout = slot_layout(&mut stage, &Reached { formed: Some(bits), ..at }, form_of_layout(hint), usize::from(slot) + 1, ())?;
    let word = read_slot(&mut stage, layout, slot, Representation::Tagged);
    // Packed and narrow slots hold narrow values, never a raw double (`packed_read`,
    // `narrow_read`), so a fixed one of those forms skips the float-slot decode.
    if tagged || form == OBJECT.inline_form || form == OBJECT.narrow_form {
        return Some(crate::Value::from_bits(word));
    }
    Some(crate::value_vec::decode_staged_word(word))
}

/// The own data value the field cache entry of the site `offset` bytes into `entries` (the
/// running program's table, `Vm::active_field_entries`) names in `object`, which the object
/// record table at `records` holds: the interpreter's primary read of a site, `site_own_read`
/// through `Concrete`, the own arm of the row the stencil build compiles from `site_read`. `None`
/// where it misses, or the entry names a prototype holder.
///
/// # Safety
/// `entries` is the table of the running program and `offset` a site's byte offset in it,
/// `records` the object record reservation and `property_values` the base of the arena that
/// holds the record's run; none outlives the call.
#[inline(always)]
pub(crate) unsafe fn concrete_site_own_read(property_values: *const crate::Value, (entries, records): (u64, u64), object: crate::Value, offset: u64) -> Option<crate::Value> {
    let mut stage = super::Concrete::new(property_values);
    super::entry::site_own_read(&mut stage, (entries, records), object.to_bits(), offset, ()).map(crate::value_vec::decode_staged_word)
}

/// `site_read` through `Concrete`: own and prototype-held entries alike.
///
/// # Safety
/// As `concrete_site_own_read`.
#[cfg(test)]
pub(crate) unsafe fn concrete_site_read(property_values: *const crate::Value, (entries, records): (u64, u64), object: crate::Value, offset: u64) -> Option<crate::Value> {
    let mut stage = super::Concrete::new(property_values);
    super::entry::site_read(&mut stage, (entries, records), object.to_bits(), offset, ()).map(crate::value_vec::decode_staged_word)
}

/// The value `object.first.second` reads through the field cache entries `first` and `second`
/// (byte offsets into `entries`): `site_own_chain` through `Concrete`, one proof of the tables for
/// both reads. `None` where either misses.
///
/// # Safety
/// As `concrete_site_own_read`, for both offsets.
#[inline(always)]
pub(crate) unsafe fn concrete_site_own_chain(property_values: *const crate::Value, tables: (u64, u64), object: crate::Value, offsets: (u64, u64)) -> Option<crate::Value> {
    let mut stage = super::Concrete::new(property_values);
    super::entry::site_own_chain(&mut stage, tables, object.to_bits(), offsets, ()).map(crate::value_vec::decode_staged_word)
}

/// The store of `value` into the own data field the site's entry `offset` bytes into `entries`
/// names in `object`: `site_store` through `Concrete` over `heap`, which admits it where the
/// write barrier needs no log. False changes nothing.
///
/// # Safety
/// As `concrete_site_read`, with `heap` the heap whose tables `entries` and `records` are.
#[inline(always)]
#[cfg(not(stencil_build))]
pub(crate) unsafe fn concrete_site_store(heap: &crate::heap::Heap, (entries, records): (u64, u64), object: crate::Value, offset: u64, value: crate::Value) -> bool {
    let mut stage = super::Concrete::storing(heap);
    super::entry::site_store(&mut stage, (entries, records), object.to_bits(), offset, (value.to_bits(), true, None), ()).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::{Heap, Object};
    use crate::staged::Concrete;
    use crate::Value;

    /// The interpreter's reading of `run`: `None` for a miss.
    fn concrete(heap: &Heap, ops: &[IcOp], object: Value) -> Option<Value> {
        let mut stage = Concrete::new(heap.property_values_base());
        let record = heap.object(object)? as *const Object as u64;
        let records = heap.object_record_base()? as u64;
        run(&mut stage, ops, Reached::bare(record), Some(records), ()).map(Value::from_bits)
    }

    /// A prototype's read guards the receiver's and the prototype's shapes; a receiver of
    /// another shape, a prototype of another, or a record with no object prototype misses.
    #[test]
    fn an_op_list_reads_a_prototype_slot_and_misses_past_its_guards() {
        let mut heap = Heap::new();
        for shape in 1..=2_u32 {
            heap.register_property_shape(shape, shape as usize, None);
        }
        let prototype = heap.alloc_object(Object::new(Value::NULL));
        heap.property_add(prototype, Value::number(1.5), 1);
        heap.object_record_mut(prototype).unwrap().mark_as_prototype();
        let object = heap.alloc_object(Object::new(prototype));
        heap.property_add(object, Value::number(7.0), 1);
        heap.property_add(object, Value::number(8.0), 2);
        let (own, holder) = (heap.object(object).unwrap().exact_shape(), heap.object(prototype).unwrap().exact_shape());
        let read = |receiver, holder| [IcOp::GuardShape(receiver), IcOp::Prototype, IcOp::GuardShape(holder), IcOp::LoadPrototypeSlot(0)];
        assert_eq!(concrete(&heap, &read(own, holder), object), Some(Value::number(1.5)));
        let load = IcOp::LoadSlot { slot: 1, form: Form::Either, storage_len: 2, rep: Representation::Tagged };
        assert_eq!(concrete(&heap, &[IcOp::GuardShape(own), load], object), Some(Value::number(8.0)));
        assert_eq!(concrete(&heap, &[IcOp::GuardShape(holder), load], object), None);
        assert_eq!(concrete(&heap, &read(holder, holder), object), None);
        assert_eq!(concrete(&heap, &read(own, own), object), None);
        assert_eq!(concrete(&heap, &read(holder, holder), prototype), None);
    }

    /// A prototype-selected constant answers for the link of the receiver's prototype and
    /// misses for any other.
    #[test]
    fn a_prototype_constant_is_selected_by_the_receivers_link() {
        let mut heap = Heap::new();
        let prototype = heap.alloc_object(Object::new(Value::NULL));
        heap.object_record_mut(prototype).unwrap().mark_as_prototype();
        let object = heap.alloc_object(Object::new(prototype));
        let link = heap.object(object).unwrap().proto_link().bits();
        let (one, two) = (Value::number(1.5).to_bits(), Value::number(2.5).to_bits());
        assert_eq!(concrete(&heap, &[IcOp::PrototypeConstants(&[(link ^ 1, one), (link, two)])], object), Some(Value::number(2.5)));
        assert_eq!(concrete(&heap, &[IcOp::PrototypeConstants(&[(link ^ 1, one)])], object), None);
    }

    /// The interpreter keeps its typed prototype read (`Heap::holder_property_get_flat`),
    /// which routing through `run` made slower (deltablue +1.6%); it reads what the staged
    /// `LoadPrototypeSlot` reads at every slot of a prototype, of narrow and wide layouts.
    #[test]
    fn the_interpreters_prototype_read_agrees_with_the_staged_one() {
        let mut heap = Heap::new();
        let wide = crate::value_vec::INLINE_PROPERTY_COUNT + 2;
        for shape in 1..=wide as u32 {
            heap.register_property_shape(shape, shape as usize, None);
        }
        for len in [1, wide] {
            let prototype = heap.alloc_object(Object::new(Value::NULL));
            for slot in 0..len {
                heap.property_add(prototype, Value::number(slot as f64 + 0.5), slot as u32 + 1);
            }
            heap.object_record_mut(prototype).unwrap().mark_as_prototype();
            let object = heap.alloc_object(Object::new(prototype));
            let holder = heap.object(prototype).unwrap();
            for slot in 0..len {
                // SAFETY: the prototype's exact shape holds `slot` outside dictionary storage.
                let interpreter = unsafe { heap.holder_property_get_flat(holder, slot) };
                assert_eq!(concrete(&heap, &[IcOp::Prototype, IcOp::LoadPrototypeSlot(slot as u16)], object), Some(interpreter),
                    "slot {slot} of {len}");
            }
        }
    }

    /// A row calls `site` in an order its operand values never change: the same ops over two
    /// sites give one trace and differ only in what the holes took.
    #[test]
    fn site_values_change_holes_not_the_trace() {
        let read = |shape: u32, slot: u16| {
            let mut trace = super::super::trace::Trace::default();
            let at = Reached::bare(0);
            let record = trace.constant(0);
            let at = Reached { record, ..at };
            let ops = [IcOp::GuardShape(shape), IcOp::LoadSlot { slot, form: Form::Either, storage_len: usize::from(slot) + 1, rep: Representation::Tagged }];
            run(&mut trace, &ops, at, None, ()).expect("a load ends the read");
            trace
        };
        let (first, second) = (read(5, 1), read(6, 0));
        assert_eq!(first.ops, second.ops, "the trace never depends on a site value");
        // Either form reads through one operand, the flat byte offset (`object::site_offset`).
        assert_eq!(first.holes, [5, 8]);
        assert_eq!(second.holes, [6, 0]);
    }
}
