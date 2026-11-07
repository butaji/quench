//! Capture reads: dense slot loads and the VM's own lookups where they miss.
use super::*;

impl Lowering<'_, '_> {
    /// A depth-zero capture read: one load from the dense slots of the activation that reads
    /// it, the running one's (`Entry::capture_values`) or a published callee's, fixed by the
    /// identity its call guards (`Lowering::frame_captures`), where the slot holds an
    /// ordinary value; otherwise, in an inlined frame, the VM's own projection of the closure
    /// the call bound (`lane_function_capture_read`), which also resolves shared and
    /// statically bound callees' slots, else the lane's own read (`capture_miss`). Each read loads
    /// where it runs, after any call that may have stored to the slot.
    pub(super) fn load_capture(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, exit: ir::Block,
        admitted_callee: Option<ir::Value>, rooted: bool) -> ir::Value {
        let slot = crate::bytecode::ImmediateLayout::capture_slot(instruction.imm());
        let frame = match self.unit.origins.get(pc).copied() {
            Some(super::super::lane_unit::Origin::Inlined { frame, .. }) => Some(frame),
            _ => None,
        };
        let callee = admitted_callee.or_else(|| frame.map(|frame| self.tagged(state, self.unit.frames[frame].callee_register)));
        let dense = self.dense_captures(frame);
        let (slow, join) = (self.builder.create_block(), self.builder.create_block());
        self.builder.set_cold_block(slow);
        let value = self.builder.append_block_param(join, types::I64);
        match dense {
            Some((values, len)) => {
                let load = self.builder.create_block();
                let in_bounds = self.ins().icmp_imm_u(IntCC::UnsignedGreaterThan, len, i64::from(slot));
                self.ins().brif(in_bounds, load, &[], slow, &[]);
                self.builder.switch_to_block(load);
                let offset = i32::from(slot) * std::mem::size_of::<Value>() as i32;
                let read = self.ins().load(types::I64, MemFlagsData::trusted(), values, offset);
                let tag = self.ins().band_imm_u(read, crate::value::ValueLayout::TAG_MASK as i64);
                let ordinary = self.ins().icmp_imm_u(IntCC::NotEqual, tag, crate::value::ValueLayout::INTERNAL_TAG as i64);
                self.ins().brif(ordinary, join, &[ir::BlockArg::Value(read)], slow, &[]);
            }
            None => { self.ins().jump(slow, &[]); }
        }
        self.builder.switch_to_block(slow);
        let read = self.capture_miss(instruction, callee, exit, rooted);
        self.ins().jump(join, &[ir::BlockArg::Value(read)]);
        self.builder.switch_to_block(join);
        value
    }

    /// The dense slots (values, length) of the activation that runs `frame`: the running
    /// one's, or a published callee's its call's guarded identity fixes; none for any other.
    fn dense_captures(&mut self, frame: Option<usize>) -> Option<(ir::Value, ir::Value)> {
        match frame {
            None => {
                let entry = self.entry.as_ref().expect("entry facts");
                Some((self.builder.use_var(entry.capture_values), self.builder.use_var(entry.capture_len)))
            }
            Some(frame) if self.unit.frames[frame].target.identity().is_some() => {
                let (values, len) = self.frame_captures.get(frame).copied().unwrap_or((0, 0));
                Some((self.ins().iconst(types::I64, values as i64), self.ins().iconst(types::I64, len as i64)))
            }
            Some(_) => None,
        }
    }

    /// `load_capture` past its dense slots: the VM's projection of the closure an inlined
    /// frame's call bound, else the lane's own read of the activation's slots, root-held or
    /// not yet viewed, where it may read a root-held binding (`InvariantRead::rooted`); a
    /// miss, or a region with no lane context, leaves.
    fn capture_miss(&mut self, instruction: WideInstruction, callee: Option<ir::Value>, exit: ir::Block, rooted: bool) -> ir::Value {
        let Some(callee) = callee else {
            if rooted && self.completion == Completion::StatusAndPc { return self.capture_get(instruction, exit); }
            self.ins().jump(exit, &[]);
            let dead = self.builder.create_block();
            self.builder.switch_to_block(dead);
            return self.ins().iconst(types::I64, 0);
        };
        let vm = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, vm));
        let slot = self.ins().iconst(types::I64, i64::from(crate::bytecode::ImmediateLayout::capture_slot(instruction.imm())));
        let helper = self.helpers.address(self.builder.ins(), LaneHelper::FunctionCapture);
        let read = emit_lane_call(&mut self.builder, helper, &[vm, callee, slot]);
        let hit = self.ins().icmp_imm_u(IntCC::NotEqual, read, Value::DELETED.to_bits() as i64);
        emit_js_guard(&mut self.builder, hit, exit);
        read
    }

    /// A capture's read by the lane's own lookup (`dispatch_fast::lane_body_capture`); a miss leaves.
    pub(super) fn capture_get(&mut self, instruction: WideInstruction, exit: ir::Block) -> ir::Value {
        let context = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, lane_context));
        let (depth, slot) = (crate::bytecode::ImmediateLayout::capture_depth(instruction.imm()), crate::bytecode::ImmediateLayout::capture_slot(instruction.imm()));
        let packed = self.ins().iconst(types::I64, i64::from(u32::from(depth) << u16::BITS | u32::from(slot)));
        let value = self.helper_call(LaneHelper::CaptureGet, &[context, packed]);
        let hit = self.ins().icmp_imm_u(IntCC::NotEqual, value, Value::DELETED.to_bits() as i64);
        emit_js_guard(&mut self.builder, hit, exit);
        value
    }
}
