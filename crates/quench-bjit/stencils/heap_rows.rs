//! The heap rows: a captured value, a dense or typed array element read or replaced, an entry-record
//! field, an array proof. Each is the shared staged row (`staged/{environment,elements,entry,
//! value}.rs`) run on `StencilStage`, so the layout reads have one definition across the
//! interpreter, the specializers and the stencils.
//!
//! The inline-cache rows (`ic_rows.rs`) are dispatched from `run`; for them `ENTRY` is `CHAINED`.
//!
//! The `this` row, the plain local rows, the array proof, the array and typed-array lengths, the
//! dense element read and store and the array append are the staged `Operation`s the interpreter
//! decodes (`LoadThis`, `LoadLocalPlain`, `StoreLocalPlain`, `DirectArray`, `ArrayLength`,
//! `TypedLength`, `ElementRead`, `ElementWrite`, `ArrayAppend`), run over the row's places
//! (`places.rs`) with the frame answers of `places_heap.rs`. An entry element row (dense or typed)
//! runs the same `ElementRead` / `ElementWrite` through `places_view.rs`, whose view is the one the
//! loop's entry proved at the site its hole names; the entry-record field row reads that site too.
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
//! receiver (`THIS_SITE_ROW`, `THIS_STORE_ROW`) take it from their window's header (`context_word`), so their hole 0 is the
//! destination alone (a read) or unused (a store).
//! The global object's slot rows (`GLOBAL_READ_ROW`, `GLOBAL_STORE_ROW`) take its record id in hole 1
//! (the store: hole 0) and the slot's byte offset in hole 2; a constant row takes its word in hole 1
//! and the realm proof (`REALM_ROW`) a VM field offset in hole 1 and the global object's word in hole 2.
//! Every failed proof leaves through exit 0 with nothing written, so the interpreter runs the
//! instruction.

use crate::bytecode::Operand;
// A store's strictness decides only the VM's refused store, which a row leaves to at its miss: the
// rows build their writes `strict: false`.
use crate::layout::{CONTEXT_BODY, JS_STEP_ARRAY_CURSOR, JS_STEP_ARRAY_RECORD_BASE, JS_STEP_CLASS_BASE, JS_STEP_EMPTY_ARRAY_LAYOUT, JS_STEP_EMPTY_ARRAY_PAYLOAD, JS_STEP_EMPTY_OBJECT_CLASS, JS_STEP_EMPTY_OBJECT_CURSOR, JS_STEP_STORES_SPECIALIZED, JS_STEP_STRING_RECORD_BASE, NATIVE_ARRAY_LAYOUT, NATIVE_OBJECT_LAYOUT, NATIVE_RECORD_CURSOR_LAYOUT};
use crate::layout::words::DIRECT_ARRAY_TAG;
use crate::staged::bump::{ClassCell, claim_empty_object};
use crate::layout::shared_words::{capture_len, capture_values};
use crate::ic_rows;
use crate::numeric::{HOLE_A, HOLE_B, Out, S, deliver, index_of};
use crate::words::{canonical, hole, hole64, hop, int_word};
use crate::places::{DEST, LEFT, RIGHT, Places, finish, finish_fast};
pub(crate) use crate::stage::stage_site;
pub(crate) use crate::words::{context_word, payload, set_context_word, word_at};
use crate::stage::StencilStage;
use crate::staged::{Frame, Operation, Stage, Step, op_kind, step_local_of, step_of, step_value_of, string_length};
use crate::codes::{ADD_CLASS_ROW, IC_GET_CLASS_ROW, IC_GET_HELD_CLASS_ROW, IC_SET_CLASS_ROW, EMPTY_ARRAY_ROW, EMPTY_OBJECT_ROW, LAST_INT_PIN, MEMINT, PIN, PINF, APPEND_ROW, ARRAY_ROW, CALLEE_CAPTURE_ROW, CAPTURE_IDENTITY_ROW, CAPTURE_ROW, CAPTURE_SET_ROW, CHAR_CODE_AT_ROW, CONSTANT_ROW, DECLARED_ROW, ENV_DEFINE_ROW, CLASS_ROW, STORES_ROW, ENV_ROW, ENV_SET_ROW, FIELD_ROW, FOR_IN_ROW, GLOBAL_READ_ROW, GLOBAL_STORE_ROW, HELD_ROW, IC_GET_HOLDER_ROW, IC_GET_ROW, IC_KNOWN_GET_ROW, IC_KNOWN_GET_FLAT_ROW, IC_KNOWN_SET_FLAT_ROW, IC_KNOWN_SET_INT_FLAT_ROW, IC_KNOWN_SET_FLAT_ADMITTED_ROW, IC_KNOWN_SET_INT_FLAT_ADMITTED_ROW, IC_KNOWN_SET_INT_ADMITTED_ROW, IC_KNOWN_SET_INT_ROW, IC_KNOWN_SET_ADMITTED_ROW, IC_KNOWN_SET_ROW, IC_PROOF_ROW, IC_SET_INT_ROW, IC_SET_ROW, IDENTITY_ROW, INDEX_ROW, KEY_ROW, LENGTH_ROW, LEXICAL_READ_ROW, LEXICAL_WRITE_ROW, LIVE_ROW, LOAD_CHECKED_ROW, LOAD_LOCAL_ROW, MEM, OBJECT_ROW, PARSE_INT_ROW, REALM_ROW, SITE_ROW, SITE_STORE_ROW, STAGED_FIRST, STAGED_LAST, STORE_CHECKED_ROW, STORE_LOCAL_ROW, THIS_ROW, THIS_SITE_ROW, THIS_STORE_ROW, TYPED_INDEX_LAST, TYPED_INDEX_ROW, TYPED_LENGTH_ROW, TYPED_STORE_LAST, TYPED_STORE_ROW, TYPEOF_INDEX_ROW, TYPEOF_ISNOT_ROW, TYPEOF_IS_ROW, TYPEOF_STRING_ROW};
use crate::staged::{code_units, environment, parse_int};
use crate::places_view::{ELEMENT_SITES, PlacesView, entry_dense, entry_typed, typed_kind};
use crate::{environment_rows, key_rows, name_rows, site_rows, staged_rows};

