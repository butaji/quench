//! The site rows: a field read or store through the site's cache entry, a prototype-held field,
//! a code dependency's liveness and the realm proof, each the staged entry rows
//! (`staged/entry.rs`) run on `StencilStage`.

use crate::bytecode::{FieldBase, FieldLookup, Register};
use crate::codes::{MEM, FIELD_ROW, SITE_ROW, SITE_STORE_ROW, THIS_SITE_ROW, THIS_STORE_ROW, HELD_LINK_AT, HELD_ROW, LIVE_ROW, REALM_ROW};
use crate::heap_rows::{context_word, word_at};
use crate::layout::{CONTEXT_THIS, JS_STEP_PROPERTY_BUFFER, JS_STEP_VM, PROPERTY_BUFFER_BASE};
use crate::numeric::{HOLE_A, HOLE_B, Out, S, deliver};
use crate::words::{hole, hole64, is_int, FIRST_TAG, HOLE_FOURTH, LAST_HOLE};
use crate::layout::words::TAG_SHIFT;
use crate::places::{DEST, Places, settle, site_object, tagged_place};
use crate::places_property::{Callout, HeldRead, PlannedPlaces, planned_frame, receiver_read};
use crate::stage::StencilStage;
use crate::staged::{Constant, Operation, Step, op_kind, step_of, step_property_of};
use crate::typed_ir::Type;

/// A field read of the loop entry's record (`FIELD_ROW`): the entry record's word, its byte offset
/// in hole 1.
#[inline(always)]
fn field_row<const D: u8>(s: S, payload: usize) -> Out {
    let mut stage = StencilStage::from_hole(HOLE_A);
    let mut places = Places::new::<D, MEM, MEM>(s);
    let step = step_property_of::<{ op_kind::GET_FIELD }, _, _>(&mut stage, &mut planned_frame(&mut places, [], Callout::LoopEntry), receiver_read());
    let word = match step {
        Step::Value(word) => Some(word),
        _ => None,
    };
    deliver::<D>(places.s, need!(places.s, word))
}

/// A field read through the site's cache entry (`entry::site_read`), or with `CHAIN` through two
/// (`entry::site_chain`, whose first and second entry offsets are hole 2's low and high halves):
/// `object` is the receiver the row names, read as the site's VM callout (`FieldGetHost`).
#[inline(always)]
fn site_row<const D: u8, const CHAIN: bool>(s: S, payload: usize, object: u64) -> Out {
    let header = word_at(payload, JS_STEP_PROPERTY_BUFFER) as usize;
    let mut stage = StencilStage::with_values(HOLE_B, word_at(header, PROPERTY_BUFFER_BASE));
    let mut places = Places::new::<D, MEM, MEM>(s);
    let read = Operation::FieldGetHost { object, atom: 0, site: 0 };
    let step = step_property_of::<{ op_kind::FIELD_GET_HOST }, _, _>(&mut stage, &mut planned_frame(&mut places, [], Callout::SiteRead { chained: CHAIN }), read);
    let word = match step {
        Step::Value(word) => Some(word),
        _ => None,
    };
    deliver::<D>(places.s, need!(places.s, word))
}

/// A store of the value operand (hole 1) through the site's cache entry (hole 2) into the object
/// `object` names (`entry::site_store`), through the store gate where `GATED`. An ungated store
/// takes a value the plan proved immediate and of the slot's type, or a frame word the slot's int32
/// type admits once it is an int32 (`StoreArm::holds`): a leave writes nothing. That proof is of
/// the slot of one shape, hole 4 (`Decoded::slot_shape`); the entry is the site's live one, which
/// may since name another shape, so an ungated store leaves for a receiver of any other shape.
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
    let mut places = Places::new::<MEM, A, MEM>(s);
    let store = Operation::StoreField { base, value, fact: Type::ANY, atom: 0, site: 0 };
    let step = step_property_of::<{ op_kind::STORE_FIELD }, _, _>(&mut stage, &mut planned_frame(&mut places, [], Callout::SiteStore { gated: GATED }), store);
    let stored = match step {
        Step::Value(word) => Some(word),
        _ => None,
    };
    need!(places.s, stored);
    Out::Next(places.s)
}

