//! The `Places` methods the key, iteration and naming rows reach through the staged steps: the
//! for-in fast arm the frame answers `CallOut::ForInNext` with, the `typeof` result table at hole 2,
//! and the record and empty-string words the site rows read from the payload.
//!
//! `Frame<Concrete> for Places` is one impl in `places.rs`, so that impl forwards to these methods:
//! `vm_step` to `call_out_fast`, `typeof_string` to `typeof_entry` (the lead adds the forwarders).

use crate::heap_rows::{payload, word_at};
use crate::layout::{JS_STEP_EMPTY_STRING, JS_STEP_FUNCTION_RECORD_BASE};
use crate::numeric::HOLE_B;
use crate::typed_ir::Representation;
use crate::places::{Places};
use crate::staged::region::Region;
use crate::staged::{CallOut, Concrete, Frame, Stage, iteration};
use crate::words::hole64;

impl Places {
    /// The object record table the object-walking rows read. The table left the payload with the object
    /// records (S7f), so the rows that need it decline.
    // S7f DESIGN: absent (`None`) until the staged walks (`instanceof_by_words`, `AddSite`) drop their records argument.
    #[inline(always)]
    pub(crate) fn object_records(&self) -> Option<u64> {
        None
    }

    /// The function record table the naming site reads (`JS_STEP_FUNCTION_RECORD_BASE`).
    #[inline(always)]
    pub(crate) fn function_records(&self) -> u64 {
        word_at(payload(&self.s), JS_STEP_FUNCTION_RECORD_BASE)
    }

    /// The empty string the naming site writes where a function has no name (`JS_STEP_EMPTY_STRING`).
    #[inline(always)]
    pub(crate) fn empty_string(&self) -> u64 {
        word_at(payload(&self.s), JS_STEP_EMPTY_STRING)
    }

    /// The fast arm of the VM's operations the frame answers in place: `CallOut::ForInNext` yields the
    /// state's next key where its memo proves the snapshot current (`iteration::for_in_next`, over the
    /// payload's record tables and epoch, `heap_records`); every other operation, and a state the
    /// memo does not prove, is `None` and leaves the row.
    #[inline(always)]
    pub(crate) fn call_out_fast(&mut self, stage: &mut Concrete, operation: CallOut) -> Option<(u64, Representation)> {
        match operation {
            CallOut::ForInNext { state } => {
                let state = self.tagged(stage, state);
                let (arrays, objects, epoch) = self.heap_records(stage)?;
                iteration::for_in_next(stage, (arrays, objects), state, epoch, ()).map(|key| (key, Representation::Tagged))
            }
            _ => None,
        }
    }

    /// The interned `typeof` result string of the name index `name`: the entry at that index of the
    /// table of the eight result strings at hole 2 (`value::immediate_typeof_string`'s load).
    #[inline(always)]
    pub(crate) fn typeof_entry(&mut self, stage: &mut Concrete, name: u64) -> Option<(u64, Representation)> {
        let offset = stage.mul_imm(name, core::mem::size_of::<crate::Value>() as u64);
        let entry = stage.add(hole64::<HOLE_B>(), offset);
        Some((stage.load_table(Region::HeapMeta.readonly(), entry), Representation::Tagged))
    }
}