/// `this` (`LoadThis`): the activation's receiver defined into the destination, where it holds one.
#[inline(always)]
fn this_row<const D: u8>(s: S) -> Out {
    let mut frame = Places::new::<D, MEM, MEM>(s);
    let load = Operation::LoadThis { destination: DEST };
    let done = matches!(step_value_of::<{ op_kind::LOAD_THIS }, _, _>(&mut StencilStage::from_hole(0), &mut frame, load), Step::Value(_));
    finish(frame, done)
}

/// A local slot read (`LoadLocalPlain`): the slot number is hole 1.
#[inline(always)]
fn load_local_row<const D: u8>(s: S) -> Out {
    let mut frame = Places::new::<D, MEM, MEM>(s);
    let load = Operation::LoadLocalPlain { destination: DEST, slot: hole::<HOLE_A>() as u16 };
    let done = matches!(step_local_of::<{ op_kind::LOAD_LOCAL_PLAIN }, _, _>(&mut StencilStage::from_hole(0), &mut frame, load), Step::Value(_));
    finish(frame, done)
}

/// A local slot write of the first operand (`StoreLocalPlain`): the slot number is hole 2. A hit continues at
/// TAKEN, as every row that has a helper step behind it does (`finish_fast`).
#[inline(always)]
fn store_local_row<const A: u8>(s: S) -> Out {
    let mut frame = Places::new::<MEM, A, MEM>(s);
    let store = Operation::StoreLocalPlain { source: LEFT, slot: hole::<HOLE_B>() as u16, result: None };
    let done = matches!(step_local_of::<{ op_kind::STORE_LOCAL_PLAIN }, _, _>(&mut StencilStage::from_hole(0), &mut frame, store), Step::Value(_));
    finish_fast(frame, done)
}

