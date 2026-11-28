//! A sibling tail call (DESIGN.md 4.4): a returning call of an owned body, which the running body's frame becomes the
//! callee's frame for. The body rewrites its own native header and body entries to the callee's, and enters the callee's
//! entry by `return_call_indirect`, so the callee's return completes the running body's caller. Where the callee's entry
//! is the stub (its body was withdrawn) or the running frame is not a native header, the call is made as any other.

use super::*;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

impl<'a, 'f> Lowering<'a, 'f> {
    /// The arguments window of the sibling tail call at `pc` (`CallSite::tail`): a returning call of a body the running
    /// body owns (`callee`), whose plan is an ordinary body's (`CallPlan::tail_callee`), which the caller's site permits
    /// (`Function::tail_site_permitted`), passed no more arguments than the callee's parameters from a register window.
    pub(crate) fn sibling_tail_arguments(&self, pc: usize, record: u32, site: &Invocation, callee: &DirectCallee) -> Option<(Register, u16)> {
        let Arguments::Window { first, count } = site.arguments else { return None };
        let function = self.program.function_get(self.function_id as usize)?;
        let body = self.program.function_get(callee.target.function as usize)?;
        let admitted = self.completion == Completion::Body
            && matches!(self.coverage, Coverage::Prefix | Coverage::Full)
            && site.returns && !site.constructs() && !site.forwards
            && matches!(site.callee, quench_runtime::staged::forms::Callee::Register(_))
            && callee.method.is_some()
            && usize::from(count) <= usize::from(body.params)
            && function.tail_site_permitted(record)
            && body.call_plan(self.program).tail_callee
            && self.vm.ahead.and_then(|ahead| ahead.calls.get(&pc)).is_none_or(Vec::is_empty);
        admitted.then_some((first, count))
    }
}

impl EmitFrame<'_> {
    /// Branches a sibling tail call to `tail_jump` where the callee's entry is a body (not the stub, which enters nothing
    /// and would restart the call) and the running frame's header is native (not one Rust observed, which `pop_native`
    /// reads as the cold path); the block continues where the call pushes its own frame.
    pub(super) fn tail_branch(&self, stage: &mut Emit<'_, '_>, owned: &DirectCallee, this: ir::Value, room: Room<ir::Value>, captures: ir::Value,
        entry: quench_runtime::staged::machine::CallTarget<ir::Value>, arguments: (Register, u16)) -> Option<()> {
        let call = self.call_site()?;
        let stub = stage.constant(crate::compiled::lane_body_abi::entry_stub() as u64);
        let unstubbed = stage.sub(entry.word(), stub);
        let bodied = stage.ne_imm(unstubbed, 0_u64);
        let packed = stage.load_u64(Region::Frame.mutable(), call.registers, context_at(quench_runtime::staged::frame::CONTEXT_PACKED));
        let native = stage.and_imm(packed, quench_runtime::staged::frame::NATIVE_FLAG);
        let native = stage.ne_imm(native, 0_u64);
        let tail = stage.all(bodied, native);
        let jump = stage.builder.create_block();
        let pushed = stage.builder.create_block();
        stage.branch(tail, (jump, &[]), (pushed, &[]));
        stage.builder.switch_to_block(jump);
        self.tail_jump(stage, owned, this, room, captures, entry, arguments)?;
        stage.builder.switch_to_block(pushed);
        Some(())
    }

    /// The sibling tail call's jump (`tail_branch`). The arguments are read before the window is written, which overlaps
    /// them (`reuse_frame_in_lane`). The running header takes the callee's packed and identity words, and its index's
    /// body and descriptor take the callee's; its call record, caller registers and caller base stay. The window is the
    /// callee's, so the values' top is the window's end for the callee's registers, and the parameters take the arguments
    /// (the ones the call does not pass are undefined). The callee's entry takes the body convention, as the shim calls it.
    fn tail_jump(&self, stage: &mut Emit<'_, '_>, owned: &DirectCallee, this: ir::Value, room: Room<ir::Value>, captures: ir::Value,
        entry: quench_runtime::staged::machine::CallTarget<ir::Value>, (first, count): (Register, u16)) -> Option<()> {
        use quench_runtime::staged::frame::{CONTEXT_CALLEE, CONTEXT_PACKED, CONTEXT_THIS};
        let call = self.call_site()?;
        let frame = Region::Frame.mutable();
        let arguments: Vec<ir::Value> = (0..count).map(|index| stage.load_u64(FRAME, call.registers, register_offset(first + index))).collect();
        let packed = self.callee_packed(stage, DirectSite::Owned(owned))?;
        let identity = self.callee_identity(stage, DirectSite::Owned(owned))?;
        stage.store_u64(frame, call.registers, context_at(CONTEXT_PACKED), packed);
        stage.store_u64(frame, call.registers, context_at(CONTEXT_CALLEE), identity);
        stage.store_u64(frame, call.registers, context_at(CONTEXT_THIS), this);
        // The running frame is the innermost one, so its index is one below the frame count the room was admitted at.
        let index = stage.add_imm(room.shadow, u64::MAX);
        let offset = stage.mul_imm(index, quench_runtime::staged::word_bytes(1));
        let bodies = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, head.body_table));
        let body_at = stage.add(bodies, offset);
        let body = stage.load_u64(ENTRY_CELL, room.cell, field(std::mem::offset_of!(EntryCell, body)));
        stage.store_u64(frame, body_at, 0, body);
        let descriptors = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, head.native_table));
        let descriptor_at = stage.add(descriptors, offset);
        let descriptor = self.callee_descriptor(stage, DirectSite::Owned(owned))?;
        stage.store_u64(frame, descriptor_at, 0, descriptor);
        let word = std::mem::size_of::<Value>() as u64;
        let context = quench_runtime::vm::register_file::FRAME_CONTEXT as u64;
        let next = stage.add_imm(call.registers, (u64::from(owned.registers) + context) * word);
        self.set_values_top(stage, next);
        let undefined = stage.constant(Value::UNDEFINED.to_bits());
        for index in 0..usize::from(owned.target.params) {
            let value = arguments.get(index).copied().unwrap_or(undefined);
            stage.store_u64(FRAME, call.registers, register_offset(index as Register), value);
        }
        let signature = stage.builder.import_signature(crate::compiled::lane_body_abi::body_signature_in_lane(types::I64));
        stage.builder.ins().return_call_indirect(signature, entry.word(), &[call.context, this, captures, call.registers]);
        Some(())
    }
}
