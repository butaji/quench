//! The heap rows: a captured value, a dense array element read or replaced, an entry-record
//! field, an array proof. Each is the shared staged row (`staged/{environment,elements,entry,
//! value}.rs`) run on `StencilStage`, so the layout reads have one definition across the
//! interpreter, the specializers and the stencils.
//!
//! Holes: 0 is the destination frame offset (the array's, for a store), 1 and 2 the operands'
//! (a field row's entry-record byte offset in 1); a capture's two per-site operands (the slot,
//! and the slot's byte offset) are holes 1 and 2. A site field read (`SITE_ROW`) packs the
//! object register's frame offset above the destination's in hole 0, takes the process's narrow
//! upper-word table as hole 1 and its cache entry's byte offset as hole 2. An array length read
//! (`LENGTH_ROW`, an array's or a string's) takes the destination in hole 0 and the base word's frame offset in hole 1. A
//! capture store (`CAPTURE_SET_ROW`) takes its value operand in hole 0 and the slot and its byte
//! offset in holes 1 and 2. A field store (`SITE_STORE_ROW`) takes the object register's frame
//! offset in hole 0, its value operand in hole 1 and the cache entry's byte offset in hole 2. A
//! chained site read (`ENTRY`) carries both entry offsets in hole 2. The rows of the activation's
//! receiver (`THIS_SITE_ROW`, `THIS_STORE_ROW`) take it from the payload, so their hole 0 is the
//! destination alone (a read) or unused (a store).
//! The global object's slot rows (`GLOBAL_READ_ROW`, `GLOBAL_STORE_ROW`) take its record id in hole 1
//! (the store: hole 0) and the slot's byte offset in hole 2; a constant row takes its word in hole 1
//! and the realm proof (`REALM_ROW`) a VM field offset in hole 1 and the global object's word in hole 2.
//! Every failed proof leaves through exit 0 with nothing written, so the interpreter runs the
//! instruction.

use crate::layout::{
    CALL_CONTEXT_ADAPTER_PAYLOAD, JS_STEP_VM, JS_STEP_ARRAY_RECORD_BASE, JS_STEP_CAPTURE_LEN, JS_STEP_CAPTURE_STORES, JS_STEP_CAPTURE_VALUES, JS_STEP_FIELD_ENTRIES, JS_STEP_LOOP_ENTRY,
    JS_STEP_OBJECT_RECORD_BASE, JS_STEP_PROPERTY_BUFFER, JS_STEP_RECEIVER, JS_STEP_STRING_RECORD_BASE, PROPERTY_BUFFER_BASE,
};
use crate::numeric::{HOLE_A, HOLE_B, Out, S, deliver, hole, hole64, index_of, number_word};
use crate::places::{DEST, LEFT, Places, tagged_place};
use crate::stage::StencilStage;
use crate::heap::NATIVE_ARRAY_LAYOUT as ARRAY;
use crate::staged::object::{self, Layout, Slots};
use crate::staged::{Stage, Step, elements, entry, environment, load_this, read_local, strings, value, write_local};

pub(crate) use crate::codes::{
    ARRAY_ROW, CAPTURE_ROW, CAPTURE_SET_ROW, CHAIN_SECOND_AT, CONSTANT_ROW, FIELD_ROW, GLOBAL_READ_ROW, GLOBAL_STORE_ROW, IDENTITY_ROW, INDEX_ROW, LENGTH_ROW, LOAD_LOCAL_ROW, MEM,
    REALM_ROW, SITE_OBJECT_AT, SITE_ROW, SITE_STORE_ROW, STORE_LOCAL_ROW, STORE_ROW, THIS_ROW, THIS_SITE_ROW, THIS_STORE_ROW, HELD_LINK_AT, HELD_ROW, LIVE_ROW,
};

unsafe extern "C" {
    static HOLE_V0: u8;
    static HOLE_V1: u8;
}

macro_rules! need {
    ($s:expr, $e:expr) => {
        match $e {
            Some(v) => v,
            None => return Out::Exit($s),
        }
    };
}

#[inline(always)]
pub(crate) fn word_at(base: usize, offset: usize) -> u64 {
    // SAFETY: a layout offset of a live record, as every staged load.
    unsafe { ((base + offset) as *const u64).read() }
}