/// A fast arm's local slot read that leaves into the helper step for a binding in its temporal dead zone:
/// the slot number is hole 1 (`LoadLocalChecked`, not numeric).
#[inline(always)]
fn load_checked_row<const D: u8>(s: S) -> Out {
    let mut frame = Places::new::<D, MEM, MEM>(s);
    let load = Operation::LoadLocalChecked { destination: DEST, slot: hole::<HOLE_A>() as u16, numeric: false };
    let done = matches!(step_local_of::<{ op_kind::LOAD_LOCAL_CHECKED }, _, _>(&mut StencilStage::from_hole(0), &mut frame, load), Step::Value(_));
    finish_fast(frame, done)
}

/// A fast arm's local slot write of the first operand to an initialized binding: the slot number is
/// hole 2 (`StoreLocalChecked`, not initializing, no result).
#[inline(always)]
fn store_checked_row<const A: u8>(s: S) -> Out {
    let mut frame = Places::new::<MEM, A, MEM>(s);
    let store = Operation::StoreLocalChecked { source: LEFT, slot: hole::<HOLE_B>() as u16, result: None, initializing: false };
    let done = matches!(step_local_of::<{ op_kind::STORE_LOCAL_CHECKED }, _, _>(&mut StencilStage::from_hole(0), &mut frame, store), Step::Value(_));
    finish_fast(frame, done)
}

/// A tier guard's row: the guard's step over the row's frame. It continues where the proof holds and
/// leaves with nothing written where it fails.
#[inline(always)]
fn guard_row<const KIND: u16>(s: S, guard: Operation<u64, ()>) -> Out {
    let mut frame = Places::new::<MEM, MEM, MEM>(s);
    let held = matches!(step_of::<KIND, _, _>(&mut StencilStage::from_hole(0), &mut frame, guard), Step::Value(_));
    if held { Out::Next(frame.s) } else { Out::Exit(frame.s) }
}

/// The register at frame offset hole 1 holds exactly the word of hole 2 (`guard::identity`).
#[inline(always)]
fn identity_row(s: S) -> Out {
    let guard = Operation::GuardIdentity { source: word_at(s.frame, hole::<HOLE_A>()), expected: hole64::<HOLE_B>() };
    guard_row::<{ op_kind::GUARD_IDENTITY }>(s, guard)
}

/// A captured declaration's entry test (`base_region::link::entry_rows`): the depth-0 capture at the slot of holes 0
/// (bound) and 1 (byte offset) of the activation's captures (the window's `CONTEXT_BODY` word, written by the call that
/// enters it) holds exactly the word of hole 2, else the entry leaves with nothing written. The slot read takes its two
/// site holes from hole 0 on (`environment::slot_address`), so the expected word is past them.
#[inline(always)]
fn capture_identity_row(s: S) -> Out {
    let captures = context_word(s.frame, CONTEXT_BODY);
    let mut stage = StencilStage::from_hole(0);
    let held = environment::dense_capture(&mut stage, capture_values(captures), capture_len(captures), hole::<0>() as u16, ())
        .is_some_and(|word| word == hole64::<HOLE_B>());
    if held { Out::Next(s) } else { Out::Exit(s) }
}

/// A declared parameter's entry test: the frame slot at hole 0 holds a word whose tag-decided languages
/// meet the declared mask at hole 1 (`guard::declared`); any other word leaves with nothing written.
#[inline(always)]
fn declared_row(s: S) -> Out {
    let guard = Operation::GuardDeclared { source: word_at(s.frame, hole::<0>()), mask: hole64::<HOLE_A>() };
    guard_row::<{ op_kind::GUARD_DECLARED }>(s, guard)
}

/// `GuardClass`: continues where the object at the frame offset in hole 0 is a direct object whose live class cell's class
/// is hole 1 (`guard::class`); any other object leaves with nothing written. The prototype and shape guards test the same
/// class word: a direct object's cell holds its class, which names its shape and prototype link (`class_row` for each).
#[inline(always)]
fn class_row(s: S) -> Out {
    let guard = Operation::GuardClass { source: word_at(s.frame, hole::<0>()), class: hole64::<HOLE_A>() as u32 };
    guard_row::<{ op_kind::GUARD_CLASS }>(s, guard)
}

/// `GuardStores`: continues where the run's dispatch says its stores specialized (`JS_STEP_STORES_SPECIALIZED`), else leaves
/// with nothing written (`slot_store::store_admitted`).
#[inline(always)]
fn stores_row(s: S) -> Out {
    let guard = Operation::GuardStores { specialized: word_at(payload(&s), JS_STEP_STORES_SPECIALIZED) };
    guard_row::<{ op_kind::GUARD_STORES }>(s, guard)
}

