//! The site rows: a field read or store through the site's cache entry, a prototype-held field,
//! a code dependency's liveness and the realm proof, each the staged entry rows
//! (`staged/entry.rs`) run on `StencilStage`.

use crate::bytecode::Register;
use crate::codes::{MEM, FIELD_ROW, SITE_OBJECT_AT, SITE_ROW, SITE_STORE_ROW, THIS_SITE_ROW, THIS_STORE_ROW, HELD_LINK_AT, HELD_ROW, LIVE_ROW, REALM_ROW};
use crate::heap_rows::{word_at};
use crate::layout::{JS_STEP_OBJECT_RECORD_BASE, JS_STEP_PROPERTY_BUFFER, JS_STEP_RECEIVER, JS_STEP_VM, PROPERTY_BUFFER_BASE};
use crate::numeric::{HOLE_A, HOLE_B, Out, S, deliver};
use crate::words::{hole, hole64, is_int, FIRST_TAG, HOLE_FOURTH};
use crate::layout::words::TAG_SHIFT;
use crate::places::{DEST, Places, settle, tagged_place};
use crate::places_property::{Callout, receiver_read, run_planned};
use crate::stage::StencilStage;
use crate::staged::{Constant, Operation, entry, step};
use crate::typed_ir::Type;

/// A field read of the loop entry's record (`FIELD_ROW`): the entry record's word, its byte offset
/// in hole 1.
#[inline(always)]
fn field_row<const D: u8>(s: S, payload: usize) -> Out {
    let mut stage = StencilStage::from_hole(HOLE_A);
    let mut places = Places::<D, MEM, MEM>::new(s);
    let word = run_planned(&mut stage, &mut places, [], Callout::LoopEntry, receiver_read());
    deliver::<D>(places.s, need!(places.s, word))
}

/// A field read through the site's cache entry (`entry::site_read`), or with `CHAIN` through two
/// (`entry::site_chain`, whose first and second entry offsets are hole 2's low and high halves):
/// `object` is the receiver the row names, read as the site's VM callout (`FieldGetHost`).
#[inline(always)]
fn site_row<const D: u8, const CHAIN: bool>(s: S, payload: usize, object: u64) -> Out {
    let header = word_at(payload, JS_STEP_PROPERTY_BUFFER) as usize;
    let mut stage = StencilStage::with_values(HOLE_B, word_at(header, PROPERTY_BUFFER_BASE));
    let mut places = Places::<D, MEM, MEM>::new(s);
    let read = Operation::FieldGetHost { object, atom: 0, site: 0 };
    let word = run_planned(&mut stage, &mut places, [], Callout::SiteRead { chained: CHAIN }, read);
    deliver::<D>(places.s, need!(places.s, word))
}

/// A store of the value operand (hole 1) through the site's cache entry (hole 2) into the object
/// `base` names (`SiteStore`, `StoreField`; `None` for the activation's receiver), through the
/// store gate where `GATED`. An ungated store takes a value the plan proved immediate and of the
/// slot's type, or a frame word the slot's int32 type admits once it is an int32: a leave writes
/// nothing.
#[inline(always)]
fn site_store_row<const A: u8, const GATED: bool>(s: S, payload: usize, base: Option<Register>) -> Out {
    let value = tagged_place::<A, HOLE_A>(&s);
    // An ungated frame operand is tested: an int32 passes; a double passes where the slot's
    // observed type holds Number, as hole 3 says (`STORE_ADMITS_DOUBLES`, `STORE_ADMITS_INT32`).
    if !GATED && A == MEM && !is_int(value) && (value.wrapping_sub(FIRST_TAG) >> TAG_SHIFT) < hole::<HOLE_FOURTH>() as u32 as u64 {
        return Out::Exit(s);
    }
    let header = word_at(payload, JS_STEP_PROPERTY_BUFFER) as usize;
    let mut stage = StencilStage::storing(HOLE_B, word_at(header, PROPERTY_BUFFER_BASE), payload);
    let mut places = Places::<MEM, A, MEM>::new(s);
    let store = Operation::StoreField { base, value, fact: Type::ANY, atom: 0, site: 0 };
    let stored = run_planned(&mut stage, &mut places, [], Callout::SiteStore { gated: GATED }, store);
    need!(places.s, stored);
    Out::Next(places.s)
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

/// The destination takes the word of hole 1: the staged `LoadConst` of that untyped constant.
#[inline(always)]
fn constant_row<const D: u8>(s: S) -> Out {
    let mut stage = StencilStage::from_hole(HOLE_A);
    let mut frame = Places::<D, MEM, MEM>::new(s);
    let constant = Constant::untyped(crate::Value::from_bits(hole64::<HOLE_A>()));
    let stepped = step(&mut stage, &mut frame, Operation::LoadConst { destination: DEST, constant });
    settle(frame, stepped, Out::Next)
}

/// The site row of kind `KIND`.
#[inline(always)]
pub(crate) fn run<const KIND: u8, const ENTRY: bool, const D: u8, const A: u8>(s: S, payload: usize) -> Out {
    match KIND {
        FIELD_ROW => field_row::<D>(s, payload),
        SITE_ROW => site_row::<D, ENTRY>(s, payload, register_object(&s)),
        SITE_STORE_ROW => site_store_row::<A, ENTRY>(s, payload, Some(DEST)),
        THIS_SITE_ROW => site_row::<D, ENTRY>(s, payload, word_at(payload, JS_STEP_RECEIVER)),
        THIS_STORE_ROW => site_store_row::<A, ENTRY>(s, payload, None),
        HELD_ROW => held_row::<D, ENTRY>(s, payload),
        LIVE_ROW => live_row(s),
        REALM_ROW => realm_row(s, payload),
        _ => constant_row::<D>(s),
    }
}
