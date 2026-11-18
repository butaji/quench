//! The heap rows: a captured value, a dense or typed array element read or replaced, an entry-record
//! field, an array proof. Each is the shared staged row (`staged/{environment,elements,entry,
//! value}.rs`) run on `StencilStage`, so the layout reads have one definition across the
//! interpreter, the specializers and the stencils.
//!
//! The inline-cache rows (`ic_rows.rs`) are dispatched from `run`; for them `ENTRY` is `CHAINED`.
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

use crate::layout::{LOOP_ENTRY_ARRAY_COUNT_AT, TypedArrayKind, CALL_CONTEXT_ADAPTER_PAYLOAD, JS_STEP_ARRAY_RECORD_BASE, JS_STEP_LOOP_ENTRY, JS_STEP_STRING_RECORD_BASE};
use crate::ic_rows;
use crate::numeric::{HOLE_A, HOLE_B, Out, S, deliver, index_of, number_word};
use crate::words::{canonical, hole, hole64, int_word, tag_languages};
use crate::places::{DEST, LEFT, Places, number_place};
use crate::stage::StencilStage;
use crate::staged::{Frame, Stage, Step, elements, entry, load_this, read_initialized_local, read_local, string_length, typed_elements, value, write_initialized_local, write_local};
use crate::codes::{ARRAY_ROW, CALLEE_CAPTURE_ROW, CAPTURE_ROW, CAPTURE_SET_ROW, CONSTANT_ROW, ENV_DEFINE_ROW, ENV_ROW, ENV_SET_ROW, FIELD_ROW, GLOBAL_READ_ROW, GLOBAL_STORE_ROW, DECLARED_ROW, IDENTITY_ROW, INDEX_ROW, TYPED_INDEX_ROW, TYPED_INDEX_LAST, TYPED_STORE_ROW, TYPED_STORE_LAST, TYPED_LENGTH_ROW, LENGTH_ROW, LEXICAL_READ_ROW, LEXICAL_WRITE_ROW, LOAD_LOCAL_ROW, MEM, SITE_ROW, SITE_STORE_ROW, STORE_LOCAL_ROW, STORE_CHECKED_ROW, LOAD_CHECKED_ROW, THIS_ROW, THIS_SITE_ROW, THIS_STORE_ROW, HELD_ROW, LIVE_ROW, REALM_ROW, KEY_ROW, OBJECT_ROW, TYPEOF_INDEX_ROW, TYPEOF_STRING_ROW, TYPEOF_IS_ROW, TYPEOF_ISNOT_ROW, INSTANCEOF_ROW, APPEND_ROW, ADD_ARENA_ROW, ADD_EITHER_ROW, ADD_PACKED_ROW, THIS_ADD_ARENA_ROW, THIS_ADD_EITHER_ROW, THIS_ADD_PACKED_ROW, FOR_IN_ROW, NAME_ARENA_ROW, NAME_EITHER_ROW, IC_GET_HOLDER_ROW, IC_GET_ROW, IC_SET_INT_ROW, IC_SET_ROW, IC_PROOF_ROW, IC_KNOWN_GET_ROW, IC_KNOWN_SET_ROW, IC_KNOWN_SET_INT_ROW};
use crate::codes::{CHAR_CODE_AT_ROW, PARSE_INT_ROW};
use crate::staged::{code_units, parse_int};
use crate::{environment_rows, key_rows, name_rows, site_rows};