/// The activation's dispatch payload: the context's adapter pointer.
#[inline(always)]
pub(crate) fn payload(s: &S) -> usize {
    word_at(s.ctx, CALL_CONTEXT_ADAPTER_PAYLOAD) as usize
}

/// The word the loop entry record at `entry` holds at the per-site byte `offset`: a field value the
/// loop's entry recorded (`LoopEntry`), written before the run and never during it.
#[inline(always)]
fn entry_word<S: Stage>(stage: &mut S, entry: S::Word, offset: S::Imm) -> S::Word {
    stage.load_site(entry, offset)
}

/// The dense backing of an array the loop entry recorded: its first value, at the per-site byte
/// `offset` of the entry record (which never changes during a run, so the load is of an immutable
/// table), and its length, read from the backing's header.
#[inline(always)]
fn entry_backing<S: Stage>(stage: &mut S, entry: S::Word, offset: S::Imm) -> (S::Word, S::Word) {
    let address = stage.add_imm(entry, offset);
    let values = stage.load_table(address);
    let length = stage.load_u32(values, ARRAY.header_length_offset as i32 - ARRAY.header_values_offset as i32);
    (values, length)
}

/// The array word an `ENTRY == false` row names: the frame slot at hole `H`.
#[inline(always)]
fn array_word<const H: u8>(s: &S) -> u64 {
    let offset = if H == 0 { &raw const HOLE_V0 as usize } else { &raw const HOLE_V1 as usize };
    word_at(s.frame, offset)
}

/// The backing `(values, length)` of the array an access names: the entry record's (the offset
/// at hole `H`, never looked up per access) or the array word's, proved a direct array.
#[inline(always)]
fn backing<const ENTRY: bool, const H: u8, const WRITABLE: bool>(s: &S, payload: usize) -> Option<(u64, u64, elements::Backing<u64>)> {
    let mut stage = StencilStage::from_hole(H);
    if ENTRY {
        let record = word_at(payload, JS_STEP_LOOP_ENTRY);
        let offset = stage_site(&mut stage);
        let (values, length) = entry_backing(&mut stage, record, offset);
        return Some((values, length, elements::Backing { record: 0, header: 0, values, length }));
    }
    let base = array_word::<H>(s);
    let record_base = word_at(payload, JS_STEP_ARRAY_RECORD_BASE);
    let backing = elements::array_backing(&mut stage, record_base, base, ())?;
    if WRITABLE {
        elements::writable_backing(&mut stage, backing, ())?;
    }
    Some((backing.values, backing.length, backing))
}

#[inline(always)]
fn stage_site(stage: &mut StencilStage) -> u64 {
    stage.site(0)
}

/// The hole holding the site table an element access reads its shape facts from.
const ELEMENT_SITES: u8 = 3;

#[inline(always)]
fn capture_row<const D: u8>(s: S, payload: usize) -> Out {
    let mut stage = StencilStage::from_hole(HOLE_A);
    let values = word_at(payload, JS_STEP_CAPTURE_VALUES);
    let len = word_at(payload, JS_STEP_CAPTURE_LEN);
    let word = need!(s, environment::dense_capture(&mut stage, values, len, 0, ()));
    deliver::<D>(s, word)
}

/// A store of the value operand (hole 0) into the same capture slot (holes 1 and 2), where the
/// run's slots admit every direct store (`environment::dense_capture_store`).
#[inline(always)]
fn capture_set_row<const A: u8>(s: S, payload: usize) -> Out {
    let mut stage = StencilStage::from_hole(HOLE_A);
    let slots = (word_at(payload, JS_STEP_CAPTURE_VALUES), word_at(payload, JS_STEP_CAPTURE_LEN), word_at(payload, JS_STEP_CAPTURE_STORES));
    need!(s, environment::dense_capture_store(&mut stage, slots, 0, tagged_place::<A, 0>(&s), ()));
    Out::Next(s)
}

#[inline(always)]
fn field_row<const D: u8>(s: S, payload: usize) -> Out {
    let mut stage = StencilStage::from_hole(HOLE_A);
    let record = word_at(payload, JS_STEP_LOOP_ENTRY);
    let offset = stage_site(&mut stage);
    deliver::<D>(s, entry_word(&mut stage, record, offset))
}

