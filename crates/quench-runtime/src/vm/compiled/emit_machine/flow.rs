//! The flow family of the machine: `MachineFlow` as Cranelift control transfers.

use super::{Builds, machine_type};
use crate::staged::machine::{Lane, MachineFlow};
#[cfg(all(feature = "aot", feature = "jit"))]
use crate::staged::machine::MachineTail;
use cranelift_codegen::ir::{self, InstBuilder};

/// The arguments of a block call.
fn block_args(values: &[ir::Value]) -> Vec<ir::BlockArg> {
    values.iter().copied().map(ir::BlockArg::Value).collect()
}

impl<'b, B: Builds<'b>> MachineFlow for B {
    type Block = ir::Block;
    type Table = ir::JumpTable;

    fn new_block(&mut self) -> ir::Block {
        self.function_builder().create_block()
    }

    fn enter(&mut self, block: ir::Block) {
        let builder = self.function_builder();
        builder.switch_to_block(block);
        builder.seal_block(block);
    }

    fn enter_open(&mut self, block: ir::Block) {
        self.function_builder().switch_to_block(block);
    }

    fn seal_all(&mut self) {
        self.function_builder().seal_all_blocks();
    }

    fn goto(&mut self, block: ir::Block, arguments: &[ir::Value]) {
        self.function_builder().ins().jump(block, &block_args(arguments));
    }

    fn block_parameter(&mut self, block: ir::Block, lane: Lane) -> ir::Value {
        self.function_builder().append_block_param(block, machine_type(lane))
    }

    fn jump_table(&mut self, cases: &[ir::Block], (default, arguments): (ir::Block, &[ir::Value])) -> ir::JumpTable {
        let builder = self.function_builder();
        let default = builder.func.dfg.block_call(default, &block_args(arguments));
        let cases: Vec<_> = cases.iter().map(|block| builder.func.dfg.block_call(*block, &[])).collect();
        builder.create_jump_table(ir::JumpTableData::new(default, &cases))
    }

    fn dispatch(&mut self, selector: ir::Value, table: ir::JumpTable) {
        self.function_builder().ins().br_table(selector, table);
    }

    fn leave(&mut self, values: &[ir::Value]) {
        self.function_builder().ins().return_(values);
    }

    fn branch_if(&mut self, flag: ir::Value, then: (ir::Block, &[ir::Value]), otherwise: (ir::Block, &[ir::Value])) {
        // A flag the lowering already knows takes its one edge: the other side is not built
        // into the CFG (no block, dominator or allocation work for a branch never taken).
        if let Some((known, _)) = super::scalar::known_integer(self.function_builder(), flag) {
            let (block, arguments) = if known != 0 { then } else { otherwise };
            let arguments = block_args(arguments);
            self.function_builder().ins().jump(block, &arguments);
            return;
        }
        let (taken, other) = (block_args(then.1), block_args(otherwise.1));
        self.function_builder().ins().brif(flag, then.0, &taken, otherwise.0, &other);
    }
}

#[cfg(all(feature = "aot", feature = "jit"))]
impl<'b, B: Builds<'b>> MachineTail for B {
    fn tail_call(&mut self, target: ir::FuncRef, arguments: &[ir::Value]) {
        self.function_builder().ins().return_call(target, arguments);
    }

    fn tail_call_indirect(&mut self, signature: ir::SigRef, target: ir::Value, arguments: &[ir::Value]) {
        self.function_builder().ins().return_call_indirect(signature, target, arguments);
    }
}