/// A prototype-held field folded to its value (hole 2): the object, the activation's receiver for an `ENTRY` row and else
/// the register the site names, is read as the staged `GetField` of its exact shape and prototype link (hole 1's low and
/// high halves), which leaves the row otherwise (`PrototypeProof::Linked`, the planned frame of `PlannedPlaces::held`).
#[inline(always)]
fn held_row<const D: u8, const ENTRY: bool>(s: S, payload: usize) -> Out {
    let object = if ENTRY { context_word(s.frame, CONTEXT_THIS) } else { site_object(&s) };
    let receiver = hole64::<HOLE_A>();
    let link = (receiver >> HELD_LINK_AT) as u32;
    let held = HeldRead { object, shape: receiver as u32, link, constant: [(link, hole64::<HOLE_B>())] };
    let mut stage = StencilStage::from_hole(HOLE_B);
    let mut places = Places::new::<D, MEM, MEM>(s);
    let mut frame = PlannedPlaces { places: &mut places, ops: [], callout: Callout::Plan, lexical: None, held: Some(held), receiver: None };
    let read = Operation::GetField { base: FieldBase::THIS, lookup: FieldLookup::Site(0), destination: DEST };
    let word = match step_property_of::<{ op_kind::GET_FIELD }, _, _>(&mut stage, &mut frame, read) {
        Step::Value(word) => Some(word),
        _ => None,
    };
    deliver::<D>(places.s, need!(places.s, word))
}

/// Continues while the invalidation word is zero: a Stable fact the code holds changed where it is set
/// (`guard::live`, whose frame reads the word at the address in hole 1).
#[inline(always)]
fn live_row(s: S) -> Out {
    let mut frame = Places::new::<MEM, MEM, MEM>(s);
    let held = matches!(step_of::<{ op_kind::GUARD_LIVE }, _, _>(&mut StencilStage::from_hole(0), &mut frame, Operation::GuardLive), Step::Value(_));
    if held { Out::Next(frame.s) } else { Out::Exit(frame.s) }
}

/// Continues while the active realm's global object (the word at hole 1's byte offset in the VM the
/// payload names) is the one in hole 2, which the code folded facts of (`guard::realm`); else the taken
/// successor, the entry's failure.
#[inline(always)]
fn realm_row(s: S) -> Out {
    let mut frame = Places::new::<MEM, MEM, MEM>(s);
    let guard = Operation::GuardRealm { expected: hole64::<HOLE_B>() };
    let held = matches!(step_of::<{ op_kind::GUARD_REALM }, _, _>(&mut StencilStage::from_hole(0), &mut frame, guard), Step::Value(_));
    if held { Out::Next(frame.s) } else { Out::Taken(frame.s) }
}

/// The destination takes the word of hole 1: the staged `LoadConst` of that untyped constant.
#[inline(always)]
fn constant_row<const D: u8>(s: S) -> Out {
    let mut stage = StencilStage::from_hole(HOLE_A);
    let mut frame = Places::new::<D, MEM, MEM>(s);
    let constant = Constant::untyped(crate::Value::from_bits(hole64::<HOLE_A>()));
    let stepped = step_of::<{ op_kind::LOAD_CONST }, _, _>(&mut stage, &mut frame, Operation::LoadConst { destination: DEST, constant });
    settle(frame, stepped, Out::Next)
}

/// The site row of kind `KIND`.
#[inline(always)]
pub(crate) fn run<const KIND: u8, const ENTRY: bool, const D: u8, const A: u8>(s: S, payload: usize) -> Out {
    match KIND {
        FIELD_ROW => field_row::<D>(s, payload),
        SITE_ROW => site_row::<D, ENTRY>(s, payload, site_object(&s)),
        SITE_STORE_ROW => site_store_row::<A, ENTRY>(s, payload, Some(DEST)),
        THIS_SITE_ROW => site_row::<D, ENTRY>(s, payload, context_word(s.frame, CONTEXT_THIS)),
        THIS_STORE_ROW => site_store_row::<A, ENTRY>(s, payload, None),
        HELD_ROW => held_row::<D, ENTRY>(s, payload),
        LIVE_ROW => live_row(s),
        REALM_ROW => realm_row(s),
        _ => constant_row::<D>(s),
    }
}