/// A field read through the site's cache entry (`entry::site_read`), or with `CHAIN` through two
/// (`entry::site_chain`, whose first and second entry offsets are hole 2's low and high halves):
/// `object` is the receiver the row names.
#[inline(always)]
fn site_row<const D: u8, const CHAIN: bool>(s: S, payload: usize, object: u64) -> Out {
    let header = word_at(payload, JS_STEP_PROPERTY_BUFFER) as usize;
    let mut stage = StencilStage::with_values(HOLE_B, word_at(header, PROPERTY_BUFFER_BASE));
    let tables = (word_at(payload, JS_STEP_FIELD_ENTRIES), word_at(payload, JS_STEP_OBJECT_RECORD_BASE));
    let word = if CHAIN {
        let offsets = hole64::<HOLE_B>();
        entry::site_chain(&mut stage, tables, object, (offsets & u64::from(u32::MAX), offsets >> CHAIN_SECOND_AT), ())
    } else {
        let offset = stage_site(&mut stage);
        entry::site_read(&mut stage, tables, object, offset, ())
    };
    deliver::<D>(s, need!(s, word))
}

/// A store of the value operand (hole 1) through the site's cache entry (hole 2) into the object
/// `object` names (`entry::site_store`): a leave writes nothing.
#[inline(always)]
fn site_store_row<const A: u8>(s: S, payload: usize, object: u64) -> Out {
    let header = word_at(payload, JS_STEP_PROPERTY_BUFFER) as usize;
    let mut stage = StencilStage::storing(HOLE_B, word_at(header, PROPERTY_BUFFER_BASE), payload);
    let tables = (word_at(payload, JS_STEP_FIELD_ENTRIES), word_at(payload, JS_STEP_OBJECT_RECORD_BASE));
    let offset = stage_site(&mut stage);
    need!(s, entry::site_store(&mut stage, tables, object, offset, tagged_place::<A, HOLE_A>(&s), ()));
    Out::Next(s)
}

/// The object register a site row names: its frame offset sits above the destination's in hole 0.
#[inline(always)]
fn register_object(s: &S) -> u64 {
    word_at(s.frame, (hole64::<0>() >> SITE_OBJECT_AT) as usize)
}

/// A prototype-held field folded to its value (hole 2): `object` is a direct object of the exact shape in
/// hole 1's low half whose prototype link is its high half (`entry::held_receiver`), else the row leaves.
#[inline(always)]
fn held_row<const D: u8>(s: S, payload: usize, object: u64) -> Out {
    let mut stage = StencilStage::from_hole(HOLE_B);
    let records = word_at(payload, JS_STEP_OBJECT_RECORD_BASE);
    let receiver = hole64::<HOLE_A>();
    need!(s, entry::held_receiver(&mut stage, records, object, (receiver & u64::from(u32::MAX), receiver >> HELD_LINK_AT), ()));
    deliver::<D>(s, hole64::<HOLE_B>())
}

/// Continues while the invalidation word at the address in hole 1 is zero: a Stable fact the code holds
/// changed where it is set (`code_dependencies::Invalidation`).
#[inline(always)]
fn live_row(s: S) -> Out {
    if word_at(hole64::<HOLE_A>() as usize, 0) == 0 { Out::Next(s) } else { Out::Exit(s) }
}

/// The values of the global object's shape a row's layout proof counts: the one slot the row names,
/// which the stage's per-site byte offset selects. The shape itself is held by a dependency (the
/// global layout fact), so the row proves only the record's representation.
const GLOBAL_SLOT_COUNT: usize = 1;

/// The layout of the global object `id` names in the record table at the payload: `None` where the
/// table is absent or the object's values are not behind its extras in the arena.
#[inline(always)]
fn global_layout(stage: &mut StencilStage, payload: usize, id: u64) -> Option<Layout<u64, bool>> {
    let records = word_at(payload, JS_STEP_OBJECT_RECORD_BASE) as u64;
    if records == 0 {
        return None;
    }
    let record = object::object_record_at(stage, records, id);
    let inline = object::inline_storage(stage, record);
    object::arena_extras_layout(stage, record, inline, GLOBAL_SLOT_COUNT, ())
}

