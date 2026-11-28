//! The class-keyed add row: a property added to a live class cell (`add::class_add`). The object-record add
//! rows and the naming rows are gone with the object record table (S7j4a): their add and naming words named
//! records, and the planner lays the helper step behind every add a class cell does not key.

use crate::layout::{JS_STEP_PROPERTY_BUFFER, PROPERTY_BUFFER_BASE};
use crate::numeric::{HOLE_B, Out, S};
use crate::words::hole64;
use crate::places::{LEFT, Places, finish_fast};
use crate::codes::{MEM, ADD_AFTER_AT, ADD_OBJECT_AT, ADD_SLOT_BITS};
use crate::stage::StencilStage;
use crate::staged::add;
use crate::heap_rows::word_at;

/// The class-keyed add row (`ADD_CLASS_ROW`): the value operand (hole 1) joins the live class cell of the object register
/// at the frame offset hole 2 carries, by the class transition holes 0 and 2 carry (`add::class_add`, whose before-key is
/// the low word of hole 0 and after-class its high word). A hit continues at TAKEN.
#[inline(always)]
fn class_add_row<const A: u8>(s: S, payload: usize) -> Out {
    let header = word_at(payload, JS_STEP_PROPERTY_BUFFER) as usize;
    let mut stage = StencilStage::storing(HOLE_B, word_at(header, PROPERTY_BUFFER_BASE), payload);
    let mut frame = Places::new::<MEM, A, MEM>(s);
    let (shapes, site) = (hole64::<0>(), hole64::<HOLE_B>());
    let edge = add::Edge { before: shapes & u64::from(u32::MAX), after: shapes >> ADD_AFTER_AT, slot: site & ((1 << ADD_SLOT_BITS) - 1) };
    let object = word_at(frame.s.frame, (site >> ADD_OBJECT_AT) as usize);
    let value = frame.tagged(&mut stage, LEFT);
    let done = add::class_add(&mut stage, object, edge, value, ()).is_some();
    finish_fast(frame, done)
}

/// The class-keyed add row (`ADD_CLASS_ROW`): the only add row the catalog keeps.
#[inline(always)]
pub(crate) fn run<const A: u8>(s: S, payload: usize) -> Out {
    class_add_row::<A>(s, payload)
}