/// `DirectArray`: continues where the register at hole 1 holds a direct array.
#[inline(always)]
fn array_row(s: S) -> Out {
    let mut frame = Places::new::<MEM, MEM, MEM>(s);
    let proof = Operation::DirectArray { source: LEFT };
    let done = matches!(step_of::<{ op_kind::DIRECT_ARRAY }, _, _>(&mut StencilStage::from_hole(0), &mut frame, proof), Step::Value(_));
    finish(frame, done)
}

/// The staged element operation over the row's places (`ELEMENT_SITES`): its word, `None` where it
/// left at its miss edge.
#[inline(always)]
fn staged_element<const KIND: u16>(frame: &mut Places, operation: Operation<u64, ()>) -> Option<u64> {
    match step_of::<KIND, _, _>(&mut StencilStage::from_hole(ELEMENT_SITES), frame, operation) {
        Step::Value(word) => Some(word),
        _ => None,
    }
}

/// An element read: the array word is hole 1; an entry row's record offset is hole 1 too. A dense
/// read is the staged `ElementRead` (`key` the register at hole 2); an entry row's read is the same
/// operation through the loop entry's view (`PlacesView` over `entry_dense`).
#[inline(always)]
fn index_row<const ENTRY: bool, const D: u8, const B: u8>(s: S, payload: usize) -> Out {
    if ENTRY {
        let view = entry_dense::<HOLE_A>(payload);
        let mut places = Places::new::<D, MEM, B>(s);
        let read = Operation::ElementRead { base: LEFT, key: Operand::register(RIGHT), destination: DEST };
        let mut frame = PlacesView::new(&mut places, LEFT, view);
        match step_of::<{ op_kind::ELEMENT_READ }, _, _>(&mut StencilStage::from_hole(ELEMENT_SITES), &mut frame, read) {
            Step::Value(word) => deliver::<D>(places.s, word),
            _ => Out::Exit(places.s),
        }
    } else {
        let mut frame = Places::new::<D, MEM, B>(s);
        match staged_element::<{ op_kind::ELEMENT_READ }>(&mut frame, Operation::ElementRead { base: LEFT, key: Operand::register(RIGHT), destination: DEST }) {
            Some(word) => deliver::<D>(frame.s, word),
            None => Out::Exit(frame.s),
        }
    }
}

/// An element store: the array is hole 0, the value hole 1, the index hole 2. A dense store is the
/// staged `ElementWrite`; an entry row's store is the same operation through the loop entry's view (`PlacesView`).
#[inline(always)]
fn store_row<const ENTRY: bool, const A: u8, const B: u8>(s: S, payload: usize) -> Out {
    if ENTRY {
        let view = entry_dense::<0>(payload);
        let mut places = Places::new::<MEM, A, B>(s);
        let write = Operation::ElementWrite { value: LEFT, base: DEST, key: RIGHT, strict: false };
        let mut frame = PlacesView::new(&mut places, DEST, view);
        match step_of::<{ op_kind::ELEMENT_WRITE }, _, _>(&mut StencilStage::from_hole(ELEMENT_SITES), &mut frame, write) {
            Step::Value(_) => Out::Next(places.s),
            _ => Out::Exit(places.s),
        }
    } else {
        let mut frame = Places::new::<MEM, A, B>(s);
        let done = staged_element::<{ op_kind::ELEMENT_WRITE }>(&mut frame, Operation::ElementWrite { value: LEFT, base: DEST, key: RIGHT, strict: false }).is_some();
        finish(frame, done)
    }
}

