//! A lane body's leave blocks: one per resume PC and set of registers it stores, filled once
//! the body is lowered. A leave stores only the live registers the body wrote whose lane
//! frame slot does not already hold their value (`LocalProofs::homed`), so the cold block of
//! an exit after a call writes back what changed since the call, not every register. A
//! register whose tagged view is unavailable is boxed in that cold block from its unboxed
//! view (`number_domains::box_unboxed`), which the hot path never builds for a leave.

use super::*;

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
    pub(super) fn fill_exits(&mut self) {
        for pc in 0..self.exits.len() {
            for ExitBlock { stored, block } in std::mem::take(&mut self.exits[pc]) {
                self.switch_to(block, true);
                self.store_exit_registers(&stored);
                let word = match self.unit.origins.get(pc).copied().unwrap_or(super::lane_unit::Origin::Caller(self.unit.end)) {
                    super::lane_unit::Origin::Caller(record) => LaneExit::At(record as usize).word(),
                    super::lane_unit::Origin::Inlined { frame, pc: record } => {
                        self.store_inlined_registers(pc, frame);
                        LaneExit::Inlined { frame, pc: record }.word()
                    }
                };
                self.return_at(word);
            }
        }
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
                self.frame_store(bits, self.registers_base, register_offset(index as Register));
            }
        }
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
                self.frame_store(bits, scratch, register_offset(register - self.unit.caller_registers));
            }
        }
    }
}
