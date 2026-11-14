//! The flow family of the machine: `MachineFlow` as Cranelift control transfers.

use super::Builds;
use crate::staged::machine::MachineFlow;
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

    fn goto(&mut self, block: ir::Block, arguments: &[ir::Value]) {
        self.function_builder().ins().jump(block, &block_args(arguments));
    }

    fn dispatch(&mut self, selector: ir::Value, table: ir::JumpTable) {
        self.function_builder().ins().br_table(selector, table);
    }

    fn leave(&mut self, values: &[ir::Value]) {
        self.function_builder().ins().return_(values);
    }

    fn branch_if(&mut self, flag: ir::Value, then: (ir::Block, &[ir::Value]), otherwise: (ir::Block, &[ir::Value])) {
        let (taken, other) = (block_args(then.1), block_args(otherwise.1));
        self.function_builder().ins().brif(flag, then.0, &taken, otherwise.0, &other);
    }
}
