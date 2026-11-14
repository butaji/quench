//! The site rows: a field read or store through the site's cache entry, a prototype-held field,
//! a code dependency's liveness and the realm proof, each the staged entry rows
//! (`staged/entry.rs`) run on `StencilStage`.

use crate::layout::{JS_STEP_FIELD_ENTRIES, JS_STEP_LOOP_ENTRY, JS_STEP_OBJECT_RECORD_BASE, JS_STEP_PROPERTY_BUFFER, JS_STEP_RECEIVER, JS_STEP_VM, PROPERTY_BUFFER_BASE};
use crate::numeric::{HOLE_A, HOLE_B, Out, S, deliver};
use crate::words::{hole, hole64};
use crate::places::tagged_place;
use crate::stage::StencilStage;
use crate::staged::{Frame, Stage, entry};
use crate::codes::{CHAIN_SECOND_AT, FIELD_ROW, SITE_OBJECT_AT, SITE_ROW, SITE_STORE_ROW, THIS_SITE_ROW, THIS_STORE_ROW, HELD_LINK_AT, HELD_ROW, LIVE_ROW, REALM_ROW};
use crate::heap_rows::{stage_site, word_at};

#[inline(always)]
fn field_row<const D: u8>(s: S, payload: usize) -> Out {
    let mut stage = StencilStage::from_hole(HOLE_A);
    let record = word_at(payload, JS_STEP_LOOP_ENTRY);
    let offset = stage_site(&mut stage);
    deliver::<D>(s, entry::entry_word(&mut stage, record, offset))
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

/// A prototype-held field folded to its value (hole 2): the object, the activation's receiver for an `ENTRY` row and else the register the site names, is a direct object of the exact shape in
/// hole 1's low half whose prototype link is its high half (`entry::held_receiver`), else the row leaves.
#[inline(always)]
fn held_row<const D: u8, const ENTRY: bool>(s: S, payload: usize) -> Out {
    let object = if ENTRY { word_at(payload, JS_STEP_RECEIVER) } else { register_object(&s) };
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

/// Continues while the active realm's global object (the word at hole 1's byte offset in the VM the
/// payload names) is the one in hole 2, which the code folded facts of (`Vm::realm_globals_offset`);
/// else the taken successor, the entry's failure.
#[inline(always)]
fn realm_row(s: S, payload: usize) -> Out {
    if word_at(word_at(payload, JS_STEP_VM) as usize, hole::<HOLE_A>()) == hole64::<HOLE_B>() { Out::Next(s) } else { Out::Taken(s) }
}

/// The destination takes the word of hole 1.
#[inline(always)]
fn constant_row<const D: u8>(s: S) -> Out {
    deliver::<D>(s, hole64::<HOLE_A>())
}

/// The site row of kind `KIND`.
#[inline(always)]
pub(crate) fn run<const KIND: u8, const ENTRY: bool, const D: u8, const A: u8>(s: S, payload: usize) -> Out {
    match KIND {
        FIELD_ROW => field_row::<D>(s, payload),
        SITE_ROW => site_row::<D, ENTRY>(s, payload, register_object(&s)),
        SITE_STORE_ROW => site_store_row::<A>(s, payload, word_at(s.frame, hole::<0>())),
        THIS_SITE_ROW => site_row::<D, ENTRY>(s, payload, word_at(payload, JS_STEP_RECEIVER)),
        THIS_STORE_ROW => site_store_row::<A>(s, payload, word_at(payload, JS_STEP_RECEIVER)),
        HELD_ROW => held_row::<D, ENTRY>(s, payload),
        LIVE_ROW => live_row(s),
        REALM_ROW => realm_row(s, payload),
        _ => constant_row::<D>(s),
    }
}
