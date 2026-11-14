//! The add and naming rows: a property added along a shape edge to a record, and a function
//! named through its shape, the staged add and naming rows (`staged/{add,naming}.rs`) run on
//! `StencilStage`.

use crate::layout::{JS_STEP_EMPTY_STRING, JS_STEP_FUNCTION_RECORD_BASE, JS_STEP_OBJECT_RECORD_BASE, JS_STEP_PROPERTY_BUFFER, JS_STEP_RECEIVER, PROPERTY_BUFFER_BASE};
use crate::numeric::{HOLE_A, HOLE_B, Out, S};
use crate::words::hole64;
use crate::places::tagged_place;
use crate::stage::StencilStage;
use crate::staged::object::Form;
use crate::staged::{Frame, Stage, add, naming};
use crate::codes::{ADD_AFTER_AT, ADD_ARENA_ROW, ADD_EITHER_ROW, ADD_LINK_AT, ADD_LINK_BITS, ADD_OBJECT_AT, ADD_PACKED_ROW, ADD_SLOT_BITS, THIS_ADD_ARENA_ROW, THIS_ADD_EITHER_ROW, THIS_ADD_PACKED_ROW, NAME_EITHER_ROW, NAME_FUNCTION_AT};
use crate::heap_rows::word_at;

/// The add row of `form`: the value operand (hole 1) is added to `object` by the transition hole 0
/// (before-shape below `ADD_AFTER_AT`, after-shape above) and hole 2 (slot, and with `CHAINED` the
/// prototype link the receiver must have) name (`add::site_add`); a hit continues at TAKEN.
#[inline(always)]
fn add_row<const A: u8, const CHAINED: bool>(s: S, payload: usize, form: Form, object: impl FnOnce(u64) -> u64) -> Out {
    let header = word_at(payload, JS_STEP_PROPERTY_BUFFER) as usize;
    let mut stage = StencilStage::storing(HOLE_B, word_at(header, PROPERTY_BUFFER_BASE), payload);
    let (shapes, site) = (hole64::<0>(), hole64::<HOLE_B>());
    let edge = add::Edge { before: shapes & u64::from(u32::MAX), after: shapes >> ADD_AFTER_AT, slot: site & ((1 << ADD_SLOT_BITS) - 1) };
    let link = CHAINED.then(|| (site >> ADD_LINK_AT) & ((1 << ADD_LINK_BITS) - 1));
    let records = word_at(payload, JS_STEP_OBJECT_RECORD_BASE);
    need!(s, add::site_add(&mut stage, records, object(site >> ADD_OBJECT_AT), (edge, link), form, tagged_place::<A, HOLE_A>(&s), ()));
    Out::Taken(s)
}

/// The add row of `form` to the object register whose frame offset hole 2 carries.
#[inline(always)]
fn register_add_row<const A: u8, const CHAINED: bool>(s: S, payload: usize, form: Form) -> Out {
    let frame = s.frame;
    add_row::<A, CHAINED>(s, payload, form, |offset| word_at(frame, offset as usize))
}

/// The add row of `form` to the activation's receiver, which the payload holds.
#[inline(always)]
fn this_add_row<const A: u8, const CHAINED: bool>(s: S, payload: usize, form: Form) -> Out {
    add_row::<A, CHAINED>(s, payload, form, |_| word_at(payload, JS_STEP_RECEIVER))
}

/// The naming row of `form`: the function in the frame slot hole 2 names takes the name word hole 1 holds
/// in the slot hole 2 carries, along the shapes hole 0 carries (`naming::site_name`); a hit continues at TAKEN.
#[inline(always)]
fn name_row(s: S, payload: usize, form: Form) -> Out {
    let header = word_at(payload, JS_STEP_PROPERTY_BUFFER) as usize;
    let mut stage = StencilStage::storing(HOLE_B, word_at(header, PROPERTY_BUFFER_BASE), payload);
    let (shapes, site) = (hole64::<0>(), hole64::<HOLE_B>());
    let edge = add::Edge { before: shapes & u64::from(u32::MAX), after: shapes >> ADD_AFTER_AT, slot: site & ((1 << ADD_SLOT_BITS) - 1) };
    let records = word_at(payload, JS_STEP_FUNCTION_RECORD_BASE);
    let function = word_at(s.frame, (site >> NAME_FUNCTION_AT) as usize);
    let empty = word_at(payload, JS_STEP_EMPTY_STRING);
    need!(s, naming::site_name(&mut stage, records, function, (edge, empty), form, hole64::<HOLE_A>(), ()));
    Out::Taken(s)
}

/// The add or naming row of kind `KIND`: `ENTRY` is `CHAINED` for an add.
#[inline(always)]
pub(crate) fn run<const KIND: u8, const ENTRY: bool, const A: u8>(s: S, payload: usize) -> Out {
    match KIND {
        ADD_PACKED_ROW => register_add_row::<A, ENTRY>(s, payload, Form::Packed),
        ADD_ARENA_ROW => register_add_row::<A, ENTRY>(s, payload, Form::Arena),
        ADD_EITHER_ROW => register_add_row::<A, ENTRY>(s, payload, Form::Either),
        THIS_ADD_PACKED_ROW => this_add_row::<A, ENTRY>(s, payload, Form::Packed),
        THIS_ADD_ARENA_ROW => this_add_row::<A, ENTRY>(s, payload, Form::Arena),
        THIS_ADD_EITHER_ROW => this_add_row::<A, ENTRY>(s, payload, Form::Either),
        NAME_EITHER_ROW => name_row(s, payload, Form::Either),
        _ => name_row(s, payload, Form::Arena),
    }
}