/// A typed element read: the entry words' offset is hole 1, the Number delivered as `Value::number` makes it
/// (`PlacesView` over the typed view of the kind).
#[inline(always)]
fn typed_index_row<const KIND: u8, const ENTRY: bool, const D: u8, const B: u8>(s: S, payload: usize) -> Out {
    let mut places = Places::new::<D, MEM, B>(s);
    // An entry row's view is the loop entry's; any other proves the array's view (hole 1 its slot) for the kind.
    // A value the proof or the read declines (another kind, an index past the count) takes the
    // generic element row, as the access did before its kind was observed.
    let view = if ENTRY { entry_typed::<KIND>(payload, (HOLE_A, TYPED_INDEX_ROW)) } else { typed_kind::<KIND>(TYPED_INDEX_ROW).and_then(|native| places.proved_typed_view(LEFT, native, false)) };
    let Some(view) = view else { return if ENTRY { Out::Exit(s) } else { index_row::<false, D, B>(s, payload) } };
    let read = Operation::ElementRead { base: LEFT, key: Operand::register(RIGHT), destination: DEST };
    let mut frame = PlacesView::new(&mut places, LEFT, view);
    let word = match step_of::<{ op_kind::ELEMENT_READ }, _, _>(&mut StencilStage::from_hole(ELEMENT_SITES), &mut frame, read) {
        Step::Value(word) => Some(word),
        _ if !ENTRY => return index_row::<false, D, B>(s, payload),
        _ => None,
    };
    // The entry's view is typed, so the Number is the double its bits hold, delivered canonical.
    match word {
        Some(word) => deliver::<D>(places.s, canonical(f64::from_bits(word))),
        None => Out::Exit(places.s),
    }
}

/// A typed element store of the Number operand (hole 1): the entry words' offset is hole 0, the index hole 2
/// (`PlacesView` over the typed view of the kind).
#[inline(always)]
fn typed_store_row<const KIND: u8, const ENTRY: bool, const A: u8, const B: u8>(s: S, payload: usize) -> Out {
    let mut places = Places::new::<MEM, A, B>(s);
    // An entry row's view is the loop entry's; any other proves the array's view (hole 0 its slot) for the kind.
    // A store the proof or the write declines (another kind, bytes another view shares, an index past
    // the count) takes the generic element row, which writes nothing before it decides.
    let view = if ENTRY { entry_typed::<KIND>(payload, (0, TYPED_STORE_ROW)) } else { typed_kind::<KIND>(TYPED_STORE_ROW).and_then(|native| places.proved_typed_view(DEST, native, true)) };
    let Some(view) = view else { return if ENTRY { Out::Exit(s) } else { store_row::<false, A, B>(s, payload) } };
    let write = Operation::ElementWrite { value: LEFT, base: DEST, key: RIGHT, strict: false };
    let mut frame = PlacesView::new(&mut places, DEST, view);
    match step_of::<{ op_kind::ELEMENT_WRITE }, _, _>(&mut StencilStage::from_hole(ELEMENT_SITES), &mut frame, write) {
        Step::Value(_) => Out::Next(places.s),
        _ if !ENTRY => store_row::<false, A, B>(s, payload),
        _ => Out::Exit(places.s),
    }
}

/// A typed array's `length` (`TypedLength`): the count the loop entry's words hold, an int32. The frame
/// reads the entry words at the site hole 1 names (`places_heap.rs`).
#[inline(always)]
fn typed_length_row<const D: u8>(s: S) -> Out {
    let mut frame = Places::new::<D, MEM, MEM>(s);
    let length = Operation::TypedLength { destination: DEST, source: LEFT };
    let done = matches!(step_of::<{ op_kind::TYPED_LENGTH }, _, _>(&mut StencilStage::from_hole(HOLE_A), &mut frame, length), Step::Value(_));
    finish(frame, done)
}

/// A `u32` word of the record at `base + offset`: a cursor's `next` or `limit`.
#[inline(always)]
fn word32_at(base: usize, offset: usize) -> u32 {
    // SAFETY: a layout offset of a live cursor, read whole as `NATIVE_RECORD_CURSOR_LAYOUT` names it.
    unsafe { ((base.wrapping_add(offset)) as *const u32).read_unaligned() }
}

/// Writes a `u32` word of a cursor or record the run owns, before any safepoint.
#[inline(always)]
fn put_word32(base: usize, offset: usize, value: u32) {
    // SAFETY: a layout offset of a cursor or a fresh record of the run, which nothing else reads yet.
    unsafe { ((base.wrapping_add(offset)) as *mut u32).write_unaligned(value) }
}

