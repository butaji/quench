//! Effect-only branches of the specializer's stage (`Stage::effect_if`, `Stage::effect_if_both`).

use super::Emit;
use quench_runtime::staged::machine::MachineFlow;
use cranelift_codegen::ir;

impl Emit<'_, '_> {
    /// `then` where `first` holds and then `second` does (computed only past `first`), else `cold`, laid out once
    /// out of line; no word joins the arms.
    pub(super) fn effect_arms(&mut self, first: ir::Value, second: Option<impl FnOnce(&mut Self) -> ir::Value>,
        then: impl FnOnce(&mut Self), cold: impl FnOnce(&mut Self)) {
        let (then_block, cold_block, join) = (self.builder.create_block(), self.builder.create_block(), self.builder.create_block());
        self.builder.set_cold_block(cold_block);
        match second {
            Some(second) => {
                let check = self.builder.create_block();
                self.branch_if(first, (check, &[]), (cold_block, &[]));
                self.builder.switch_to_block(check);
                self.builder.seal_block(check);
                let second = second(self);
                self.branch_if(second, (then_block, &[]), (cold_block, &[]));
            }
            None => self.branch_if(first, (then_block, &[]), (cold_block, &[])),
        }
        self.builder.switch_to_block(then_block);
        self.builder.seal_block(then_block);
        then(self);
        self.goto(join, &[]);
        self.builder.switch_to_block(cold_block);
        self.builder.seal_block(cold_block);
        cold(self);
        self.goto(join, &[]);
        self.builder.switch_to_block(join);
        self.builder.seal_block(join);
    }
}
