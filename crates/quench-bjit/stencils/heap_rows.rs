//! The heap rows: a captured value, a dense array element read or replaced, an entry-record
//! field, an array proof. Each is the shared staged row (`staged/{environment,elements,entry,
//! value}.rs`) run on `StencilStage`, so the layout reads have one definition across the
//! interpreter, the specializers and the stencils.
//!
//! Holes: 0 is the destination frame offset (the array's, for a store), 1 and 2 the operands'
//! (a field row's entry-record byte offset in 1); a capture's two per-site operands (the slot,
//! and the slot's byte offset) are holes 1 and 2. A site field read (`SITE_ROW`) packs the
//! object register's frame offset above the destination's in hole 0, takes the process's narrow
//! upper-word table as hole 1 and its cache entry's byte offset as hole 2. Every failed proof
//! leaves through exit 0 with nothing written, so the interpreter runs the instruction.

use crate::layout::{
    CALL_CONTEXT_ADAPTER_PAYLOAD, JS_STEP_ARRAY_RECORD_BASE, JS_STEP_CAPTURE_LEN, JS_STEP_CAPTURE_VALUES, JS_STEP_FIELD_ENTRIES, JS_STEP_LOOP_ENTRY,
    JS_STEP_OBJECT_RECORD_BASE, JS_STEP_PROPERTY_BUFFER, PROPERTY_BUFFER_BASE,
};
use crate::numeric::{HOLE_A, HOLE_B, Out, S, deliver, hole, hole64, index_of, number_word};
use crate::places::{DEST, LEFT, Places};
use crate::stage::StencilStage;
use crate::staged::{Step, elements, entry, environment, load_this, read_local, value, write_local};

pub(crate) use crate::codes::{ARRAY_ROW, CAPTURE_ROW, FIELD_ROW, INDEX_ROW, LOAD_LOCAL_ROW, MEM, SITE_OBJECT_AT, SITE_ROW, STORE_LOCAL_ROW, STORE_ROW, THIS_ROW};

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
fn stage_site(stage: &mut StencilStage) -> u64 {
    use crate::staged::Stage;
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

#[inline(always)]
fn field_row<const D: u8>(s: S, payload: usize) -> Out {
    let mut stage = StencilStage::from_hole(HOLE_A);
    let record = word_at(payload, JS_STEP_LOOP_ENTRY);
    let offset = stage_site(&mut stage);
    deliver::<D>(s, entry::entry_word(&mut stage, record, offset))
}

/// A field read through the site's cache entry (`entry::site_read`): the object is the register
/// whose frame offset hole 0 carries above the destination's.
#[inline(always)]
fn site_row<const D: u8>(s: S, payload: usize) -> Out {
    let object = word_at(s.frame, (hole64::<0>() >> SITE_OBJECT_AT) as usize);
    let header = word_at(payload, JS_STEP_PROPERTY_BUFFER) as usize;
    let mut stage = StencilStage::with_values(HOLE_B, word_at(header, PROPERTY_BUFFER_BASE));
    let (entries, records) = (word_at(payload, JS_STEP_FIELD_ENTRIES), word_at(payload, JS_STEP_OBJECT_RECORD_BASE));
    let offset = stage_site(&mut stage);
    deliver::<D>(s, need!(s, entry::site_read(&mut stage, (entries, records), object, offset, ())))
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
        FIELD_ROW => field_row::<D>(s, payload),
        SITE_ROW => site_row::<D>(s, payload),
        ARRAY_ROW => array_row(s),
        THIS_ROW => this_row::<D>(s),
        LOAD_LOCAL_ROW => load_local_row::<D>(s),
        STORE_LOCAL_ROW => store_local_row::<A>(s),
        INDEX_ROW => index_row::<ENTRY, D, B>(s, payload),
        _ => store_row::<ENTRY, A, B>(s, payload),
    }
}