/// The global object's slot (hole 2: its byte offset) read into the destination.
#[inline(always)]
fn global_read_row<const D: u8>(s: S, payload: usize) -> Out {
    let header = word_at(payload, JS_STEP_PROPERTY_BUFFER) as usize;
    let mut stage = StencilStage::with_values(HOLE_B, word_at(header, PROPERTY_BUFFER_BASE));
    let layout = need!(s, global_layout(&mut stage, payload, hole::<HOLE_A>() as u64));
    deliver::<D>(s, object::read_slot(&mut stage, layout, 0))
}

/// The first operand stored to the global object's slot (hole 0: the record id), through the store
/// gate where `GATED`: a value that may be a heap reference joins the barrier's log, and a store
/// the property arena observes (`slots_tracked`) is the VM's.
#[inline(always)]
fn global_store_row<const A: u8, const GATED: bool>(s: S, payload: usize) -> Out {
    let header = word_at(payload, JS_STEP_PROPERTY_BUFFER) as usize;
    let mut stage = StencilStage::storing(HOLE_B, word_at(header, PROPERTY_BUFFER_BASE), payload);
    let id = hole::<0>() as u64;
    let value = tagged_place::<A, HOLE_A>(&s);
    let layout = need!(s, global_layout(&mut stage, payload, id));
    let Slots::Flat(base) = layout.slots else { return Out::Exit(s) };
    if GATED {
        let open = stage.store_gate(id, value);
        need!(s, stage.guard(open, ()));
    }
    let offset = stage.site(0);
    let address = stage.add_imm(base, offset);
    stage.store_u64(address, 0, value);
    Out::Next(s)
}

/// The destination takes the word of hole 1.
#[inline(always)]
fn constant_row<const D: u8>(s: S) -> Out {
    deliver::<D>(s, hole64::<HOLE_A>())
}

/// Continues while the VM's active realm names the global object word of hole 2 (the realm field
/// is at the byte offset of hole 1 behind the payload's VM), else leaves through the taken successor.
#[inline(always)]
fn realm_row(s: S, payload: usize) -> Out {
    let vm = word_at(payload, JS_STEP_VM);
    if word_at(vm as usize, hole::<HOLE_A>()) == hole64::<HOLE_B>() { Out::Next(s) } else { Out::Taken(s) }
}

/// The `length` of the array or string word at hole 1 (`strings::length_of`).
#[inline(always)]
fn length_row<const D: u8>(s: S, payload: usize) -> Out {
    let mut stage = StencilStage::from_hole(0);
    let base = array_word::<HOLE_A>(&s);
    let records = (word_at(payload, JS_STEP_ARRAY_RECORD_BASE), word_at(payload, JS_STEP_STRING_RECORD_BASE));
    deliver::<D>(s, need!(s, strings::length_of(&mut stage, records, base, ())))
}

/// Whether the row's staged work was done and fit its place: it continues, else it leaves.
#[inline(always)]
fn finish<const D: u8, const A: u8>(frame: Places<D, A, MEM>, done: bool) -> Out {
    if frame.writes_fit(done) { Out::Next(frame.s) } else { Out::Exit(frame.s) }
}

/// The activation's `this` into the destination (`staged::load_this`).
#[inline(always)]
fn this_row<const D: u8>(s: S) -> Out {
    let mut frame = Places::<D, MEM, MEM>::new(s);
    let done = matches!(load_this(&mut StencilStage::from_hole(0), &mut frame, DEST, ()), Step::Value(_));
    finish(frame, done)
}

/// A local slot read: the slot number is hole 1 (`staged::read_local`).
#[inline(always)]
fn load_local_row<const D: u8>(s: S) -> Out {
    let mut frame = Places::<D, MEM, MEM>::new(s);
    let done = read_local(&mut StencilStage::from_hole(0), &mut frame, DEST, hole::<HOLE_A>() as u16, Some(())).is_some();
    finish(frame, done)
}

