//! The heap rows: a captured value, a dense array element read or replaced, an entry-record
//! field, an array proof. Each is the shared staged row (`staged/{environment,elements,entry,
//! value}.rs`) run on `StencilStage`, so the layout reads have one definition across the
//! interpreter, the specializers and the stencils.
//!
//! Holes: 0 is the destination frame offset (the array's, for a store), 1 and 2 the operands'
//! (a field row's entry-record byte offset in 1); a capture's two per-site operands (the slot,
//! and the slot's byte offset) are holes 1 and 2. Every failed proof leaves through exit 0 with
//! nothing written, so the interpreter runs the instruction.

use crate::layout::{CALL_CONTEXT_ADAPTER_PAYLOAD, JS_STEP_ARRAY_RECORD_BASE, JS_STEP_CAPTURE_LEN, JS_STEP_CAPTURE_VALUES, JS_STEP_LOOP_ENTRY};
use crate::numeric::{Out, S, deliver, index_of, number_word};
use crate::stage::StencilStage;
use crate::staged::{elements, entry, environment, value};

pub(crate) const CAPTURE: u8 = 0;
pub(crate) const INDEX: u8 = 1;
pub(crate) const STORE: u8 = 2;
pub(crate) const FIELD: u8 = 3;
pub(crate) const ARRAY: u8 = 4;

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
fn word_at(base: usize, offset: usize) -> u64 {
    // SAFETY: a layout offset of a live record, as every staged load.
    unsafe { ((base + offset) as *const u64).read() }
}

/// The activation's dispatch payload: the context's adapter pointer.
#[inline(always)]
fn payload(s: &S) -> usize {
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

#[inline(always)]
pub(crate) fn run<const KIND: u8, const ENTRY: bool, const D: u8, const A: u8, const B: u8>(s: S) -> Out {
    let payload = payload(&s);
    match KIND {
        CAPTURE => {
            let mut stage = StencilStage::from_hole(1);
            let values = word_at(payload, JS_STEP_CAPTURE_VALUES);
            let len = word_at(payload, JS_STEP_CAPTURE_LEN);
            let word = need!(s, environment::dense_capture(&mut stage, values, len, 0, ()));
            deliver::<D>(s, word)
        }
        FIELD => {
            let mut stage = StencilStage::from_hole(1);
            let record = word_at(payload, JS_STEP_LOOP_ENTRY);
            let offset = stage_site(&mut stage);
            deliver::<D>(s, entry::entry_word(&mut stage, record, offset))
        }
        ARRAY => {
            let mut stage = StencilStage::from_hole(0);
            let base = array_word::<1>(&s);
            if value::is_direct_array(&mut stage, base) { Out::Next(s) } else { Out::Exit(s) }
        }
        INDEX => {
            let index = need!(s, index_of::<B, 2>(&s));
            // The array word is hole 1; an entry row's record offset is hole 1 too.
            let (values, length, _) = need!(s, backing::<ENTRY, 1, false>(&s, payload));
            let mut stage = StencilStage::from_hole(3);
            let (_, word) = need!(s, elements::dense_element(&mut stage, values, length, u64::from(index as u32), ()));
            deliver::<D>(s, word)
        }
        _ => {
            // STORE: the array is hole 0, the value hole 1, the index hole 2.
            let index = need!(s, index_of::<B, 2>(&s));
            let word = need!(s, number_word::<A, 1>(&s));
            let (values, length, _) = need!(s, backing::<ENTRY, 0, true>(&s, payload));
            let mut stage = StencilStage::from_hole(3);
            let (address, old) = need!(s, elements::dense_element(&mut stage, values, length, u64::from(index as u32), ()));
            elements::replace_element(&mut stage, values, address, old, word);
            Out::Next(s)
        }
    }
}
