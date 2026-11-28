//! The `Places` methods the key and naming rows reach through the staged steps: the `typeof` result
//! table at hole 2. `Frame<Concrete> for Places` is one impl in `places.rs`, which forwards
//! `typeof_string` to `typeof_entry`.

use crate::numeric::HOLE_B;
use crate::typed_ir::Representation;
use crate::places::{Places};
use crate::staged::region::Region;
use crate::staged::{Concrete, Stage};
use crate::words::hole64;

impl Places {
    /// The interned `typeof` result string of the name index `name`: the entry at that index of the
    /// table of the eight result strings at hole 2 (`value::immediate_typeof_string`'s load).
    #[inline(always)]
    pub(crate) fn typeof_entry(&mut self, stage: &mut Concrete, name: u64) -> Option<(u64, Representation)> {
        let offset = stage.mul_imm(name, core::mem::size_of::<crate::Value>() as u64);
        let entry = stage.add(hole64::<HOLE_B>(), offset);
        Some((stage.load_table(Region::HeapMeta.readonly(), entry), Representation::Tagged))
    }
}
