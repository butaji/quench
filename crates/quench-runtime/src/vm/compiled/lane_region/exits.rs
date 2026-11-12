//! A lane body's leave blocks: one per resume PC and set of registers it stores, filled once
//! the body is lowered. A leave stores only the live registers the body wrote whose lane
//! frame slot does not already hold their value (`LocalProofs::homed`), so the cold block of
//! an exit after a call writes back what changed since the call, not every register. A
//! register whose tagged view is unavailable is boxed in that cold block from its unboxed
//! view (`number_domains::box_unboxed`), which the hot path never builds for a leave.

use super::*;

/// The stack slots a materialized inline recovery builds its activations in: the registers'
/// snapshot and the frame descriptors.
type RecoverySlots = (ir::StackSlot, ir::StackSlot);

/// The caller registers a leave stores, by the view the cold block stores each from: the
/// current tagged bits, or the int32 or float64 view boxed there.
#[derive(PartialEq)]
pub(super) struct ExitStores {
    tagged: CompoundBitSet,
    int: CompoundBitSet,
    float: CompoundBitSet,
}

/// A leave block and the registers it stores to the lane frame.
pub(super) struct ExitBlock {
    pub(super) stored: ExitStores,
    pub(super) block: ir::Block,
}

impl Lowering<'_, '_> {
    /// The caller registers a leave at `pc` stores from `state`: written, live there and not homed.
    pub(super) fn stored_at(&self, state: &State, pc: usize) -> ExitStores {
        let mut stores = ExitStores { tagged: CompoundBitSet::new(), int: CompoundBitSet::new(), float: CompoundBitSet::new() };
        for register in self.written.iter().take_while(|register| *register < usize::from(self.unit.caller_registers)) {
            if !self.is_live(pc, register as Register) || state.proofs.homed.contains(register) { continue; }
            let fact = state.values[register];
            let view = match fact.value.representation {
                _ if fact.tagged.current() => &mut stores.tagged,
                Representation::Int32 => &mut stores.int,
                _ => &mut stores.float,
            };
            view.insert(register);
        }
        stores
    }

    /// Gives the live registers of the inlined frames a leave at `pc` recovers their tagged bits:
    /// the recovery reads them from the variables.
    pub(super) fn materialize_inlined(&mut self, state: &mut State, pc: usize) {
        let inlined = usize::from(self.unit.caller_registers)..state.values.len();
        let needed: Vec<usize> = inlined.filter(|register| self.is_live(pc, *register as Register))
            .filter(|register| !state.values[*register].tagged.current()).collect();
        for register in needed { self.tagged(state, register as Register); }
    }

    /// The block that stores `stored` and leaves at `pc`, made at its first leave.
    pub(super) fn exit_block(&mut self, pc: usize, stored: ExitStores) -> ir::Block {
        if let Some(held) = self.exits[pc].iter().find(|held| held.stored == stored) { return held.block; }
        let block = self.builder.create_block();
        self.builder.set_cold_block(block);
        self.exits[pc].push(ExitBlock { stored, block });
        block
    }

    /// The exit blocks: write back each one's registers, and an inlined callee's registers to the
    /// window its recovery makes the activation from, then leave at the origin.
    pub(super) fn fill_exits(&mut self) -> Option<()> {
        let slots = self.recovery_slots()?;
        for pc in 0..self.exits.len() {
            for ExitBlock { stored, block } in std::mem::take(&mut self.exits[pc]) {
                self.switch_to(block, true);
                self.store_exit_registers(&stored);
                let word = match self.unit.origins.get(pc).copied().unwrap_or(super::lane_unit::Origin::Caller(self.unit.end)) {
                    super::lane_unit::Origin::Caller(record) => u64::from(record),
                    super::lane_unit::Origin::Inlined { frame, pc: record } if self.completion == Completion::SourcePc => {
                        self.materialize_chain(pc, frame, record, slots.expect("materialized chain homes"))?;
                        u64::from(JS_INLINE_MATERIALIZED_PC)
                    }
                    super::lane_unit::Origin::Inlined { frame, pc: record } => {
                        self.store_inlined_registers(pc, frame);
                        LaneExit::Inlined { frame, pc: record }.word()
                    }
                };
                self.return_at(word);
            }
        }
        Some(())
    }

    /// Each register of `stored` to the lane frame: its tagged bits, or its unboxed view boxed here.
    fn store_exit_registers(&mut self, stored: &ExitStores) {
        let views = [(&stored.tagged, Representation::Tagged), (&stored.int, Representation::Int32),
            (&stored.float, Representation::Float64)];
        for (registers, representation) in views {
            for index in registers.iter() {
                let bits = match representation {
                    Representation::Tagged => self.builder.use_var(self.registers.tagged[index]),
                    _ => super::number_domains::box_unboxed(&mut self.builder, &mut self.registers, representation, index),
                };
                self.builder.ins().store(MemFlagsData::trusted(), bits, self.registers_base, register_offset(index as Register));
            }
        }
    }

