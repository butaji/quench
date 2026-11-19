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
//! receiver (`THIS_SITE_ROW`, `THIS_STORE_ROW`) take it from the payload, so their hole 0 is the
//! destination alone (a read) or unused (a store).
//! The global object's slot rows (`GLOBAL_READ_ROW`, `GLOBAL_STORE_ROW`) take its record id in hole 1
//! (the store: hole 0) and the slot's byte offset in hole 2; a constant row takes its word in hole 1
//! and the realm proof (`REALM_ROW`) a VM field offset in hole 1 and the global object's word in hole 2.
//! Every failed proof leaves through exit 0 with nothing written, so the interpreter runs the
//! instruction.

use crate::bytecode::Operand;
use crate::layout::{CALL_CONTEXT_ADAPTER_PAYLOAD, JS_STEP_ARRAY_RECORD_BASE, JS_STEP_OBJECT_RECORD_BASE, JS_STEP_STRING_RECORD_BASE};
use crate::ic_rows;
use crate::numeric::{HOLE_A, HOLE_B, Out, S, deliver, index_of};
use crate::words::{hole, hole64, int_word};
use crate::places::{DEST, LEFT, RIGHT, Places};
use crate::stage::StencilStage;
use crate::staged::{Operation, Stage, Step, step, step_local, step_value, string_length};
use crate::codes::{ADD_ARENA_ROW, ADD_EITHER_ROW, ADD_PACKED_ROW, APPEND_ROW, ARRAY_ROW, CALLEE_CAPTURE_ROW, CAPTURE_ROW, CAPTURE_SET_ROW, CHAR_CODE_AT_ROW, CONSTANT_ROW, DECLARED_ROW, ENV_DEFINE_ROW, HELD_LINK_AT, PROTOTYPE_ROW, ENV_ROW, ENV_SET_ROW, FIELD_ROW, FOR_IN_ROW, GLOBAL_READ_ROW, GLOBAL_STORE_ROW, HELD_ROW, IC_GET_HOLDER_ROW, IC_GET_ROW, IC_KNOWN_GET_ROW, IC_KNOWN_SET_INT_ROW, IC_KNOWN_SET_ROW, IC_PROOF_ROW, IC_SET_INT_ROW, IC_SET_ROW, IDENTITY_ROW, INDEX_ROW, INSTANCEOF_ROW, KEY_ROW, LENGTH_ROW, LEXICAL_READ_ROW, LEXICAL_WRITE_ROW, LIVE_ROW, LOAD_CHECKED_ROW, LOAD_LOCAL_ROW, MEM, NAME_ARENA_ROW, NAME_EITHER_ROW, OBJECT_ROW, PARSE_INT_ROW, REALM_ROW, SITE_ROW, SITE_STORE_ROW, STAGED_FIRST, STAGED_LAST, STORE_CHECKED_ROW, STORE_LOCAL_ROW, THIS_ADD_ARENA_ROW, THIS_ADD_EITHER_ROW, THIS_ADD_PACKED_ROW, THIS_ROW, THIS_SITE_ROW, THIS_STORE_ROW, TYPED_INDEX_LAST, TYPED_INDEX_ROW, TYPED_LENGTH_ROW, TYPED_STORE_LAST, TYPED_STORE_ROW, TYPEOF_INDEX_ROW, TYPEOF_ISNOT_ROW, TYPEOF_IS_ROW, TYPEOF_STRING_ROW};
use crate::staged::{code_units, parse_int};
use crate::places_view::{self, ELEMENT_SITES, entry_dense, entry_typed};
use crate::{environment_rows, key_rows, name_rows, site_rows, staged_rows};

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

#[inline(always)]
pub(crate) fn stage_site(stage: &mut StencilStage) -> u64 {
    stage.site(0)
}