/// Writes a `u64` word of a fresh record or stub the run owns, before any safepoint.
#[inline(always)]
fn put_word64(base: usize, offset: usize, value: u64) {
    // SAFETY: as `put_word32`.
    unsafe { ((base.wrapping_add(offset)) as *mut u64).write_unaligned(value) }
}

/// An empty allocation of kind `KIND` (`EMPTY_OBJECT_ROW` or `EMPTY_ARRAY_ROW`), with nothing written where it leaves.
#[inline(always)]
fn empty_row<const KIND: u8, const D: u8>(s: S, payload: usize) -> Out {
    if const { KIND == EMPTY_OBJECT_ROW } { empty_object_row::<D>(s, payload) } else { empty_array_row::<D>(s, payload) }
}

/// An empty ordinary object (`EMPTY_OBJECT_ROW`): the staged claim (`bump::claim_empty_object`) over the dispatch head's
/// region 1 cell cursor, class cell base and captured class, run on `StencilStage`. Its answer is the direct object word
/// of the claimed class cell's granule, delivered into the destination (hole 0); where the cursor is spent or no class is
/// captured it leaves with nothing written, so the helper makes the object. Nothing here can panic.
#[inline(always)]
fn empty_object_row<const D: u8>(s: S, payload: usize) -> Out {
    // A tagged reference cannot land in an int or double place: such a destination leaves before anything is written.
    if D == MEMINT || (PIN..=LAST_INT_PIN).contains(&D) || D >= PINF {
        return Out::Exit(s);
    }
    let cell = ClassCell {
        cursor: word_at(payload, JS_STEP_EMPTY_OBJECT_CURSOR),
        cells: word_at(payload, JS_STEP_CLASS_BASE),
        class: word_at(payload, JS_STEP_EMPTY_OBJECT_CLASS),
    };
    match claim_empty_object(&mut StencilStage::from_hole(0), cell, ()) {
        Some(word) => deliver::<D>(s, word),
        None => Out::Exit(s),
    }
}

/// An empty array (`EMPTY_ARRAY_ROW`): while the array cursor has a record left and its template is captured, the record of
/// the bumped id gets the template's two words, and the destination takes the direct reference of that id. Arrays have no
/// stub. Any other case leaves with nothing written.
#[inline(always)]
fn empty_array_row<const D: u8>(s: S, payload: usize) -> Out {
    if D == MEMINT || (PIN..=LAST_INT_PIN).contains(&D) || D >= PINF {
        return Out::Exit(s);
    }
    let cursor = word_at(payload, JS_STEP_ARRAY_CURSOR) as usize;
    let records = word_at(payload, JS_STEP_ARRAY_RECORD_BASE) as usize;
    let layout = word_at(payload, JS_STEP_EMPTY_ARRAY_LAYOUT);
    if cursor == 0 || records == 0 || layout == 0 {
        return Out::Exit(s);
    }
    let next = word32_at(cursor, NATIVE_RECORD_CURSOR_LAYOUT.next_offset);
    if next >= word32_at(cursor, NATIVE_RECORD_CURSOR_LAYOUT.limit_offset) {
        return Out::Exit(s);
    }
    let words = word_at(payload, JS_STEP_EMPTY_ARRAY_PAYLOAD);
    put_word32(cursor, NATIVE_RECORD_CURSOR_LAYOUT.next_offset, next.wrapping_add(1));
    let record = records.wrapping_add((next as usize).wrapping_mul(NATIVE_ARRAY_LAYOUT.record_size));
    put_word64(record, NATIVE_OBJECT_LAYOUT.payload_offset, words);
    put_word32(record, NATIVE_OBJECT_LAYOUT.layout_offset, layout as u32);
    deliver::<D>(s, DIRECT_ARRAY_TAG | u64::from(next))
}

