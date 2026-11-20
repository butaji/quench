//! A lane body's leave blocks: one per resume PC and set of registers it stores, filled once
//! the body is lowered. A leave stores only the live registers the body wrote whose lane
//! frame slot does not already hold their value (`LocalProofs::homed`), so the cold block of
//! an exit after a call writes back what changed since the call, not every register. A
//! register whose tagged view is unavailable is boxed in that cold block from its unboxed
//! view (`number_domains::box_unboxed`), which the hot path never builds for a leave.

use super::*;
use super::super::exit_protocol::{LiveValue, Source};

/// The caller registers a leave stores, by the view the cold block stores each from: the
/// current tagged bits, or the int32, 64-bit integer or float64 view boxed there.
#[derive(PartialEq)]
pub(super) struct ExitStores {
    tagged: CompoundBitSet,
    int: CompoundBitSet,
    long: CompoundBitSet,
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
        let mut stores = ExitStores { tagged: CompoundBitSet::new(), int: CompoundBitSet::new(), long: CompoundBitSet::new(), float: CompoundBitSet::new() };
        for register in self.written.iter().take_while(|register| *register < usize::from(self.unit.caller_registers)) {
            if !self.is_live(pc, register as Register) || state.proofs.homed.contains(register) { continue; }
            let fact = state.values[register];
            // An unboxed register leaves boxed from its unboxed view, which every path defines. Its tagged view
            // may be a word one arm of a branching row boxed (`register_tagged`): the leave's block, shared by
            // every leave that stores the same registers, does not see it, and the tagged variable is stale on
            // the arms that did not box it (an element read's generic arm left a loop counter `undefined`).
            let view = match fact.value.representation {
                Representation::Tagged => &mut stores.tagged,
                Representation::Int32 => &mut stores.int,
                Representation::Int64 => &mut stores.long,
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
            // An unboxed register's tagged word is boxed again here, where it dominates the leave (`stored_at`).
            .filter(|register| !state.values[*register].tagged.current() || state.values[*register].value.representation != Representation::Tagged).collect();
        for register in needed { self.tagged(state, register as Register); }
    }

    /// The block that stores `stored` and leaves at `pc`, made at its first leave.
    pub(super) fn exit_block(&mut self, pc: usize, stored: ExitStores) -> ir::Block {
        if let Some(held) = self.exits[pc].iter().find(|held| held.stored == stored) { return held.block; }
        let block = cold_block(&mut self.builder);
        self.exits[pc].push(ExitBlock { stored, block });
        block
    }

    /// The block that stores `stored` and leaves at the exit word it is passed, shared by every
    /// caller-frame leave of a body that stores the same registers.
    pub(super) fn shared_exit_block(&mut self, stored: ExitStores) -> ir::Block {
        if let Some(held) = self.shared_exits.iter().find(|held| held.stored == stored) { return held.block; }
        let block = cold_block(&mut self.builder);
        self.builder.append_block_param(block, ir::types::I64);
        self.shared_exits.push(ExitBlock { stored, block });
        block
    }

    /// The exit blocks: write back each one's registers, and an inlined callee's registers to the
    /// window its recovery makes the activation from, then leave at the origin. `None` where an
    /// ahead body has a leave no base body entry continues (`leave_to_base`): no lane body then.
    pub(super) fn fill_exits(&mut self) -> Option<()> {
        if super::clif_census::enabled() {
            let count = |stored: &ExitStores| [&stored.tagged, &stored.int, &stored.long, &stored.float].iter().map(|set| set.iter().count()).sum::<usize>();
            let private: Vec<(usize, usize)> = self.exits.iter().flatten().map(|exit| (count(&exit.stored), usize::from(matches!(self.unit.origins.first(), Some(_))))).collect();
            let boxed: usize = self.exits.iter().flatten().chain(self.shared_exits.iter()).map(|exit| [&exit.stored.int, &exit.stored.long, &exit.stored.float].iter().map(|set| set.iter().count()).sum::<usize>()).sum();
            eprintln!("QUENCH_EXIT_CENSUS {{\"shared\":{},\"shared_stores\":{},\"private\":{},\"private_stores\":{},\"boxed\":{boxed}}}",
                self.shared_exits.len(), self.shared_exits.iter().map(|exit| count(&exit.stored)).sum::<usize>(), private.len(), private.iter().map(|(n, _)| n).sum::<usize>());
        }
        for ExitBlock { stored, block } in std::mem::take(&mut self.shared_exits) {
            self.switch_to(block, true);
            let word = self.builder.block_params(block)[0];
            self.store_exit_registers(&stored);
            let stop = self.stop;
            self.goto(stop, &[word]);
        }
        for pc in 0..self.exits.len() {
            for ExitBlock { stored, block } in std::mem::take(&mut self.exits[pc]) {
                self.switch_to(block, true);
                self.store_exit_registers(&stored);
                if self.vm.ahead.is_some() { self.leave_to_base(pc)?; continue; }
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
        Some(())
    }

    /// Continues the activation at the base body's record this leave's origin names
    /// (`AheadLeaves`). The stores are written and the entry words are the base body's, so it runs
    /// on the same Frame; an inlined callee's registers go to the scratch window first.
    // TODO: materialize the callee's activation (`body_inlined_activation_in_lane`); AOT has no helper for it yet.
    /// `None` where the base body enters no record at the leave's origin.
    pub(super) fn leave_to_base(&mut self, pc: usize) -> Option<()> {
        let (namespace, key) = self.base_leave(pc)?;
        if let Some(super::lane_unit::Origin::Inlined { frame, .. }) = self.unit.origins.get(pc).copied() {
            self.store_inlined_registers(pc, frame);
        }
        let target = crate::vm::jit_backend::symbol_address(&mut self.builder, namespace, key);
        let signature = self.builder.func.signature.clone();
        let signature = self.builder.import_signature(signature);
        let (this, captures) = self.entry_words;
        self.builder.ins().return_call_indirect(signature, target, &[self.context, this, captures, self.registers_base]);
        Some(())
    }

    /// The `(namespace, key)` of the base body's entry the leave at `pc` continues at.
    fn base_leave(&self, pc: usize) -> Option<(u32, u32)> {
        let leaves = self.vm.ahead.expect("an ahead lowering names its base entries");
        leaves.targets.get(pc).copied().flatten()
    }

    /// Each register of `stored` to the lane frame: its tagged bits, or its unboxed view boxed here.
    fn store_exit_registers(&mut self, stored: &ExitStores) {
        let views = [(&stored.tagged, Representation::Tagged), (&stored.int, Representation::Int32),
            (&stored.long, Representation::Int64), (&stored.float, Representation::Float64)];
        for (registers, representation) in views {
            for index in registers.iter() {
                let bits = match representation {
                    Representation::Tagged => self.builder.use_var(self.registers.tagged[index]),
                    Representation::Int32 => super::number_domains::box_unboxed(&mut self.builder, &mut self.registers, representation, index),
                    // A double boxes out of line: the leave is cold, and the inline encoding
                    // makes two blocks and fifteen instructions per register.
                    _ => {
                        let number = match representation {
                            Representation::Int64 => {
                                let variable = super::frame::register_long(&mut self.builder, &mut self.registers, index);
                                let word = self.builder.use_var(variable);
                                super::object_slots::emit(&mut self.builder).raw_unary(crate::staged::machine::Unary::FromSigned(crate::staged::machine::Lane::F64), word)
                            }
                            _ => {
                                let variable = super::frame::register_float(&mut self.builder, &mut self.registers, index);
                                self.builder.use_var(variable)
                            }
                        };
                        let bits = super::object_slots::emit(&mut self.builder).raw_unary(crate::staged::machine::Unary::Bitcast(crate::staged::machine::Lane::I64), number);
                        self.helper_call(LaneHelper::BoxNumber, &[bits])
                    }
                };
                self.store_word(regions::frame(), bits, self.registers_base, register_offset(index as Register));
            }
        }
    }

    /// The values a leave at `pc` inside `frame` stores: the live registers of the frame and of
    /// the frames around it (`is_live`, the one liveness query), each at its unit register past the
    /// caller's (`LaneMethod::scratch_width`). The exit table records this same list (`ExitTable::of_unit`).
    pub(super) fn inlined_stores(&self, pc: usize, frame: usize) -> Vec<LiveValue> {
        self.unit.chain(frame).into_iter().flat_map(|index| {
            let inlined = self.unit.frames[index];
            inlined.base..inlined.base + inlined.registers
        }).filter(|register| self.is_live(pc, *register))
            .map(|register| LiveValue { slot: register, source: Source::Scratch(register - self.unit.caller_registers) })
            .collect()
    }

    /// Stores the values `inlined_stores` gives a leave at `pc` inside `frame` to the scratch
    /// window. Only the live ones: the activation reads no other (`dispatch_fast::body_inlined_activation_in_lane`).
    fn store_inlined_registers(&mut self, pc: usize, frame: usize) {
        let scratch = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, inlined));
        for LiveValue { slot, source: Source::Scratch(at) } in self.inlined_stores(pc, frame) {
            let bits = self.builder.use_var(self.registers.tagged[usize::from(slot)]);
            self.store_word(regions::frame(), bits, scratch, register_offset(at));
        }
    }
}