    fn recovery_slots(&mut self) -> Option<Option<RecoverySlots>> {
        if self.completion != Completion::SourcePc || self.unit.frames.is_empty() { return Some(None); }
        let word = types::I64.bytes();
        let snapshot = self.builder.create_sized_stack_slot(ir::StackSlotData::new(
            ir::StackSlotKind::ExplicitSlot, u32::from(self.unit.registers - self.unit.caller_registers) * word,
            word.trailing_zeros() as u8));
        let depth = self.unit.recovery_depth();
        let descriptors = self.builder.create_sized_stack_slot(ir::StackSlotData::new(
            ir::StackSlotKind::ExplicitSlot,
            u32::try_from(depth.checked_mul(size_of::<InlineRecoveryFrame>())?).ok()?,
            align_of::<InlineRecoveryFrame>().trailing_zeros() as u8));
        Some(Some((snapshot, descriptors)))
    }

    /// Each register of the frame and the frames around it, at its unit register past the
    /// caller's (`LaneMethod::scratch_width`). Only the live ones: the activation reads no
    /// other (`dispatch_fast::lane_body_inlined_activation`).
    fn store_inlined_registers(&mut self, pc: usize, frame: usize) {
        let scratch = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, inlined));
        for index in self.unit.chain(frame) {
            let inlined = self.unit.frames[index];
            for register in inlined.base..inlined.base + inlined.registers {
                if !self.is_live(pc, register) { continue; }
                let bits = self.builder.use_var(self.registers.tagged[usize::from(register)]);
                self.builder.ins().store(MemFlagsData::trusted(), bits, scratch, register_offset(register - self.unit.caller_registers));
            }
        }
    }

    /// The suspended chain of inlined activations `frame` is in, built by the shared
    /// materialization helpers from the registers and descriptors stored in `slots`.
    fn materialize_chain(&mut self, pc: usize, frame: usize, record: u32, (snapshot, descriptors): RecoverySlots) -> Option<()> {
        let scratch = self.builder.ins().stack_addr(types::I64, snapshot, 0);
        let entries = self.builder.ins().stack_addr(types::I64, descriptors, 0);
        let chain = self.unit.chain(frame);
        for (depth, index) in chain.iter().copied().enumerate() {
            let inlined = self.unit.frames[index];
            self.snapshot_frame(pc, inlined, scratch);
            // Ancestors are suspended at the next child's call; the innermost
            // activation resumes at the exact operation whose admission failed.
            let source = chain.get(depth + 1).map_or(record, |child| self.unit.frames[*child].call);
            for (offset, value) in [
                (std::mem::offset_of!(InlineRecoveryFrame, function), inlined.target.function),
                (std::mem::offset_of!(InlineRecoveryFrame, call_pc), inlined.call),
                (std::mem::offset_of!(InlineRecoveryFrame, source_pc), source),
                (std::mem::offset_of!(InlineRecoveryFrame, registers), u32::from(inlined.base - self.unit.caller_registers)),
            ] {
                let bits = self.builder.ins().iconst(types::I32, i64::from(value));
                self.builder.ins().store(MemFlagsData::trusted(), bits, entries,
                    i32::try_from(depth * size_of::<InlineRecoveryFrame>() + offset).ok()?);
            }
        }
        self.call_materialization(frame, record, chain.len(), scratch, entries)
    }

    fn snapshot_frame(&mut self, pc: usize, inlined: super::lane_unit::Frame, scratch: ir::Value) {
        for register in inlined.base..inlined.base + inlined.registers {
            let bits = if self.is_live(pc, register) {
                self.builder.use_var(self.registers.tagged[usize::from(register)])
            } else {
                self.builder.ins().iconst(types::I64, Value::UNDEFINED.to_bits() as i64)
            };
            self.builder.ins().store(MemFlagsData::trusted(), bits, scratch,
                register_offset(register - self.unit.caller_registers));
        }
    }

    fn call_materialization(&mut self, frame: usize, record: u32, depth: usize, scratch: ir::Value,
        entries: ir::Value) -> Option<()> {
        if depth == 1 {
            let inlined = self.unit.frames[frame];
            let function = self.builder.ins().iconst(types::I64, i64::from(inlined.target.function));
            let call = self.builder.ins().iconst(types::I64, i64::from(inlined.call));
            let source = self.builder.ins().iconst(types::I64, i64::from(record));
            let scratch = self.builder.ins().iadd_imm_u(scratch,
                i64::from(register_offset(inlined.base - self.unit.caller_registers)));
            let helper = self.helpers.address(&mut self.builder, LaneHelper::MaterializeInline);
            emit_lane_helper_call(&mut self.builder, self.payload, helper,
                &[self.payload, function, call, source, scratch]);
        } else {
            let count = self.builder.ins().iconst(types::I64, i64::try_from(depth).ok()?);
            let helper = self.helpers.address(&mut self.builder, LaneHelper::MaterializeInlineChain);
            emit_lane_helper_call(&mut self.builder, self.payload, helper,
                &[self.payload, entries, count, scratch]);
        }
        Some(())
    }
}
