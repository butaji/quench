//! The `Places` methods the key, iteration and naming rows reach through the staged steps: the
//! for-in fast arm the frame answers `CallOut::ForInNext` with and the `typeof` result table at hole 2.
//!
//! `Frame<Concrete> for Places` is one impl in `places.rs`, so that impl forwards to these methods:
//! `vm_step` to `call_out_fast`, `typeof_string` to `typeof_entry` (the lead adds the forwarders).

use crate::numeric::HOLE_B;
use crate::typed_ir::Representation;
use crate::places::{Places};
use crate::staged::region::Region;
use crate::staged::{CallOut, Concrete, Frame, Stage, iteration};
use crate::words::hole64;

impl Places {
    /// The fast arm of the VM's operations the frame answers in place: `CallOut::ForInNext` yields the
    /// state's next key where its memo proves the snapshot current (`iteration::for_in_next`, over the
    /// payload's record tables and epoch, `heap_records`); every other operation, and a state the
    /// memo does not prove, is `None` and leaves the row.
    #[inline(always)]
    pub(crate) fn call_out_fast(&mut self, stage: &mut Concrete, operation: CallOut) -> Option<(u64, Representation)> {
        match operation {
            CallOut::ForInNext { state } => {
                let state = self.tagged(stage, state);
                let (arrays, epoch) = self.heap_records(stage)?;
                iteration::for_in_next(stage, arrays, state, epoch, ()).map(|key| (key, Representation::Tagged))
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
