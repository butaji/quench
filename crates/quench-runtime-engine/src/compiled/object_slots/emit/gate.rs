//! The store gate's word as the u-jit emits it: the I8 a staged barrier test answers over the dispatch payload's
//! store-watch word (`Stage::store_gate`, `Stage::held_store_gate`).

use super::*;
use quench_runtime::staged::machine::Lane;

impl Emit<'_, '_> {
    /// The I8 gate the staged `test` answers over the dispatch payload's store-watch word: 1 where it continues,
    /// 0 where it leaves (at a cold block); 0 for a stage with no payload.
    pub(super) fn gate_word(&mut self, test: impl FnOnce(&mut Self, ir::Value, usize, ir::Block) -> Option<()>) -> ir::Value {
        let Some(payload) = self.gate else { let zero = self.raw_const(Lane::I64, 0); return self.ult(zero, zero) };
        let (logged, join) = (self.builder.create_block(), self.builder.create_block());
        self.builder.set_cold_block(logged);
        let open = self.builder.append_block_param(join, types::I8);
        let watch = std::mem::offset_of!(super::super::super::JsStepDispatch, head.store_watch);
        test(self, payload, watch, logged).expect("an Emit guard always continues");
        let yes = self.raw_const(Lane::I8, 1);
        self.goto(join, &[yes]);
        self.builder.switch_to_block(logged);
        let no = self.raw_const(Lane::I8, 0);
        self.goto(join, &[no]);
        self.builder.switch_to_block(join);
        open
    }
}
