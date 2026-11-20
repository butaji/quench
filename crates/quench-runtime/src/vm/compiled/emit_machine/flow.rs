//! The flow family of the machine: `MachineFlow` as Cranelift control transfers.

use super::{Builds, machine_type};
use crate::staged::machine::{Lane, MachineFlow, MachineVariables};
#[cfg(feature = "jit")]
use crate::staged::machine::MachineTail;
use cranelift_codegen::ir::{self, InstBuilder};

/// The arguments of a block call.
/// Whether `flag` is a constant, or a comparison of two constants, and which.
fn known_flag(dfg: &ir::DataFlowGraph, flag: ir::Value) -> Option<bool> {
    let constant = |value: ir::Value| match dfg.value_def(dfg.resolve_aliases(value)) {
        ir::ValueDef::Result(inst, 0) => match dfg.insts[inst] {
            ir::InstructionData::UnaryImm { opcode: ir::Opcode::Iconst, imm } => Some(imm.bits()),
            _ => None,
        },
        _ => None,
    };
    if let Some(bits) = constant(flag) { return Some(bits != 0); }
    let ir::ValueDef::Result(inst, 0) = dfg.value_def(dfg.resolve_aliases(flag)) else { return None };
    match dfg.insts[inst] {
        ir::InstructionData::IntCompare { opcode: ir::Opcode::Icmp, cond, args: [left, right] } => {
            let (left, right) = (constant(left)?, constant(right)?);
            match cond {
                ir::condcodes::IntCC::Equal => Some(left == right),
                ir::condcodes::IntCC::NotEqual => Some(left != right),
                _ => None,
            }
        }
        _ => None,
    }
}

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
        let builder = self.function_builder();
        // A flag the stage already knows takes its edge: the other arm is not a predecessor of the
        // join, so code after it stays dominated by the values computed before the branch (a
        // diamond whose dead arm joins later makes Cranelift recompute them at each later use).
        if let Some(known) = known_flag(&builder.func.dfg, flag) {
            let (block, arguments) = if known { (then.0, taken) } else { (otherwise.0, other) };
            builder.ins().jump(block, &arguments);
            return;
        }
        builder.ins().brif(flag, then.0, &taken, otherwise.0, &other);
    }
}

impl<'b, B: Builds<'b>> MachineVariables for B {
    type Variable = cranelift_frontend::Variable;

    fn declare_variable(&mut self, lane: Lane) -> cranelift_frontend::Variable {
        self.function_builder().declare_var(machine_type(lane))
    }

    fn define_variable(&mut self, variable: cranelift_frontend::Variable, value: ir::Value) {
        self.function_builder().def_var(variable, value);
    }

    fn read_variable(&mut self, variable: cranelift_frontend::Variable) -> ir::Value {
        self.function_builder().use_var(variable)
    }

    /// The opposite comparison where `flag` is an integer comparison.
    fn compare_complement(&mut self, flag: ir::Value) -> Option<ir::Value> {
        let builder = self.function_builder();
        let ir::ValueDef::Result(inst, _) = builder.func.dfg.value_def(flag) else { return None };
        match builder.func.dfg.insts[inst] {
            ir::InstructionData::IntCompare { cond, args: [left, right], .. } => Some(builder.ins().icmp(ir::condcodes::CondCode::complement(cond), left, right)),
            _ => None,
        }
    }
}

#[cfg(feature = "jit")]
impl<'b, B: Builds<'b>> MachineTail for B {
    fn tail_call(&mut self, target: ir::FuncRef, arguments: &[ir::Value]) {
        self.function_builder().ins().return_call(target, arguments);
    }

    fn tail_call_indirect(&mut self, signature: ir::SigRef, target: ir::Value, arguments: &[ir::Value]) {
        self.function_builder().ins().return_call_indirect(signature, target, arguments);
    }
}
