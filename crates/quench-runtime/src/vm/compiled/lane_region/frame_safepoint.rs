//! The staged interrupt/collection safepoint for a lane frame.

use super::EmitFrame;
use super::super::{emit_js_guard, Emit, EmitProof};
use cranelift_codegen::ir::{types, InstBuilder, MemFlagsData, condcodes::IntCC};

/// A fault-polling body records its poll instruction; any other leaves at the miss edge when
/// the flag is set.
pub(super) fn safepoint(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, proof: EmitProof) -> Option<()> {
    let EmitProof::Safepoint { interrupt, miss, fault_poll } = proof else { return None };
    let requested = stage.builder.ins().atomic_load(types::I8, MemFlagsData::trusted(), interrupt);
    if fault_poll {
        let cranelift_codegen::ir::ValueDef::Result(inst, _) = stage.builder.func.dfg.value_def(requested) else { return None };
        frame.polls.push(inst);
    } else {
        let miss = miss?;
        let requested = stage.builder.ins().uextend(types::I32, requested);
        let clear = stage.builder.ins().icmp_imm_u(IntCC::Equal, requested, 0);
        emit_js_guard(stage.builder, clear, miss);
    }
    Some(())
}