unsafe extern "C" {
    static HOLE_V0: u8;
    static HOLE_V1: u8;
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

/// The array word an `ENTRY == false` row names: the frame slot at hole `H`.
#[inline(always)]
fn array_word<const H: u8>(s: &S) -> u64 {
    word_at(s.frame, hole::<H>())
}

/// The backing `(values, length)` of the array an access names: the entry record's (the offset
/// at hole `H`, never looked up per access) or the array word's, proved a direct array.
#[inline(always)]
fn backing<const ENTRY: bool, const H: u8, const WRITABLE: bool>(s: &S, payload: usize) -> Option<(u64, u64, elements::Backing<u64>)> {
    let mut stage = StencilStage::from_hole(H);
    if ENTRY {
        let record = word_at(payload, JS_STEP_LOOP_ENTRY);
        let offset = stage_site(&mut stage);
        let (values, length) = entry::entry_backing(&mut stage, record, offset);
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
pub(crate) fn stage_site(stage: &mut StencilStage) -> u64 {
    stage.site(0)
}

/// The hole holding the site table an element access reads its shape facts from.
const ELEMENT_SITES: u8 = 3;

/// The `length` of the array or string word at hole 1 (`string_length::length_of`).
#[inline(always)]
fn length_row<const D: u8>(s: S, payload: usize) -> Out {
    let mut stage = StencilStage::from_hole(0);
    let base = array_word::<HOLE_A>(&s);
    let records = (word_at(payload, JS_STEP_ARRAY_RECORD_BASE), word_at(payload, JS_STEP_STRING_RECORD_BASE));
    deliver::<D>(s, need!(s, string_length::length_of(&mut stage, records, base, ())))
}

/// Whether the row's staged work was done and fit its place: it continues, else it leaves.
#[inline(always)]
fn finish<const D: u8, const A: u8>(frame: Places<D, A, MEM>, done: bool) -> Out {
    if frame.writes_fit(done) { Out::Next(frame.s) } else { Out::Exit(frame.s) }
}

/// A row's hit continues at TAKEN, past the helper step laid out after it where it is a fast arm; a miss
/// leaves as it did.
#[inline(always)]
pub(crate) fn hop(out: Out) -> Out {
    match out {
        Out::Next(s) => Out::Taken(s),
        left => left,
    }
}

/// A fast arm's row: where its proof held and its places fit it continues at TAKEN, past the
/// helper step laid out after it, else it leaves into that step.
#[inline(always)]
pub(crate) fn finish_fast<const D: u8, const A: u8>(frame: Places<D, A, MEM>, done: bool) -> Out {
    hop(finish(frame, done))
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

/// A local slot write of the first operand: the slot number is hole 2 (`staged::write_local`). A hit continues at
/// TAKEN, as every row that has a helper step behind it does (`finish_fast`).
#[inline(always)]
fn store_local_row<const A: u8>(s: S) -> Out {
    let mut frame = Places::<MEM, A, MEM>::new(s);
    let done = write_local(&mut StencilStage::from_hole(0), &mut frame, LEFT, hole::<HOLE_B>() as u16, Some(())).is_some();
    finish_fast(frame, done)
}

/// A fast arm's local slot read that leaves into the helper step for a binding in its temporal dead zone:
/// the slot number is hole 1 (`staged::read_initialized_local`).
#[inline(always)]
fn load_checked_row<const D: u8>(s: S) -> Out {
    let mut frame = Places::<D, MEM, MEM>::new(s);
    let done = read_initialized_local(&mut StencilStage::from_hole(0), &mut frame, DEST, hole::<HOLE_A>() as u16, ()).is_some();
    finish_fast(frame, done)
}

/// A fast arm's local slot write of the first operand to an initialized binding: the slot number is
/// hole 2 (`staged::write_initialized_local`).
#[inline(always)]
fn store_checked_row<const A: u8>(s: S) -> Out {
    let mut frame = Places::<MEM, A, MEM>::new(s);
    let done = write_initialized_local(&mut StencilStage::from_hole(0), &mut frame, LEFT, hole::<HOLE_B>() as u16, ()).is_some();
    finish_fast(frame, done)
}

/// The register at frame offset hole 1 holds exactly the word of hole 2.
#[inline(always)]
fn identity_row(s: S) -> Out {
    if word_at(s.frame, hole::<HOLE_A>()) == hole64::<HOLE_B>() { Out::Next(s) } else { Out::Exit(s) }
}

/// A declared parameter's entry test: the frame slot at hole 0 holds a word whose tag-decided languages
/// (`tag_languages`) meet the declared mask at hole 1; any other word leaves with nothing written.
#[inline(always)]
fn declared_row(s: S) -> Out {
    if tag_languages(word_at(s.frame, hole::<0>())) & hole64::<HOLE_A>() != 0 { Out::Next(s) } else { Out::Exit(s) }
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
    if !ENTRY {
        // A wide or narrow backing; the upper-word table a narrow element decodes through is hole 4.
        let mut stage = StencilStage::from_hole(ELEMENT_SITES);
        let (records, base) = (word_at(payload, JS_STEP_ARRAY_RECORD_BASE), array_word::<HOLE_A>(&s));
        let word = need!(s, elements::record_element(&mut stage, records, base, u64::from(index as u32), hole64::<{ crate::words::LAST_HOLE }>(), ()));
        return deliver::<D>(s, word);
    }
    let (values, length, _) = need!(s, backing::<ENTRY, HOLE_A, false>(&s, payload));
    let mut stage = StencilStage::from_hole(ELEMENT_SITES);
    let word = need!(s, elements::read_element(&mut stage, elements::DenseView { values, length, packed: false, within: false }, u64::from(index as u32), ()));
    deliver::<D>(s, word)
}

/// An element store: the array is hole 0, the value hole 1, the index hole 2.
#[inline(always)]
fn store_row<const ENTRY: bool, const A: u8, const B: u8>(s: S, payload: usize) -> Out {
    let index = need!(s, index_of::<B, HOLE_B>(&s));
    let word = need!(s, number_word::<A, HOLE_A>(&s));
    let (values, length, _) = need!(s, backing::<ENTRY, 0, true>(&s, payload));
    let mut stage = StencilStage::from_hole(ELEMENT_SITES);
    need!(s, elements::replace_present(&mut stage, elements::DenseView { values, length, packed: false, within: false }, u64::from(index as u32), word, ()));
    Out::Next(s)
}

/// The typed view the loop's entry recorded for the array whose entry words lie at the record offset in value hole
/// `at`, of the kind row `KIND` (counted from `first`) plans: the entry proved the kind once.
#[inline(always)]
fn typed_view<const KIND: u8>(payload: usize, (at, first): (u8, u8)) -> Option<typed_elements::TypedView<u64>> {
    let native = typed_elements::NativeKind::new(TypedArrayKind::ALL[usize::from(KIND - first)])?;
    let mut stage = StencilStage::from_hole(at);
    let offset = stage_site(&mut stage);
    let record = word_at(payload, JS_STEP_LOOP_ENTRY);
    let values = entry::entry_word(&mut stage, record, offset);
    let length = entry::entry_word(&mut stage, record, offset + LOOP_ENTRY_ARRAY_COUNT_AT);
    Some(typed_elements::TypedView { values, length, kind: typed_elements::ViewKind::Static(native), within: false })
}

/// A typed element read: the entry words' offset is hole 1, the Number delivered as `Value::number` makes it.
#[inline(always)]
fn typed_index_row<const KIND: u8, const D: u8, const B: u8>(s: S, payload: usize) -> Out {
    let index = need!(s, index_of::<B, HOLE_B>(&s));
    let view = need!(s, typed_view::<KIND>(payload, (HOLE_A, TYPED_INDEX_ROW)));
    let number = need!(s, typed_elements::typed_read(&mut StencilStage::from_hole(0), view, u64::from(index as u32), ()));
    deliver::<D>(s, canonical(f64::from_bits(number)))
}

/// A typed element store of the Number operand (hole 1): the entry words' offset is hole 0, the index hole 2.
#[inline(always)]
fn typed_store_row<const KIND: u8, const A: u8, const B: u8>(s: S, payload: usize) -> Out {
    let index = need!(s, index_of::<B, HOLE_B>(&s));
    let number = need!(s, number_place::<A, HOLE_A>(&s));
    let view = need!(s, typed_view::<KIND>(payload, (0, TYPED_STORE_ROW)));
    need!(s, typed_elements::typed_write(&mut StencilStage::from_hole(0), view, u64::from(index as u32), number, ()));
    Out::Next(s)
}

/// A typed array's `length`: the count the entry words (offset in hole 1) hold, an int32.
#[inline(always)]
fn typed_length_row<const D: u8>(s: S, payload: usize) -> Out {
    let mut stage = StencilStage::from_hole(HOLE_A);
    let offset = stage_site(&mut stage);
    let length = entry::entry_word(&mut stage, word_at(payload, JS_STEP_LOOP_ENTRY), offset + LOOP_ENTRY_ARRAY_COUNT_AT);
    deliver::<D>(s, int_word(length as u32 as i32))
}

/// `AppendArray`'s fast arm: the Number operand (hole 1) appended to the array at hole 0 where its
/// backing is writable and its capacity word has room (`elements::append_in_room`); a hit continues at TAKEN.
#[inline(always)]
fn append_row<const A: u8>(s: S, payload: usize) -> Out {
    let word = need!(s, number_word::<A, HOLE_A>(&s));
    let (_, _, backing) = need!(s, backing::<false, 0, true>(&s, payload));
    let mut stage = StencilStage::from_hole(ELEMENT_SITES);
    need!(s, elements::append_in_room(&mut stage, backing, word, ()));
    Out::Taken(s)
}

/// `charCodeAt` of the flat string in the frame slot at hole 1, at the int32 index at hole 2 where it is in range: the code unit
/// (`code_units::code_unit`), an int32. Any other receiver, form, index or unit leaves with nothing written.
#[inline(always)]
fn char_code_at_row<const D: u8, const B: u8>(s: S, payload: usize) -> Out {
    let index = need!(s, index_of::<B, HOLE_B>(&s));
    let base = array_word::<HOLE_A>(&s);
    let mut stage = StencilStage::from_hole(0);
    let string = need!(s, string_length::flat_string(&mut stage, word_at(payload, JS_STEP_STRING_RECORD_BASE), base, ()));
    if index < 0 || index as u64 >= string.length {
        return Out::Exit(s);
    }
    let unit = code_units::code_unit(&mut stage, string.wide != 0, string.units, index as u64);
    deliver::<D>(s, int_word(unit as u32 as i32))
}

/// `parseInt` of the flat string in the frame slot at hole 1, in radix 10: the int32 `parse_int::decimal_int32` answers over its
/// code units. Any other text leaves with nothing written.
#[inline(always)]
fn parse_int_row<const D: u8>(s: S, payload: usize) -> Out {
    let base = array_word::<HOLE_A>(&s);
    let mut stage = StencilStage::from_hole(0);
    let string = need!(s, string_length::flat_string(&mut stage, word_at(payload, JS_STEP_STRING_RECORD_BASE), base, ()));
    let (units, wide) = (string.units, string.wide != 0);
    let value = need!(s, parse_int::decimal_int32((0..string.length).map(|index| code_units::code_unit(&mut stage, wide, units, index) as u16)));
    deliver::<D>(s, int_word(value))
}

#[inline(always)]
fn register_row<const KIND: u8, const D: u8, const A: u8>(s: S) -> Out {
    match KIND {
        IDENTITY_ROW => identity_row(s),
        THIS_ROW => this_row::<D>(s),
        LOAD_LOCAL_ROW => load_local_row::<D>(s),
        STORE_LOCAL_ROW => store_local_row::<A>(s),
        LOAD_CHECKED_ROW => load_checked_row::<D>(s),
        _ => store_checked_row::<A>(s),
    }
}

#[inline(always)]
fn element_row<const KIND: u8, const ENTRY: bool, const D: u8, const A: u8, const B: u8>(s: S, payload: usize) -> Out {
    match KIND {
        ARRAY_ROW => array_row(s),
        LENGTH_ROW => length_row::<D>(s, payload),
        INDEX_ROW => index_row::<ENTRY, D, B>(s, payload),
        TYPED_INDEX_ROW..=TYPED_INDEX_LAST => typed_index_row::<KIND, D, B>(s, payload),
        TYPED_STORE_ROW..=TYPED_STORE_LAST => typed_store_row::<KIND, A, B>(s, payload),
        TYPED_LENGTH_ROW => typed_length_row::<D>(s, payload),
        APPEND_ROW => append_row::<A>(s, payload),
        _ => store_row::<ENTRY, A, B>(s, payload),
    }
}

#[inline(always)]
pub(crate) fn run<const KIND: u8, const ENTRY: bool, const D: u8, const A: u8, const B: u8>(s: S) -> Out {
    let payload = payload(&s);
    match KIND {
        CAPTURE_ROW | CAPTURE_SET_ROW | ENV_ROW | ENV_SET_ROW | ENV_DEFINE_ROW | GLOBAL_READ_ROW | GLOBAL_STORE_ROW
            | LEXICAL_READ_ROW | LEXICAL_WRITE_ROW | CALLEE_CAPTURE_ROW =>
            environment_rows::run::<KIND, ENTRY, D, A>(s, payload),
        FIELD_ROW | SITE_ROW | SITE_STORE_ROW | THIS_SITE_ROW | THIS_STORE_ROW | HELD_ROW | LIVE_ROW | REALM_ROW | CONSTANT_ROW =>
            site_rows::run::<KIND, ENTRY, D, A>(s, payload),
        OBJECT_ROW | KEY_ROW | TYPEOF_INDEX_ROW | TYPEOF_STRING_ROW | TYPEOF_IS_ROW | TYPEOF_ISNOT_ROW | INSTANCEOF_ROW | FOR_IN_ROW =>
            key_rows::run::<KIND>(s, payload),
        ADD_PACKED_ROW | ADD_ARENA_ROW | ADD_EITHER_ROW | THIS_ADD_PACKED_ROW | THIS_ADD_ARENA_ROW | THIS_ADD_EITHER_ROW
            | NAME_EITHER_ROW | NAME_ARENA_ROW => name_rows::run::<KIND, ENTRY, A>(s, payload),
        IDENTITY_ROW | THIS_ROW | LOAD_LOCAL_ROW | STORE_LOCAL_ROW | LOAD_CHECKED_ROW | STORE_CHECKED_ROW => register_row::<KIND, D, A>(s),
        IC_GET_ROW | IC_GET_HOLDER_ROW | IC_SET_ROW | IC_SET_INT_ROW | IC_PROOF_ROW | IC_KNOWN_GET_ROW | IC_KNOWN_SET_ROW | IC_KNOWN_SET_INT_ROW =>
            ic_rows::run::<KIND, ENTRY, D, A>(s),
        DECLARED_ROW => declared_row(s),
        CHAR_CODE_AT_ROW => char_code_at_row::<D, B>(s, payload),
        PARSE_INT_ROW => parse_int_row::<D>(s, payload),
        _ => element_row::<KIND, ENTRY, D, A, B>(s, payload),
    }
}