/// `ArrayLength`: the `length` of the direct array at hole 1, defined into the destination. A string's
/// length (`string_length::length_of`) is the fallback where the array operation does not hold.
#[inline(always)]
fn length_row<const D: u8>(s: S, payload: usize) -> Out {
    let mut frame = Places::new::<D, MEM, MEM>(s);
    let length = Operation::ArrayLength { destination: DEST, source: LEFT };
    if matches!(step_of::<{ op_kind::ARRAY_LENGTH }, _, _>(&mut StencilStage::from_hole(0), &mut frame, length), Step::Value(_)) {
        return finish(frame, true);
    }
    let mut stage = StencilStage::from_hole(0);
    let base = frame.read(&mut stage, LEFT);
    let records = (word_at(payload, JS_STEP_ARRAY_RECORD_BASE), word_at(payload, JS_STEP_STRING_RECORD_BASE));
    deliver::<D>(frame.s, need!(frame.s, string_length::length_of(&mut stage, records, base, ())))
}

/// `ArrayAppend`: the Number operand (hole 1) appended to the array at hole 0 where its backing is writable
/// and its capacity word has room (`Places::heap_array_append`); a hit continues at TAKEN.
#[inline(always)]
fn append_row<const A: u8>(s: S) -> Out {
    let mut frame = Places::new::<MEM, A, MEM>(s);
    let append = Operation::ArrayAppend { array: DEST, value: LEFT };
    let done = matches!(step_of::<{ op_kind::ARRAY_APPEND }, _, _>(&mut StencilStage::from_hole(ELEMENT_SITES), &mut frame, append), Step::Value(_));
    finish_fast(frame, done)
}

/// `charCodeAt` of the flat string in the frame slot at hole 1, at the int32 index at hole 2 where it is in range: the code unit
/// (`code_units::code_unit`), an int32. Any other receiver, form, index or unit leaves with nothing written.
#[inline(always)]
fn char_code_at_row<const D: u8, const B: u8>(s: S, payload: usize) -> Out {
    let index = need!(s, index_of::<B, HOLE_B>(&s));
    let mut frame = Places::new::<D, MEM, B>(s);
    let mut stage = StencilStage::from_hole(0);
    let base = frame.read(&mut stage, LEFT);
    let string = need!(frame.s, string_length::flat_string(&mut stage, word_at(payload, JS_STEP_STRING_RECORD_BASE), base, ()));
    if index < 0 || index as u64 >= string.length {
        return Out::Exit(frame.s);
    }
    let unit = code_units::code_unit(&mut stage, string.wide != 0, string.units, index as u64);
    deliver::<D>(frame.s, int_word(unit as u32 as i32))
}

/// `parseInt` of the flat string in the frame slot at hole 1, in radix 10: the int32 `parse_int::decimal_int32_row` answers over its
/// code units. Any other text leaves with nothing written.
#[inline(always)]
fn parse_int_row<const D: u8>(s: S, payload: usize) -> Out {
    let mut frame = Places::new::<D, MEM, MEM>(s);
    let mut stage = StencilStage::from_hole(0);
    let base = frame.read(&mut stage, LEFT);
    let string = need!(frame.s, string_length::flat_string(&mut stage, word_at(payload, JS_STEP_STRING_RECORD_BASE), base, ()));
    let (units, wide) = (string.units, string.wide != 0);
    let (parsed, value) = parse_int::decimal_int32_row(&mut stage, wide, units, string.length);
    let value = need!(frame.s, parsed.then_some(value as i32));
    deliver::<D>(frame.s, int_word(value))
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
    if const { KIND == ARRAY_ROW } {
        array_row(s)
    } else if const { KIND == LENGTH_ROW } {
        length_row::<D>(s, payload)
    } else if const { KIND == INDEX_ROW } {
        index_row::<ENTRY, D, B>(s, payload)
    } else if const { (TYPED_INDEX_ROW <= KIND && KIND <= TYPED_INDEX_LAST) } {
        typed_index_row::<KIND, ENTRY, D, B>(s, payload)
    } else if const { (TYPED_STORE_ROW <= KIND && KIND <= TYPED_STORE_LAST) } {
        typed_store_row::<KIND, ENTRY, A, B>(s, payload)
    } else if const { KIND == TYPED_LENGTH_ROW } {
        typed_length_row::<D>(s)
    } else if const { KIND == APPEND_ROW } {
        append_row::<A>(s)
    } else {
        store_row::<ENTRY, A, B>(s, payload)
    }
}