/// A local slot write of the first operand: the slot number is hole 2 (`staged::write_local`).
#[inline(always)]
fn store_local_row<const A: u8>(s: S) -> Out {
    let mut frame = Places::<MEM, A, MEM>::new(s);
    let done = write_local(&mut StencilStage::from_hole(0), &mut frame, LEFT, hole::<HOLE_B>() as u16, Some(())).is_some();
    finish(frame, done)
}

/// The register at frame offset hole 1 holds exactly the word of hole 2.
#[inline(always)]
fn identity_row(s: S) -> Out {
    if word_at(s.frame, hole::<HOLE_A>()) == hole64::<HOLE_B>() { Out::Next(s) } else { Out::Exit(s) }
}

#[inline(always)]
fn array_row(s: S) -> Out {
    let mut stage = StencilStage::from_hole(0);
    let base = array_word::<HOLE_A>(&s);
    if value::is_direct_array(&mut stage, base) { Out::Next(s) } else { Out::Exit(s) }
}

/// An element read: the array word is hole 1; an entry row's record offset is hole 1 too.
#[inline(always)]
fn index_row<const ENTRY: bool, const D: u8, const B: u8>(s: S, payload: usize) -> Out {
    let index = need!(s, index_of::<B, HOLE_B>(&s));
    let (values, length, _) = need!(s, backing::<ENTRY, HOLE_A, false>(&s, payload));
    let mut stage = StencilStage::from_hole(ELEMENT_SITES);
    let (_, word) = need!(s, elements::dense_element(&mut stage, values, length, u64::from(index as u32), ()));
    deliver::<D>(s, word)
}

/// An element store: the array is hole 0, the value hole 1, the index hole 2.
#[inline(always)]
fn store_row<const ENTRY: bool, const A: u8, const B: u8>(s: S, payload: usize) -> Out {
    let index = need!(s, index_of::<B, HOLE_B>(&s));
    let word = need!(s, number_word::<A, HOLE_A>(&s));
    let (values, length, _) = need!(s, backing::<ENTRY, 0, true>(&s, payload));
    let mut stage = StencilStage::from_hole(ELEMENT_SITES);
    let (address, old) = need!(s, elements::dense_element(&mut stage, values, length, u64::from(index as u32), ()));
    elements::replace_element(&mut stage, values, address, old, word);
    Out::Next(s)
}

#[inline(always)]
pub(crate) fn run<const KIND: u8, const ENTRY: bool, const D: u8, const A: u8, const B: u8>(s: S) -> Out {
    let payload = payload(&s);
    match KIND {
        CAPTURE_ROW => capture_row::<D>(s, payload),
        CAPTURE_SET_ROW => capture_set_row::<A>(s, payload),
        FIELD_ROW => field_row::<D>(s, payload),
        SITE_ROW => site_row::<D, ENTRY>(s, payload, register_object(&s)),
        SITE_STORE_ROW => site_store_row::<A>(s, payload, word_at(s.frame, hole::<0>())),
        THIS_SITE_ROW => site_row::<D, ENTRY>(s, payload, word_at(payload, JS_STEP_RECEIVER)),
        THIS_STORE_ROW => site_store_row::<A>(s, payload, word_at(payload, JS_STEP_RECEIVER)),
        HELD_ROW => held_row::<D>(s, payload, if ENTRY { word_at(payload, JS_STEP_RECEIVER) } else { register_object(&s) }),
        LIVE_ROW => live_row(s),
        GLOBAL_READ_ROW => global_read_row::<D>(s, payload),
        GLOBAL_STORE_ROW => global_store_row::<A, ENTRY>(s, payload),
        CONSTANT_ROW => constant_row::<D>(s),
        REALM_ROW => realm_row(s, payload),
        ARRAY_ROW => array_row(s),
        IDENTITY_ROW => identity_row(s),
        THIS_ROW => this_row::<D>(s),
        LOAD_LOCAL_ROW => load_local_row::<D>(s),
        STORE_LOCAL_ROW => store_local_row::<A>(s),
        LENGTH_ROW => length_row::<D>(s, payload),
        INDEX_ROW => index_row::<ENTRY, D, B>(s, payload),
        _ => store_row::<ENTRY, A, B>(s, payload),
    }
}
