//! The property-read primitives an inline-cache op list runs over the reached record: the holder and constant reads a
//! prototype answers take, the layout guards and their slot reads, and the decoding of a layout id. Each is generic over
//! `Stage`, so the interpreter (`Concrete`) and the native tiers (`Emit`, the stencils) run the one definition.

use super::Stage;
use super::ic::Reached;
use super::Shaped;
use super::region::Region;
use super::object::{Form, Layout, PackedAccess, arena_run, cell_fields, extras_pointer, has_extras, inline_storage, own_layout, proto_link, read_slot_value};
use crate::heap::NATIVE_OBJECT_LAYOUT as OBJECT;
use crate::typed_ir::Representation;

/// The value the hint `(guard, watch, value)` records for the object part at `record`: `None`
/// (leaving at `miss`) where the record's layout (`Object::guard_word`: the interned pair of
/// its whole auxiliary word, the exact shape with its storage form, and its prototype link and
/// flags) differs, or the watch no longer holds. One compare of the layout id proves the shape
/// and the prototype together.
#[inline(always)]
pub fn holder_read<S: Stage>(stage: &mut S, record: S::Word, guard: u64, watch: u64, value: u64,
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
pub fn getter_own_read<S: Stage>(stage: &mut S, record: S::Word, guard: u64, watch: u64, slot: u16,
    miss: S::Miss) -> Option<S::Word> {
    let words = stage.load_u32(Region::ShapeWord.mutable(), record,
        crate::heap::NATIVE_OBJECT_LAYOUT.layout_offset as i32);
    let guard = stage.site(guard);
    let same = stage.eq_imm(words, guard);
    stage.guard(same, miss)?;
    let watch = stage.site(watch);
    let holds = stage.assumption(watch);
    stage.guard(holds, miss)?;
    let at = Reached { shaped: Some(Shaped::of_guard(record)), ..Reached::bare(record) };
    load_slot(stage, &at, (slot, Representation::Tagged), Form::Either, usize::from(slot) + 1, miss)
}

/// The value the own-constant hint `(record, watch, value)` records for the object part at
/// `at`: `None` (leaving at `miss`) for any record but the hinted one, or once the watch on its
/// values no longer holds. A receiver that is always one object, whose slot the code reads and
/// nothing rewrites (an enum, a namespace, a module's exports, a configuration object), reads
/// as its value: one compare of the record's address and the watch's flag. Two objects of one
/// layout (two enums with the same members) share a layout but never a record.
#[inline(always)]
pub fn own_constant_read<S: Stage>(stage: &mut S, at: S::Word, record: u64, watch: u64, value: u64,
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
pub fn site_word<S: Stage>(stage: &mut S, bits: u64) -> S::Word {
    let zero = stage.constant(0);
    let bits = stage.site(bits);
    stage.add_imm(zero, bits)
}

/// The value the string hint `(watch, value)` records for the receiver word `word`: `None`
/// (leaving at `miss`) for any receiver but a direct string, or once the watch on
/// `String.prototype`'s values no longer holds.
#[inline(always)]
pub fn string_read<S: Stage>(stage: &mut S, word: S::Word, watch: u64, value: u64, miss: S::Miss) -> Option<S::Word> {
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
pub fn number_read<S: Stage>(stage: &mut S, word: S::Word, watch: u64, value: u64, miss: S::Miss) -> Option<S::Word> {
    let (double, integer) = super::value::number_classes(stage, word);
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
pub fn guard_shape<S: Stage>(stage: &mut S, at: &Reached<S::Word, S::Cond>, shape: u32, miss: S::Miss) -> Option<Shaped<S::Word>> {
    let exact = shape_word(stage, at);
    let same = { let shape = stage.shape_site(shape); stage.eq_imm(exact, shape) };
    stage.guard(same, miss)?;
    Some(Shaped::of_guard(at.record))
}

/// `IcOp::GuardShapeFormed`: continues where the reached record's whole auxiliary word is `shape` with no
/// storage-form bit, the plain arena run (`object::proved_own_layout` of form bits 0).
#[inline(always)]
pub fn guard_shape_formed<S: Stage>(stage: &mut S, at: &Reached<S::Word, S::Cond>, shape: u32, miss: S::Miss) -> Option<Shaped<S::Word>> {
    let word = at.auxiliary.unwrap_or_else(#[inline(always)] || super::object::aux_word(stage, at.record));
    let same = { let shape = stage.shape_site(shape); stage.eq_imm(word, shape) };
    stage.guard(same, miss)?;
    Some(Shaped::of_guard(at.record))
}

/// `IcOp::GuardLayout`: continues where the reached record's layout id is `id`.
#[inline(always)]
pub fn guard_layout<S: Stage>(stage: &mut S, at: &Reached<S::Word, S::Cond>, id: u32, miss: S::Miss) -> Option<Shaped<S::Word>> {
    let loaded = at.word.unwrap_or_else(#[inline(always)] || layout_id(stage, at.record));
    let same = { let id = stage.site(u64::from(id)); stage.eq_imm(loaded, id) };
    stage.guard(same, miss)?;
    Some(Shaped::of_guard(at.record))
}

/// `IcOp::GuardClass`: continues where the reached class cell's class is `class`. The cell's header is the
/// one word read, and a cell whose class is not `class` leaves at `miss`.
#[inline(always)]
pub fn guard_class<S: Stage>(stage: &mut S, at: &Reached<S::Word, S::Cond>, class: u32, miss: S::Miss) -> Option<Shaped<S::Word>> {
    let loaded = super::object::cell_class(stage, at.record);
    let same = { let class = stage.site(u64::from(class)); stage.eq_imm(loaded, class) };
    stage.guard(same, miss)?;
    Some(Shaped::of_cell(at.record))
}

/// `IcOp::GuardImageClass`: continues where the reached cell's class is the one an ahead image's loader
/// resolves for field guard key `key` (`ImageWord::Layout`, the class id).
#[inline(always)]
pub fn guard_image_class<S: Stage>(stage: &mut S, at: &Reached<S::Word, S::Cond>, key: u32, miss: S::Miss) -> Option<Shaped<S::Word>> {
    let loaded = super::object::cell_class(stage, at.record);
    let same = { let class = stage.image_word(super::stage::ImageWord::Layout, key); stage.eq_imm(loaded, class) };
    stage.guard(same, miss)?;
    Some(Shaped::of_cell(at.record))
}

/// `IcOp::LoadField`: the tagged value at `slot` of the class cell the reached word names, which the class
/// guard before it proved holds the slot (`object::cell_fields`).
#[inline(always)]
pub fn load_field<S: Stage>(stage: &mut S, at: &Reached<S::Word, S::Cond>, (slot, rep): (u32, Representation)) -> S::Word {
    debug_assert!(at.shaped.is_some(), "a field load follows the guard of its class");
    debug_assert!(rep == Representation::Tagged, "a class cell's field is read tagged");
    // The class cell's own field, inline or spilled (`entry::cell_field`, `class_slot_address`).
    let slot = stage.constant(u64::from(slot));
    super::entry::cell_field(stage, at.record, slot)
}

/// `IcOp::LoadHeldField`: the tagged value at `slot` of the live class cell at granule `holder`, the holder
/// `entry::class_held_value` reads. No guard of its own: the class guard before it proved the receiver, and the
/// holder's fields moving resets the caches before any read.
#[inline(always)]
pub fn load_held_field<S: Stage>(stage: &mut S, holder: u32, slot: u32) -> S::Word {
    let granule = stage.constant(u64::from(holder));
    let cell = super::object::cell_at_granule(stage, granule);
    let slot = stage.constant(u64::from(slot));
    super::entry::cell_field(stage, cell, slot)
}

/// The record's layout id (`Object::layout_id`), the one header word it keeps.
#[inline(always)]
pub fn layout_id<S: Stage>(stage: &mut S, record: S::Word) -> S::Word {
    stage.load_u32(Region::ShapeWord.mutable(), record, OBJECT.layout_offset as i32)
}

/// The storage-form bits of layout `id`, read from its interned entry: a fact of the id, so a
/// specializer folds it at compile time. `id` is one a record's layout matched (an interned
/// id, its entry readable with no bound test).
#[inline(always)]
pub fn layout_form(id: u32) -> u32 {
    crate::heap::layouts::word(id) as u32 & FORM_BITS
}

/// The exact shape of layout `id` (`Object::exact_shape`), any word (`layouts::auxiliary`).
#[inline(always)]
pub fn layout_shape(id: u32) -> u32 {
    crate::heap::layouts::auxiliary(id) & !FORM_BITS
}

/// Every storage-form bit of an auxiliary word.
pub const FORM_BITS: u32 = OBJECT.inline_form | OBJECT.extras_form | OBJECT.narrow_form;

/// The `Form` layout `id` names: a fact of the id (its auxiliary word's form bits), so a caller
/// holding the id at compile time passes it and the stage emits no run-time form test. Plain
/// inline values are `Packed`, a plain arena run is `Arena`. A narrow run and values with extras
/// have no `Form` of their own: a formed read takes them from the id (`proved_own_layout`), and
/// the fallback reads them as `Either`. An id past the table names no layout and reads as `Arena`,
/// which no guard of a cleared hint admits. Reads the auxiliary word, which bounds its id.
#[inline(always)]
pub fn form_of_layout(id: u32) -> Form {
    match crate::heap::layouts::auxiliary(id) & FORM_BITS {
        0 => Form::Arena,
        bits if bits == OBJECT.inline_form => Form::Packed,
        _ => Form::Either,
    }
}

/// `IcOp::LoadSlot`: the value at `slot` of the reached record.
#[inline(always)]
pub fn load_slot<S: Stage>(stage: &mut S, at: &Reached<S::Word, S::Cond>, (slot, rep): (u16, Representation), form: Form, storage_len: usize,
    miss: S::Miss) -> Option<S::Word> {
    let layout = slot_layout(stage, at, form, storage_len, miss)?;
    Some(read_slot_value(stage, layout, slot, rep))
}

/// The layout `IcOp::LoadSlot` reads through: the reached record's, whose shape is guarded
/// and keeps `storage_len` values in `form`. A caller that proved it before reads through it
/// again instead of proving it twice (`property::kept_slot`).
#[inline(always)]
pub fn slot_layout<S: Stage>(stage: &mut S, at: &Reached<S::Word, S::Cond>, form: Form, storage_len: usize,
    miss: S::Miss) -> Option<Layout<S::Word, S::Cond>> {
    let shaped = at.shaped.expect("a slot read follows the guard of its shape");
    if let Some(cell) = shaped.cell() {
        return Some(cell_fields(stage, cell, storage_len));
    }
    if let Some(bits) = at.formed && let Some(layout) = super::object::proved_own_layout(stage, shaped, bits, storage_len) {
        return Some(layout);
    }
    let record = shaped.record();
    let inline = at.inline.unwrap_or_else(#[inline(always)] || inline_storage(stage, record));
    own_layout(stage, record, inline, form, storage_len, PackedAccess::Reload, miss)
}

/// The reached record's exact shape word: the caller's where it holds one, else loaded.
#[inline(always)]
pub fn shape_word<S: Stage>(stage: &mut S, at: &Reached<S::Word, S::Cond>) -> S::Word {
    at.shape.unwrap_or_else(#[inline(always)] || super::object::exact_shape(stage, at.record))
}

/// `IcOp::PrototypeConstants`: the value `values` pair with the link of `record`'s prototype.
#[inline(always)]
pub fn prototype_constants<S: Stage>(stage: &mut S, receiver: Shaped<S::Word>, values: &[(u32, u64)], miss: S::Miss) -> Option<S::Word> {
    let link = proto_link(stage, receiver.record());
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

/// `IcOp::HolderConstant`: the value the image linked for holder `key`, where `receiver`'s
/// prototype link is the holder's.
#[inline(always)]
pub fn holder_constant<S: Stage>(stage: &mut S, receiver: Shaped<S::Word>, key: u32, miss: S::Miss) -> Option<S::Word> {
    let link = proto_link(stage, receiver.record());
    let holder = stage.image_word(super::stage::ImageWord::HolderLink, key);
    let same = stage.eq_imm(link, holder);
    stage.guard(same, miss)?;
    let value = stage.image_word(super::stage::ImageWord::HolderValue, key);
    Some(stage.imm_word(value))
}

/// `IcOp::LoadPrototypeSlot` of the record at `record`.
#[inline(always)]
pub fn prototype_slot<S: Stage>(stage: &mut S, holder: Shaped<S::Word>, slot: u16, miss: S::Miss) -> Option<S::Word> {
    let record = holder.record();
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