#[inline(always)]
pub(crate) fn run<const KIND: u8, const ENTRY: bool, const D: u8, const A: u8, const B: u8>(s: S) -> Out {
    // The staged rows (`staged_rows.rs`) are fast arms the planner lays a helper step behind: a hit hops past it.
    if const { KIND >= STAGED_FIRST && KIND <= STAGED_LAST } {
        return hop(staged_rows::run::<KIND, D, A, B>(s));
    }
    let payload = payload(&s);
    if const { KIND == CAPTURE_ROW || KIND == CAPTURE_SET_ROW || KIND == ENV_ROW || KIND == ENV_SET_ROW || KIND == ENV_DEFINE_ROW || KIND == GLOBAL_READ_ROW || KIND == GLOBAL_STORE_ROW || KIND == LEXICAL_READ_ROW || KIND == LEXICAL_WRITE_ROW || KIND == CALLEE_CAPTURE_ROW } {
        environment_rows::run::<KIND, ENTRY, D, A>(s, payload)
    } else if const { KIND == FIELD_ROW || KIND == SITE_ROW || KIND == SITE_STORE_ROW || KIND == THIS_SITE_ROW || KIND == THIS_STORE_ROW || KIND == HELD_ROW || KIND == LIVE_ROW || KIND == REALM_ROW || KIND == CONSTANT_ROW } {
        site_rows::run::<KIND, ENTRY, D, A>(s, payload)
    } else if const { KIND == OBJECT_ROW || KIND == KEY_ROW || KIND == TYPEOF_INDEX_ROW || KIND == TYPEOF_STRING_ROW || KIND == TYPEOF_IS_ROW || KIND == TYPEOF_ISNOT_ROW || KIND == FOR_IN_ROW } {
        key_rows::run::<KIND>(s, payload)
    } else if const { KIND == ADD_CLASS_ROW } {
        name_rows::run::<A>(s, payload)
    } else if const { KIND == IDENTITY_ROW || KIND == THIS_ROW || KIND == LOAD_LOCAL_ROW || KIND == STORE_LOCAL_ROW || KIND == LOAD_CHECKED_ROW || KIND == STORE_CHECKED_ROW } {
        register_row::<KIND, D, A>(s)
    } else if const { KIND == IC_GET_ROW || KIND == IC_GET_HOLDER_ROW || KIND == IC_GET_CLASS_ROW || KIND == IC_GET_HELD_CLASS_ROW || KIND == IC_SET_CLASS_ROW || KIND == IC_SET_ROW || KIND == IC_SET_INT_ROW || KIND == IC_PROOF_ROW || KIND == IC_KNOWN_GET_ROW || KIND == IC_KNOWN_SET_ROW || KIND == IC_KNOWN_SET_INT_ROW || KIND == IC_KNOWN_SET_ADMITTED_ROW || KIND == IC_KNOWN_SET_INT_ADMITTED_ROW || KIND == IC_KNOWN_GET_FLAT_ROW || KIND == IC_KNOWN_SET_FLAT_ROW || KIND == IC_KNOWN_SET_INT_FLAT_ROW || KIND == IC_KNOWN_SET_FLAT_ADMITTED_ROW || KIND == IC_KNOWN_SET_INT_FLAT_ADMITTED_ROW } {
        ic_rows::run::<KIND, ENTRY, D, A>(s)
    } else if const { KIND == DECLARED_ROW } {
        declared_row(s)
    } else if const { KIND == CAPTURE_IDENTITY_ROW } {
        capture_identity_row(s)
    } else if const { KIND == CLASS_ROW } {
        class_row(s)
    } else if const { KIND == STORES_ROW } {
        stores_row(s)
    } else if const { KIND == CHAR_CODE_AT_ROW } {
        char_code_at_row::<D, B>(s, payload)
    } else if const { KIND == PARSE_INT_ROW } {
        parse_int_row::<D>(s, payload)
    } else if const { KIND == EMPTY_OBJECT_ROW || KIND == EMPTY_ARRAY_ROW } {
        empty_row::<KIND, D>(s, payload)
    } else {
        element_row::<KIND, ENTRY, D, A, B>(s, payload)
    }
}
