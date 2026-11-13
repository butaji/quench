//! Scans of a function's code by op.

use super::{Function, Op, WideInstruction};

impl Function {
    /// The instructions whose op `wanted` accepts, in fixed-width form. An instruction another
    /// op owns is not decoded: a scan for a few ops reads one word of the rest.
    pub(crate) fn instructions_of<'a>(&'a self, wanted: impl Fn(Op) -> bool + 'a) -> impl Iterator<Item = WideInstruction> + 'a {
        self.code.iter().filter_map(move |&packed| {
            if packed.is_wide() {
                let wide = self.wide[packed.wide_index()];
                wanted(wide.op()).then_some(wide)
            } else {
                wanted(packed.op()).then(|| packed.as_wide())
            }
        })
    }
}