/// Whether the row's staged work was done and fit its place: it continues, else it leaves.
#[inline(always)]
fn finish<const D: u8, const A: u8, const B: u8>(frame: Places<D, A, B>, done: bool) -> Out {
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

/// `this` (`LoadThis`): the activation's receiver defined into the destination, where it holds one.
#[inline(always)]
fn this_row<const D: u8>(s: S) -> Out {
    let mut frame = Places::<D, MEM, MEM>::new(s);
    let load = Operation::LoadThis { destination: DEST };
    let done = matches!(step_value(&mut StencilStage::from_hole(0), &mut frame, load), Step::Value(_));
    finish(frame, done)
}

/// A local slot read (`LoadLocalPlain`): the slot number is hole 1.
#[inline(always)]
fn load_local_row<const D: u8>(s: S) -> Out {
    let mut frame = Places::<D, MEM, MEM>::new(s);
    let load = Operation::LoadLocalPlain { destination: DEST, slot: hole::<HOLE_A>() as u16 };
    let done = matches!(step_local(&mut StencilStage::from_hole(0), &mut frame, load), Step::Value(_));
    finish(frame, done)
}

/// A local slot write of the first operand (`StoreLocalPlain`): the slot number is hole 2. A hit continues at
/// TAKEN, as every row that has a helper step behind it does (`finish_fast`).
#[inline(always)]
fn store_local_row<const A: u8>(s: S) -> Out {
    let mut frame = Places::<MEM, A, MEM>::new(s);
    let store = Operation::StoreLocalPlain { source: LEFT, slot: hole::<HOLE_B>() as u16, result: None };
    let done = matches!(step_local(&mut StencilStage::from_hole(0), &mut frame, store), Step::Value(_));
    finish_fast(frame, done)
}

/// A fast arm's local slot read that leaves into the helper step for a binding in its temporal dead zone:
/// the slot number is hole 1 (`LoadLocalChecked`, not numeric).
#[inline(always)]
fn load_checked_row<const D: u8>(s: S) -> Out {
    let mut frame = Places::<D, MEM, MEM>::new(s);
    let load = Operation::LoadLocalChecked { destination: DEST, slot: hole::<HOLE_A>() as u16, numeric: false };
    let done = matches!(step_local(&mut StencilStage::from_hole(0), &mut frame, load), Step::Value(_));
    finish_fast(frame, done)
}

/// A fast arm's local slot write of the first operand to an initialized binding: the slot number is
/// hole 2 (`StoreLocalChecked`, not initializing, no result).
#[inline(always)]
fn store_checked_row<const A: u8>(s: S) -> Out {
    let mut frame = Places::<MEM, A, MEM>::new(s);
    let store = Operation::StoreLocalChecked { source: LEFT, slot: hole::<HOLE_B>() as u16, result: None, initializing: false };
    let done = matches!(step_local(&mut StencilStage::from_hole(0), &mut frame, store), Step::Value(_));
    finish_fast(frame, done)
}

/// A tier guard's row: the guard's step over the row's frame. It continues where the proof holds and
/// leaves with nothing written where it fails.
#[inline(always)]
fn guard_row(s: S, guard: Operation<u64, ()>) -> Out {
    let mut frame = Places::<MEM, MEM, MEM>::new(s);
    let held = matches!(step(&mut StencilStage::from_hole(0), &mut frame, guard), Step::Value(_));
    if held { Out::Next(frame.s) } else { Out::Exit(frame.s) }
}

/// The register at frame offset hole 1 holds exactly the word of hole 2 (`guard::identity`).
#[inline(always)]
fn identity_row(s: S) -> Out {
    let guard = Operation::GuardIdentity { source: word_at(s.frame, hole::<HOLE_A>()), expected: hole64::<HOLE_B>() };
    guard_row(s, guard)
}

/// A declared parameter's entry test: the frame slot at hole 0 holds a word whose tag-decided languages
/// meet the declared mask at hole 1 (`guard::declared`); any other word leaves with nothing written.
#[inline(always)]
fn declared_row(s: S) -> Out {
    let guard = Operation::GuardDeclared { source: word_at(s.frame, hole::<0>()), mask: hole64::<HOLE_A>() };
    guard_row(s, guard)
}

/// `GuardPrototype`: continues where the object at the frame offset in hole 0 is a direct object of the exact shape in
/// hole 1's low half whose prototype link is its high half (`HELD_LINK_AT`), its record read from the object record table
/// (`guard::prototype`); any other object leaves with nothing written.
#[inline(always)]
fn prototype_row(s: S) -> Out {
    let receiver = hole64::<HOLE_A>();
    let guard = Operation::GuardPrototype {
        source: word_at(s.frame, hole::<0>()),
        records: word_at(payload(&s), JS_STEP_OBJECT_RECORD_BASE),
        shape: receiver & u64::from(u32::MAX),
        link: receiver >> HELD_LINK_AT,
    };
    guard_row(s, guard)
}

/// `DirectArray`: continues where the register at hole 1 holds a direct array.
#[inline(always)]
fn array_row(s: S) -> Out {
    let mut frame = Places::<MEM, MEM, MEM>::new(s);
    let proof = Operation::DirectArray { source: LEFT };
    let done = matches!(step(&mut StencilStage::from_hole(0), &mut frame, proof), Step::Value(_));
    finish(frame, done)
}

/// The staged element operation over the row's places (`ELEMENT_SITES`): its word, `None` where it
/// left at its miss edge.
#[inline(always)]
fn staged_element<const D: u8, const A: u8, const B: u8>(frame: &mut Places<D, A, B>, operation: Operation<u64, ()>) -> Option<u64> {
    match step(&mut StencilStage::from_hole(ELEMENT_SITES), frame, operation) {
        Step::Value(word) => Some(word),
        _ => None,
    }
}

/// An element read: the array word is hole 1; an entry row's record offset is hole 1 too. A dense
/// read is the staged `ElementRead` (`key` the register at hole 2); an entry row's read is the same
/// operation through the loop entry's view (`places_view::entry_read`).
#[inline(always)]
fn index_row<const ENTRY: bool, const D: u8, const B: u8>(s: S, payload: usize) -> Out {
    let index = need!(s, index_of::<B, HOLE_B>(&s));
    if !ENTRY {
        // A wide, double or narrow backing; the upper-word table a narrow element decodes through is hole 4.
        let mut stage = StencilStage::from_hole(ELEMENT_SITES);
        let (records, base) = (word_at(payload, JS_STEP_ARRAY_RECORD_BASE), array_word::<HOLE_A>(&s));
        let word = need!(s, elements::record_element(&mut stage, records, base, u64::from(index as u32), hole64::<{ crate::words::LAST_HOLE }>(), ()));
        return deliver::<D>(s, word);
    }
    let (values, length, _) = need!(s, backing::<ENTRY, HOLE_A, false>(&s, payload));
    let mut stage = StencilStage::from_hole(ELEMENT_SITES);
    let word = need!(s, elements::read_element(&mut stage, elements::DenseView { values, length, packed: false, within: false, double: None }, u64::from(index as u32), ()));
    deliver::<D>(s, word)
}

/// An element store: the array is hole 0, the value hole 1, the index hole 2. A dense store is the
/// staged `ElementWrite`; an entry row's store is the same operation through the loop entry's view.
#[inline(always)]
fn store_row<const ENTRY: bool, const A: u8, const B: u8>(s: S, payload: usize) -> Out {
    let index = need!(s, index_of::<B, HOLE_B>(&s));
    let word = need!(s, number_word::<A, HOLE_A>(&s));
    let (values, length, _) = need!(s, backing::<ENTRY, 0, true>(&s, payload));
    let mut stage = StencilStage::from_hole(ELEMENT_SITES);
    need!(s, elements::replace_present(&mut stage, elements::DenseView { values, length, packed: false, within: false, double: None }, u64::from(index as u32), word, ()));
    Out::Next(s)
}

/// A typed element read: the entry words' offset is hole 1, the Number delivered as `Value::number` makes it
/// (`places_view::entry_read` over the typed view of the kind).
#[inline(always)]
fn typed_index_row<const KIND: u8, const D: u8, const B: u8>(s: S, payload: usize) -> Out {
    let view = need!(s, entry_typed::<KIND>(payload, (HOLE_A, TYPED_INDEX_ROW)));
    places_view::entry_read::<D, B>(s, view)
}

/// A typed element store of the Number operand (hole 1): the entry words' offset is hole 0, the index hole 2
/// (`places_view::entry_write` over the typed view of the kind).
#[inline(always)]
fn typed_store_row<const KIND: u8, const A: u8, const B: u8>(s: S, payload: usize) -> Out {
    let view = need!(s, entry_typed::<KIND>(payload, (0, TYPED_STORE_ROW)));
    places_view::entry_write::<A, B>(s, view)
}

/// A typed array's `length` (`TypedLength`): the count the loop entry's words hold, an int32. The frame
/// reads the entry words at the site hole 1 names (`places_heap.rs`).
#[inline(always)]
fn typed_length_row<const D: u8>(s: S) -> Out {
    let mut frame = Places::<D, MEM, MEM>::new(s);
    let length = Operation::TypedLength { destination: DEST, source: LEFT };
    let done = matches!(step(&mut StencilStage::from_hole(HOLE_A), &mut frame, length), Step::Value(_));
    finish(frame, done)
}

/// `ArrayLength`: the `length` of the direct array at hole 1, defined into the destination. A string's
/// length (`string_length::length_of`) is the fallback where the array operation does not hold.
#[inline(always)]
fn length_row<const D: u8>(s: S, payload: usize) -> Out {
    let mut frame = Places::<D, MEM, MEM>::new(s);
    let length = Operation::ArrayLength { destination: DEST, source: LEFT };
    if matches!(step(&mut StencilStage::from_hole(0), &mut frame, length), Step::Value(_)) {
        return finish(frame, true);
    }
    let s = frame.s;
    let mut stage = StencilStage::from_hole(0);
    let base = array_word::<HOLE_A>(&s);
    let records = (word_at(payload, JS_STEP_ARRAY_RECORD_BASE), word_at(payload, JS_STEP_STRING_RECORD_BASE));
    deliver::<D>(s, need!(s, string_length::length_of(&mut stage, records, base, ())))
}

/// `ArrayAppend`: the Number operand (hole 1) appended to the array at hole 0 where its backing is writable
/// and its capacity word has room (`Places::heap_array_append`); a hit continues at TAKEN.
#[inline(always)]
fn append_row<const A: u8>(s: S) -> Out {
    let mut frame = Places::<MEM, A, MEM>::new(s);
    let append = Operation::ArrayAppend { array: DEST, value: LEFT };
    let done = matches!(step(&mut StencilStage::from_hole(ELEMENT_SITES), &mut frame, append), Step::Value(_));
    finish_fast(frame, done)
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
        TYPED_LENGTH_ROW => typed_length_row::<D>(s),
        APPEND_ROW => append_row::<A>(s),
        _ => store_row::<ENTRY, A, B>(s, payload),
    }
}

#[inline(always)]
pub(crate) fn run<const KIND: u8, const ENTRY: bool, const D: u8, const A: u8, const B: u8>(s: S) -> Out {
    // The staged rows (`staged_rows.rs`) are fast arms the planner lays a helper step behind: a hit hops past it.
    if KIND >= STAGED_FIRST && KIND <= STAGED_LAST {
        return hop(staged_rows::run::<KIND, D, A, B>(s));
    }
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
        PROTOTYPE_ROW => prototype_row(s),
        CHAR_CODE_AT_ROW => char_code_at_row::<D, B>(s, payload),
        PARSE_INT_ROW => parse_int_row::<D>(s, payload),
        _ => element_row::<KIND, ENTRY, D, A, B>(s, payload),
    }
}
